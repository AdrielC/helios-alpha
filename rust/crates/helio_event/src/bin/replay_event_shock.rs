//! `cargo run -p helio_event --bin replay_event_shock -- --events FILE --bars FILE [--as-of SEC] [--out DIR]`
//!
//! Loads normalized events and daily bars, runs the event-shock vertical, writes CSV + markdown.

use helio_event::*;
use helio_scan::{FlushReason, FlushableScan, Scan, VecEmitter};
use helio_time::{AvailableAt, SimpleWeekdayCalendar};
use std::fs;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "Usage: replay_event_shock --events <file> --bars <file> [--as-of <epoch_sec>] [--out <dir>] [--events-format csv|jsonl|solar]"
    );
    eprintln!("  solar  = CSV columns id,available_at,impact_start,impact_end,severity,confidence (demo / ingest)");
    std::process::exit(2);
}

fn treatment_trades(trades: &[TradeResult]) -> Vec<&TradeResult> {
    trades
        .iter()
        .filter(|t| t.matched_treatment.is_none())
        .collect()
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        f64::NAN
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
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
        "solar" => load_solar_event_shocks_csv(&events_raw).expect("parse solar events"),
        _ => load_event_shocks_csv(&events_raw).expect("parse events"),
    };
    let n_events = shocks.len();

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

    let summary_csv = out_dir.join("summary.csv");
    let mut ws = csv::Writer::from_path(&summary_csv).expect("summary csv");
    ws.write_record([
        "n_events_ingested",
        "n_trade_rows",
        "n_treatment_trades",
        "count",
        "mean_return",
        "median_return",
        "hit_rate",
        "std_dev",
        "bootstrap_mean_diff_vs_control",
        "mean_treatment_return",
        "mean_control_return_paired",
        "mean_excess_treatment_vs_control",
    ])
    .ok();

    let treat_refs = treatment_trades(&trade_vec);
    let n_treatment = treat_refs.len();
    let treat_returns: Vec<f64> = treat_refs.iter().map(|t| t.gross_return).collect();
    let mean_treat = mean(&treat_returns);

    use std::collections::HashMap;
    let mut tmap: HashMap<u64, f64> = HashMap::new();
    for t in &trade_vec {
        if t.matched_treatment.is_none() {
            tmap.insert(t.event_id.0, t.gross_return);
        }
    }
    let mut ctrl_returns = Vec::new();
    let mut excess = Vec::new();
    for t in &trade_vec {
        if let Some(mid) = t.matched_treatment {
            if let Some(&tr) = tmap.get(&mid.0) {
                ctrl_returns.push(t.gross_return);
                excess.push(tr - t.gross_return);
            }
        }
    }
    let mean_ctrl = mean(&ctrl_returns);
    let mean_excess = mean(&excess);

    if let Some(s) = &last {
        ws.write_record([
            n_events.to_string(),
            trade_vec.len().to_string(),
            n_treatment.to_string(),
            s.count.to_string(),
            format!("{:.6}", s.mean_return),
            format!("{:.6}", s.median_return),
            format!("{:.6}", s.hit_rate),
            format!("{:.6}", s.std_dev),
            s.bootstrap_mean_diff_vs_control
                .map(|x| format!("{:.6}", x))
                .unwrap_or_default(),
            format!("{:.6}", mean_treat),
            if ctrl_returns.is_empty() {
                String::new()
            } else {
                format!("{:.6}", mean_ctrl)
            },
            if excess.is_empty() {
                String::new()
            } else {
                format!("{:.6}", mean_excess)
            },
        ])
        .ok();
    } else {
        ws.write_record([
            n_events.to_string(),
            trade_vec.len().to_string(),
            n_treatment.to_string(),
            "0".into(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            format!("{:.6}", mean_treat),
            if ctrl_returns.is_empty() {
                String::new()
            } else {
                format!("{:.6}", mean_ctrl)
            },
            if excess.is_empty() {
                String::new()
            } else {
                format!("{:.6}", mean_excess)
            },
        ])
        .ok();
    }
    ws.flush().ok();

    let mut treat_sorted = treat_refs.clone();
    treat_sorted.sort_by(|a, b| {
        b.gross_return
            .partial_cmp(&a.gross_return)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let winners: Vec<_> = treat_sorted.iter().take(3).copied().collect();
    let losers: Vec<_> = treat_sorted.iter().rev().take(3).copied().collect();

    let md = out_dir.join("report.md");
    let mut report = String::from("# Event shock replay report\n\n");
    report.push_str("## Headline metrics\n\n");
    report.push_str(&format!("| Metric | Value |\n|--------|-------|\n"));
    report.push_str(&format!("| Events ingested | {} |\n", n_events));
    report.push_str(&format!(
        "| Trade rows (treatment + controls) | {} |\n",
        trade_vec.len()
    ));
    report.push_str(&format!("| Treatment trades | {} |\n", n_treatment));
    if let Some(s) = &last {
        report.push_str(&format!(
            "| Mean return (treatment) | {:.6} |\n",
            s.mean_return
        ));
        report.push_str(&format!(
            "| Median return (treatment) | {:.6} |\n",
            s.median_return
        ));
        report.push_str(&format!("| Hit rate (treatment) | {:.4} |\n", s.hit_rate));
    } else {
        report.push_str("| Mean / median / hit rate | (no treatment trades) |\n");
    }
    if !ctrl_returns.is_empty() {
        report.push_str(&format!(
            "| Mean control return (paired) | {:.6} |\n",
            mean_ctrl
        ));
        report.push_str(&format!(
            "| Mean excess (treatment − matched control) | {:.6} |\n",
            mean_excess
        ));
        if let Some(b) = last.as_ref().and_then(|x| x.bootstrap_mean_diff_vs_control) {
            report.push_str(&format!(
                "| Bootstrap mean diff (treatment − control, resampled pairs) | {:.6} |\n",
                b
            ));
        }
    } else {
        report.push_str("| Matched-control comparison | (no control rows) |\n");
    }

    report.push_str("\n## Top 3 treatment winners (by gross return)\n\n");
    report.push_str("| event_id | entry | exit | return |\n|----------|-------|------|--------|\n");
    for t in &winners {
        report.push_str(&format!(
            "| {} | {} | {} | {:.6} |\n",
            t.event_id.0, t.entry_session.0, t.exit_session.0, t.gross_return
        ));
    }
    if winners.is_empty() {
        report.push_str("| — | — | — | — |\n");
    }

    report.push_str("\n## Top 3 treatment losers (by gross return)\n\n");
    report.push_str("| event_id | entry | exit | return |\n|----------|-------|------|--------|\n");
    for t in &losers {
        report.push_str(&format!(
            "| {} | {} | {} | {:.6} |\n",
            t.event_id.0, t.entry_session.0, t.exit_session.0, t.gross_return
        ));
    }
    if losers.is_empty() {
        report.push_str("| — | — | — | — |\n");
    }

    report.push_str("\n## All trade rows\n\n");
    report.push_str(
        "| event | entry | exit | return | mdd | control_for |\n|---|---|---|---|---|---|\n",
    );
    for t in &trade_vec {
        let ctrl = t
            .matched_treatment
            .map(|m| format!("{}", m.0))
            .unwrap_or_else(|| "—".into());
        report.push_str(&format!(
            "| {} | {} | {} | {:.6} | {:.6} | {} |\n",
            t.event_id.0, t.entry_session.0, t.exit_session.0, t.gross_return, t.max_drawdown, ctrl
        ));
    }

    fs::write(&md, report).expect("write report");

    println!("Wrote {:?}, {:?}, {:?}", trades_csv, summary_csv, md);
}
