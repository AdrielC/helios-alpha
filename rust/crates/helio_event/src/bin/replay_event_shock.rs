//! `cargo run -p helio_event --bin replay_event_shock -- --events FILE --bars FILE [--as-of SEC] [--out DIR]`
//!
//! Loads generic event shocks and daily bars, runs the vertical pipeline, writes CSV + markdown summary.

use helio_event::*;
use helio_scan::{FlushReason, FlushableScan, Scan, VecEmitter};
use helio_time::{AvailableAt, SimpleWeekdayCalendar};
use std::fs;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "Usage: replay_event_shock --events <csv|jsonl> --bars <csv> [--as-of <epoch_sec>] [--out <dir>] [--events-format csv|jsonl]"
    );
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut events_path: Option<PathBuf> = None;
    let mut bars_path: Option<PathBuf> = None;
    let mut out_dir = PathBuf::from("event_shock_out");
    let mut as_of: Option<i64> = None;
    let mut events_format = "csv".to_string();

    while let Some(a) = args.first().cloned() {
        args.remove(0);
        match a.as_str() {
            "--events" => events_path = Some(PathBuf::from(args.remove(0))),
            "--bars" => bars_path = Some(PathBuf::from(args.remove(0))),
            "--out" => out_dir = PathBuf::from(args.remove(0)),
            "--as-of" => as_of = Some(args.remove(0).parse().expect("as-of int")),
            "--events-format" => events_format = args.remove(0),
            "--help" | "-h" => usage(),
            other => {
                eprintln!("unknown arg: {other}");
                usage();
            }
        }
    }

    let events_path = events_path.expect("missing --events");
    let bars_path = bars_path.expect("missing --bars");

    let events_raw = fs::read_to_string(&events_path).expect("read events");
    let shocks = match events_format.as_str() {
        "jsonl" => load_event_shocks_jsonl(&events_raw).expect("parse events"),
        _ => load_event_shocks_csv(&events_raw).expect("parse events"),
    };

    let bars_raw = fs::read_to_string(&bars_path).expect("read bars");
    let bars = load_daily_bars_csv(&bars_raw).expect("parse bars");
    let candidates = candidate_entries_from_bars(&bars);
    let replay = build_vertical_replay(shocks, bars);

    let cal = SimpleWeekdayCalendar;
    let xlu = Symbol("XLU".into());
    let spy = Symbol("SPY".into());
    let vertical = EventShockVerticalScan::new(
        as_of.map(AvailableAt),
        EventShockFilterConfig::default(),
        cal,
        ExitPolicy::FixedHorizonSessions { n: 5 },
        Exposure::Pair {
            long: xlu.clone(),
            short: spy.clone(),
        },
        EventShockControlConfig {
            seed: 42,
            controls_per_treatment: 1,
            horizon_sessions: 5,
            exposure: Exposure::Pair {
                long: xlu,
                short: spy,
            },
            vol_epsilon: None,
        },
        candidates,
    );

    let mut st = vertical.init();
    let mut trades = VecEmitter::new();
    for r in &replay {
        vertical.step(&mut st, r.clone(), &mut trades);
    }
    vertical.flush(&mut st, FlushReason::EndOfInput, &mut trades);
    let trade_vec = trades.into_inner();

    fs::create_dir_all(&out_dir).ok();
    let trades_csv = out_dir.join("trades.csv");
    let mut w = csv::Writer::from_path(&trades_csv).expect("trades csv");
    w.write_record([
        "event_id",
        "entry_session",
        "exit_session",
        "gross_return",
        "max_drawdown",
        "holding_sessions",
        "matched_treatment",
    ])
    .ok();
    for t in &trade_vec {
        w.write_record([
            t.event_id.0.to_string(),
            t.entry_session.0.to_string(),
            t.exit_session.0.to_string(),
            format!("{:.6}", t.gross_return),
            format!("{:.6}", t.max_drawdown),
            t.holding_period_sessions.to_string(),
            t.matched_treatment
                .map(|m| m.0.to_string())
                .unwrap_or_default(),
        ])
        .ok();
    }
    w.flush().ok();

    let fold = EventShockMetricsFoldScan::default();
    let mut st_f = fold.init();
    let mut summaries = VecEmitter::new();
    for t in &trade_vec {
        let label = if let Some(m) = t.matched_treatment {
            LabeledTradeResult::Control {
                matched_event_id: m,
                trade: t.clone(),
            }
        } else {
            LabeledTradeResult::Treatment(t.clone())
        };
        fold.step(&mut st_f, label, &mut summaries);
    }
    let sum_vec = summaries.into_inner();
    let last = sum_vec.last().cloned();

    let summary_csv = out_dir.join("summary.csv");
    let mut ws = csv::Writer::from_path(&summary_csv).expect("summary csv");
    ws.write_record([
        "count",
        "mean_return",
        "median_return",
        "hit_rate",
        "std_dev",
        "bootstrap_mean_diff_vs_control",
    ])
    .ok();
    if let Some(s) = &last {
        ws.write_record([
            s.count.to_string(),
            format!("{:.6}", s.mean_return),
            format!("{:.6}", s.median_return),
            format!("{:.6}", s.hit_rate),
            format!("{:.6}", s.std_dev),
            s.bootstrap_mean_diff_vs_control
                .map(|x| format!("{:.6}", x))
                .unwrap_or_default(),
        ])
        .ok();
    }
    ws.flush().ok();

    let md = out_dir.join("report.md");
    let mut report = String::from("# Event shock replay\n\n");
    report.push_str("## Trades\n\n");
    report.push_str("| event | entry | exit | return | mdd | ctrl? |\n|---|---|---|---|---|---|\n");
    for t in &trade_vec {
        let ctrl = t
            .matched_treatment
            .map(|m| format!("{}", m.0))
            .unwrap_or_else(|| "-".into());
        report.push_str(&format!(
            "| {} | {} | {} | {:.4} | {:.4} | {} |\n",
            t.event_id.0, t.entry_session.0, t.exit_session.0, t.gross_return, t.max_drawdown, ctrl
        ));
    }
    if let Some(s) = last {
        report.push_str("\n## Summary\n\n");
        report.push_str(&format!(
            "- n = {}\n- mean = {:.6}\n- median = {:.6}\n- hit rate = {:.4}\n- std = {:.6}\n",
            s.count, s.mean_return, s.median_return, s.hit_rate, s.std_dev
        ));
        if let Some(b) = s.bootstrap_mean_diff_vs_control {
            report.push_str(&format!(
                "- bootstrap mean (treatment − control pairs) = {:.6}\n",
                b
            ));
        }
    }
    fs::write(&md, report).expect("write report");

    println!("Wrote {:?}, {:?}, {:?}", trades_csv, summary_csv, md);
}
