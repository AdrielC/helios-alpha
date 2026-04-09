//! `cargo run -p helio_event --bin replay_event_shock -- --events FILE --bars FILE ...`
//!
//! Loads normalized events and daily bars, runs the event-shock vertical, writes CSV + markdown.
//! Verifies incremental / batch / checkpoint-resume paths produce identical trade lists.

use helio_event::*;
use helio_scan::{Scan, VecEmitter};
use helio_time::{AvailableAt, SimpleWeekdayCalendar};
use std::fs;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "Usage: replay_event_shock --events <file> --bars <file> [options]\n\
         Options:\n\
          --out <dir>              output directory (default: event_shock_out)\n\
         --as-of <epoch_sec>      availability gate (optional)\n\
         --events-format <fmt>    csv | jsonl | compact | compact-region\n\
         --strategy <name>        xlu-spy-5 | defense-spy-mid (default: xlu-spy-5)\n\
         --min-lead-secs <i64>    lead-time band lower (default: 0)\n\
         --max-lead-secs <i64>    lead-time band upper (default: 9223372036854775807)\n\
         --control-seed <u64>     matched-control RNG seed (default: 42)\n\
         --skip-replay-verify     do not assert batch==incremental==checkpoint"
    );
    eprintln!("  compact        = id,available_at,impact_start,impact_end,severity,confidence (global scope)");
    eprintln!("  compact-region = same + optional region_code → EventScope::Region");
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

fn parse_strategy(s: &str) -> EventShockStrategyPreset {
    match s.to_ascii_lowercase().as_str() {
        "defense-spy-mid" | "ita-spy-mid" => EventShockStrategyPreset::DefenseSpyPairMidWindow,
        _ => EventShockStrategyPreset::XluSpyPairFiveSession,
    }
}

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut events_path: Option<PathBuf> = None;
    let mut bars_path: Option<PathBuf> = None;
    let mut out_dir = PathBuf::from("event_shock_out");
    let mut as_of: Option<i64> = None;
    let mut events_format = "csv".to_string();
    let mut strategy_name = "xlu-spy-5".to_string();
    let mut min_lead_secs: i64 = 0;
    let mut max_lead_secs: i64 = i64::MAX;
    let mut control_seed: u64 = 42;
    let mut skip_verify = false;

    while let Some(a) = args.first().cloned() {
        args.remove(0);
        match a.as_str() {
            "--events" => events_path = Some(PathBuf::from(args.remove(0))),
            "--bars" => bars_path = Some(PathBuf::from(args.remove(0))),
            "--out" => out_dir = PathBuf::from(args.remove(0)),
            "--as-of" => as_of = Some(args.remove(0).parse().expect("as-of int")),
            "--events-format" => events_format = args.remove(0),
            "--strategy" => strategy_name = args.remove(0),
            "--min-lead-secs" => min_lead_secs = args.remove(0).parse().expect("min-lead-secs"),
            "--max-lead-secs" => max_lead_secs = args.remove(0).parse().expect("max-lead-secs"),
            "--control-seed" => control_seed = args.remove(0).parse().expect("control-seed"),
            "--skip-replay-verify" => skip_verify = true,
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
        "compact" => load_compact_event_shocks_csv(&events_raw).expect("parse compact events"),
        "compact-region" => {
            load_compact_region_event_shocks_csv(&events_raw).expect("parse compact-region events")
        }
        _ => load_event_shocks_csv(&events_raw).expect("parse events"),
    };
    let n_events = shocks.len();
    let lead_report = summarize_lead_times(&shocks, min_lead_secs, max_lead_secs);

    let bars_raw = fs::read_to_string(&bars_path).expect("read bars");
    let bars = load_daily_bars_csv(&bars_raw).expect("parse bars");
    let candidates = candidate_entries_from_bars(&bars);
    let replay = build_vertical_replay(shocks, bars);

    let preset = parse_strategy(&strategy_name);
    let filter = EventShockFilterConfig {
        min_severity: 0.0,
        min_confidence: 0.0,
        min_lead_secs,
        max_lead_secs,
        scope: ScopeFilter::Any,
    };

    let cal = SimpleWeekdayCalendar;
    let vertical = EventShockVerticalScan::new(
        as_of.map(AvailableAt),
        filter,
        cal,
        preset.exit_policy(),
        preset.treatment_exposure(),
        EventShockControlConfig {
            seed: control_seed,
            controls_per_treatment: 1,
            horizon_sessions: preset.control_horizon_sessions(),
            exposure: preset.control_exposure_clone(),
            vol_epsilon: None,
        },
        candidates,
    );

    let batch = collect_vertical_trades_batch(&vertical, &replay);
    let incremental = collect_vertical_trades_incremental(&vertical, &replay);
    let checkpointed =
        collect_vertical_trades_with_checkpoint_resume(&vertical, &replay, replay.len() / 2);

    if !skip_verify && (batch != incremental || batch != checkpointed) {
        eprintln!("replay verification failed: batch/incremental/checkpoint trades differ");
        std::process::exit(1);
    }
    let trade_vec = batch;

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

    let lead_csv = out_dir.join("lead_time.csv");
    let mut wl = csv::Writer::from_path(&lead_csv).expect("lead csv");
    wl.write_record([
        "n_events",
        "min_lead_secs",
        "max_lead_secs_observed",
        "band_min_secs",
        "band_max_secs",
        "n_tradable_under_band",
    ])
    .ok();
    wl.write_record([
        lead_report.n_events.to_string(),
        lead_report.min_lead_secs.to_string(),
        lead_report.max_lead_secs.to_string(),
        lead_report.band_min_secs.to_string(),
        lead_report.band_max_secs.to_string(),
        lead_report.n_tradable_under_band.to_string(),
    ])
    .ok();
    wl.flush().ok();

    let summary_csv = out_dir.join("summary.csv");
    let mut ws = csv::Writer::from_path(&summary_csv).expect("summary csv");
    ws.write_record([
        "n_events_ingested",
        "n_trade_rows",
        "n_treatment_trades",
        "lead_band_min_secs",
        "lead_band_max_secs",
        "n_events_tradable_lead_band",
        "strategy",
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
            min_lead_secs.to_string(),
            max_lead_secs.to_string(),
            lead_report.n_tradable_under_band.to_string(),
            strategy_name.clone(),
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
            min_lead_secs.to_string(),
            max_lead_secs.to_string(),
            lead_report.n_tradable_under_band.to_string(),
            strategy_name.clone(),
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
    report.push_str(&format!(
        "Strategy: `{}` · events format: `{}` · replay verify: {}\n\n",
        strategy_name,
        events_format,
        if skip_verify {
            "skipped"
        } else {
            "batch == incremental == checkpoint OK"
        }
    ));

    report.push_str("## Lead time (impact_start − available_at)\n\n");
    report.push_str(&format!(
        "| | |\n|--|--|\n| Events | {} |\n| Min lead (sec) | {} |\n| Max lead (sec) | {} |\n| Configured band \\[min, max\\] | [{}, {}] |\n| Events in band (tradable lead) | {} |\n",
        lead_report.n_events,
        lead_report.min_lead_secs,
        lead_report.max_lead_secs,
        lead_report.band_min_secs,
        lead_report.band_max_secs,
        lead_report.n_tradable_under_band
    ));
    report.push_str(
        "\n*(Pipeline filter uses the same band for `min_lead_secs` / `max_lead_secs`.)*\n\n",
    );

    report.push_str("## Headline metrics\n\n");
    report.push_str("| Metric | Value |\n|--------|-------|\n");
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

    println!(
        "Wrote {:?}, {:?}, {:?}, {:?}",
        trades_csv, lead_csv, summary_csv, md
    );
}
