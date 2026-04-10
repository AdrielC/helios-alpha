//! Arrow-style **Scan** composition: split/merge/choose/fanin, **decomposition** (`ArrowApply`, `ZipTuple`),
//! **conditional emit** (`EmitWhen`), **Vec** collection (`step_collect`), and `scan_then!`.
//!
//! Run: `cargo run -p helio_scan --example arrow_pipeline`

use helio_scan::{
    and_then, Arr, ArrowScanExt, Choose, Dup, Either, Fanin, FilterMap, First, Id, Map, Merge,
    MergeIn, OnLeft, Scan, Second, Split, SplitOut, ZipTuple,
};

fn main() {
    // --- step_collect: Vec of 0..N outputs per step (no emit = filter) ---
    let mut id_st = Id::<i32>::new().init();
    let v = Id::<i32>::new().step_collect(&mut id_st, 7);
    println!("=== step_collect(Id, 7) ===\n  {v:?}");

    let dup = Dup::<i32>::new();
    let mut d_st = dup.init();
    println!(
        "=== Dup: two emits ===\n  {:?}",
        dup.step_collect(&mut d_st, 3)
    );

    // --- EmitWhen: rolling sum only after buffer full (saturated window) ---
    struct Window3;
    impl Scan for Window3 {
        type In = i32;
        type Out = i32;
        type State = [i32; 3];
        fn init(&self) -> Self::State {
            [0; 3]
        }
        fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: helio_scan::Emit<Self::Out>,
        {
            state[0] = state[1];
            state[1] = state[2];
            state[2] = input;
            emit.emit(state[0] + state[1] + state[2]);
        }
    }
    let gated = Window3.emit_when(|st: &[i32; 3]| st[0] != 0);
    let mut g_st = gated.init();
    println!(
        "=== EmitWhen (3-slot window, emit when full) ===\n  after 1,2: {:?}\n  after 3: {:?}",
        {
            gated.step_collect(&mut g_st, 1);
            gated.step_collect(&mut g_st, 2)
        },
        gated.step_collect(&mut g_st, 3)
    );

    // --- Decomposition: ArrowApply (env, operand); ZipTuple / both! parallel tuple ---
    let app = Arr::new(|x: i32| x * 10).apply::<String>();
    let mut a_st = app.init();
    println!(
        "=== ArrowApply: (env ignored, operand scaled) ===\n  {:?}",
        app.step_collect(&mut a_st, ("ctx".into(), 4))
    );

    let zt = ZipTuple {
        left: Arr::new(|x: i32| x + 1),
        right: Arr::new(|x: i32| x * 2),
    };
    let mut z_st = zt.init();
    println!(
        "=== ZipTuple (2, 5) ===\n  {:?}",
        zt.step_collect(&mut z_st, (2, 5))
    );

    let both_pipe = helio_scan::scan_both!(Arr::new(|s: String| s.len() as i32), Arr::new(|n: i32| n + 1));
    let mut b_st = both_pipe.init();
    println!(
        "=== scan_both! (String len, int bump) ===\n  {:?}",
        both_pipe.step_collect(&mut b_st, ("hello".into(), 40))
    );

    // --- OnLeft: sum stream, ignore Right branch ---
    let left_only = OnLeft::<_, String>::new(Arr::new(|x: i32| x + 100));
    let mut ol = left_only.init();
    println!(
        "=== OnLeft ===\n  left: {:?}\n  right dropped: {:?}",
        left_only.step_collect(&mut ol, Either::Left(1)),
        left_only.step_collect(&mut ol, Either::Right("skip".into()))
    );

    // --- Original demos ---
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

    // Mega: Fanin → Choose → MergeIn adapter → Merge
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

    // Compose mega with post-label map using and_then
    let mega_labeled = and_then(
        mega,
        Map {
            inner: Id::<Either<String, String>>::new(),
            map: |e| format!("OUT:{e:?}"),
        },
    );

    println!("\n=== Mega + Id map (single scan_then + and_then) — input 5 ===");
    let mut st = mega_labeled.init();
    for line in mega_labeled.step_collect(&mut st, 5) {
        println!("  {line}");
    }

    println!("\n=== First / Second ===");
    let with_session = First::new(Arr::new(|x: i32| x * 3));
    let mut st2 = with_session.init();
    println!("  {:?}", with_session.step_collect(&mut st2, (42, "sess-9".to_string())));

    let mut st3 = split_tagged.init();
    println!(
        "\n=== Split + map on 10 ===\n  {:?}",
        split_tagged.step_collect(&mut st3, 10)
    );

    let mut st4 = macro_pipe.init();
    println!(
        "\n=== scan_then! on 3 ===\n  {:?}",
        macro_pipe.step_collect(&mut st4, 3)
    );

    let sec = Second::new(Arr::new(|x: i32| x + 1));
    let mut st5 = sec.init();
    println!(
        "\n=== Second ===\n  {:?}",
        sec.step_collect(&mut st5, ("meta".to_string(), 99))
    );

    let ext = Arr::new(|x: i32| x + 1).split(Arr::new(|x: i32| x * x));
    let mut st6 = ext.init();
    println!(
        "\n=== ArrowScanExt::split ===\n  {:?}",
        ext.step_collect(&mut st6, 4)
    );
}
