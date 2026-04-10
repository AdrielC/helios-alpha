//! One **mega-pipeline** using arrow combinators: [`Arr`], [`First`], [`Second`], [`Split`],
//! [`Merge`], [`Choose`], [`Fanin`], [`scan_then!`](helio_scan::scan_then), [`Map`](helio_scan::Map),
//! [`FilterMap`](helio_scan::FilterMap).
//!
//! Run: `cargo run -p helio_scan --example arrow_pipeline`

use helio_scan::{
    Arr, ArrowScanExt, Choose, Either, Fanin, FilterMap, First, Map, Merge, MergeIn, Scan, Second,
    Split, SplitOut, VecEmitter,
};

fn main() {
    // --- Small demos ---
    let split_tagged = Map {
        inner: Split {
            left: Arr::new(|x: i32| x + 7),
            right: Arr::new(|x: i32| -x),
        },
        map: |o: SplitOut<i32, i32>| match o {
            SplitOut::A(v) => format!("split-plus7:{v}"),
            SplitOut::B(v) => format!("split-neg:{v}"),
        },
    };

    let macro_pipe = helio_scan::scan_then!(
        Split {
            left: Arr::new(|x: i32| x.saturating_mul(2)),
            right: Arr::new(|x: i32| x.saturating_add(100)),
        },
        FilterMap {
            inner: Arr::new(|o: SplitOut<i32, i32>| o),
            f: |o| match o {
                SplitOut::A(v) if v > 0 => Some(format!("macro-A:{v}")),
                SplitOut::B(v) if v % 2 == 0 => Some(format!("macro-B:{v}")),
                _ => None,
            },
        }
    );

    // --- Mega composition (single `scan_then!`): Fanin → Choose → label lengths → Merge ---
    // Input: i32. Fanin runs +1 and *2; Choose formats each side; adapter turns strings into
    // MergeIn::L/R(len); Merge pretty-prints each branch.
    let mega = helio_scan::scan_then!(
        Fanin {
            left: Arr::new(|x: i32| x + 1),
            right: Arr::new(|x: i32| x * 2),
        },
        Choose {
            left: Arr::new(|x: i32| format!("L{x}")),
            right: Arr::new(|x: i32| format!("R{x}")),
        },
        Arr::new(|e: Either<String, String>| match e {
            Either::Left(s) => MergeIn::L(s.len() as i32),
            Either::Right(s) => MergeIn::R(s.len() as i32),
        }),
        Merge {
            left: Arr::new(|n: i32| format!("merged-L({n})")),
            right: Arr::new(|n: i32| format!("merged-R({n})")),
        }
    );

    println!("=== Mega pipeline: Fanin → Choose → Arr(MergeIn) → Merge — input 5 ===");
    let mut st = mega.init();
    let mut out = VecEmitter::new();
    mega.step(&mut st, 5, &mut out);
    for line in out.into_inner() {
        println!("  {line:?}");
    }

    println!("\n=== First: (value, session) → triple value ===");
    let with_session = First::new(Arr::new(|x: i32| x * 3));
    let mut st2 = with_session.init();
    let mut o2 = VecEmitter::new();
    with_session.step(&mut st2, (42, "sess-9".to_string()), &mut o2);
    println!("  {:?}", o2.into_inner());

    println!("\n=== Split + map on 10 ===");
    let mut st3 = split_tagged.init();
    let mut o3 = VecEmitter::new();
    split_tagged.step(&mut st3, 10, &mut o3);
    println!("  {:?}", o3.into_inner());

    println!("\n=== scan_then! macro pipe on 3 ===");
    let mut st4 = macro_pipe.init();
    let mut o4 = VecEmitter::new();
    macro_pipe.step(&mut st4, 3, &mut o4);
    println!("  {:?}", o4.into_inner());

    println!("\n=== Second: (session, value) ===");
    let sec = Second::new(Arr::new(|x: i32| x + 1));
    let mut st5 = sec.init();
    let mut o5 = VecEmitter::new();
    sec.step(&mut st5, ("meta".to_string(), 99), &mut o5);
    println!("  {:?}", o5.into_inner());

    println!("\n=== ArrowScanExt::split ===");
    let ext = Arr::new(|x: i32| x + 1).split(Arr::new(|x: i32| x * x));
    let mut st6 = ext.init();
    let mut o6 = VecEmitter::new();
    ext.step(&mut st6, 4, &mut o6);
    println!("  {:?}", o6.into_inner());
}
