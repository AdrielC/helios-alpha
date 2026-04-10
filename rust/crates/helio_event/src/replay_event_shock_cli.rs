//! CLI and JSON-config entry for the event-shock vertical demo.
//!
//! Run: `cargo run -p helio_event --bin replay_event_shock -- replay-event-shock --events ...`
//! or omit the subcommand and pass flags only.

use crate::{
    bars_from_file, build_vertical_replay, candidate_entries_from_bars,
    collect_vertical_trades_batch, collect_vertical_trades_incremental,
    collect_vertical_trades_with_checkpoint_resume, event_scope_label, summarize_lead_times,
    shocks_from_file, EventShockControlConfig, EventShockFilterConfig, EventShockMetricsFoldScan,
    EventShockReplayConfig, EventShockStrategyPreset, EventShockVerticalScan, ExecutionEntryTiming,
    LabeledTradeResult, ScopeFilter, TradeResult,
};
use helio_scan::{Scan, VecEmitter};
use helio_time::{AvailableAt, SimpleWeekdayCalendar};
use std::fs;
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "Usage: replay_event_shock [--events <file> --bars <file> | --config <file.json>] [options]\n\
         Options:\n\
          --events <file>          events CSV / JSONL (required unless --config)\n\
          --bars <file>            daily bars CSV (required unless --config)\n\
          --config <file>          JSON config (see EventShockReplayConfig)\n\
          --out <dir>              output directory (default: event_shock_out)\n\
          --as-of <epoch_sec>      availability gate (optional)\n\
          --events-format <fmt>    csv | jsonl | compact | compact-region\n\
          --strategy <name>        xlu-spy-3 | xlu-spy-5 | defense-spy-mid (default: xlu-spy-3)\n\
          --execution-entry <m>    next_session_open | entry_session_open (default: next_session_open)\n\
          --min-lead-secs <i64>    lead-time band lower (default: 0)\n\
          --max-lead-secs <i64>    lead-time band upper (default: 9223372036854775807)\n\
          --control-seed <u64>     matched-control seed (default: 42)\n\
          --skip-replay-verify     do not assert batch==incremental==checkpoint"
    );
    eprintln!("  Full CSV: id[,kind],available_at,impact_start,impact_end,severity,confidence,scope[,...]");
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
        "xlu-spy-3" | "xlu_spy_3" => EventShockStrategyPreset::XluSpyPairThreeSession,
        "defense-spy-mid" | "ita-spy-mid" => EventShockStrategyPreset::DefenseSpyPairMidWindow,
        _ => EventShockStrategyPreset::XluSpyPairFiveSession,
    }
}

fn canonical_strategy_name(s: &str) -> String {
    parse_strategy(s).cli_name().to_string()
}

fn parse_execution_entry(s: &str) -> Result<ExecutionEntryTiming, String> {
    match s.to_ascii_lowercase().replace('-', "_").as_str() {
        "next_session_open" => Ok(ExecutionEntryTiming::NextSessionOpen),
        "entry_session_open" => Ok(ExecutionEntryTiming::EntrySessionOpen),
        other => Err(format!("unknown --execution-entry: {other}")),
    }
}

#[derive(Debug)]
struct CliReplay {
    events_path: PathBuf,
    bars_path: PathBuf,
    out_dir: PathBuf,
    as_of: Option<i64>,
    events_format: String,
    strategy_name: String,
    min_lead_secs: i64,
    max_lead_secs: i64,
    control_seed: u64,
    skip_verify: bool,
    execution_entry: ExecutionEntryTiming,
}

fn parse_cli_args(mut args: Vec<String>) -> Result<CliReplay, String> {
    let mut events_path: Option<PathBuf> = None;
    let mut bars_path: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut out_dir = PathBuf::from("event_shock_out");
    let mut as_of: Option<i64> = None;
    let mut events_format = "csv".to_string();
    let mut strategy_name = "xlu-spy-3".to_string();
    let mut min_lead_secs: i64 = 0;
    let mut max_lead_secs: i64 = i64::MAX;
    let mut control_seed: u64 = 42;
    let mut skip_verify = false;
    let mut execution_entry = ExecutionEntryTiming::NextSessionOpen;

    while let Some(a) = args.first().cloned() {
        args.remove(0);
        match a.as_str() {
            "--events" => events_path = Some(PathBuf::from(args.remove(0))),
            "--bars" => bars_path = Some(PathBuf::from(args.remove(0))),
            "--config" => config_path = Some(PathBuf::from(args.remove(0))),
            "--out" => out_dir = PathBuf::from(args.remove(0)),
            "--as-of" => {
                as_of = Some(
                    args.remove(0)
                        .parse()
                        .map_err(|_| "as-of must be i64".to_string())?,
                );
            }
            "--events-format" => events_format = args.remove(0),
            "--strategy" => strategy_name = args.remove(0),
            "--min-lead-secs" => {
                min_lead_secs = args
                    .remove(0)
                    .parse()
                    .map_err(|_| "min-lead-secs must be i64".to_string())?;
            }
            "--max-lead-secs" => {
                max_lead_secs = args
                    .remove(0)
                    .parse()
                    .map_err(|_| "max-lead-secs must be i64".to_string())?;
            }
            "--control-seed" => {
                control_seed = args
                    .remove(0)
                    .parse()
                    .map_err(|_| "control-seed must be u64".to_string())?;
            }
            "--execution-entry" => {
                execution_entry = parse_execution_entry(&args.remove(0))?;
            }
            "--skip-replay-verify" => skip_verify = true,
            "--help" | "-h" => usage(),
            other => return Err(format!("unknown arg: {other}")),
        }
    }

    if let Some(p) = config_path {
        let cfg = EventShockReplayConfig::from_json_path(&p)?;
        if cfg.events_path.is_empty() || cfg.bars_path.is_empty() {
            return Err("config must set events_path and bars_path".into());
        }
        return Ok(CliReplay {
            events_path: PathBuf::from(cfg.events_path),
            bars_path: PathBuf::from(cfg.bars_path),
            out_dir: if cfg.out_dir.is_empty() {
                PathBuf::from("event_shock_out")
            } else {
                PathBuf::from(cfg.out_dir)
            },
            as_of: cfg.as_of_epoch_sec,
            events_format: cfg.events_format,
            strategy_name: canonical_strategy_name(&cfg.strategy),
            min_lead_secs: cfg.min_lead_secs,
            max_lead_secs: cfg.max_lead_secs,
            control_seed: cfg.control_seed,
            skip_verify: cfg.skip_replay_verify,
            execution_entry: parse_execution_entry(&cfg.execution_entry)?,
        });
    }

    let events_path = events_path.ok_or_else(|| "--events required (or use --config)".to_string())?;
    let bars_path = bars_path.ok_or_else(|| "--bars required (or use --config)".to_string())?;
    Ok(CliReplay {
        events_path,
        bars_path,
        out_dir,
        as_of,
        events_format,
        strategy_name,
        min_lead_secs,
        max_lead_secs,
        control_seed,
        skip_verify,
        execution_entry,
    })
}

/// Run the full demo from argv tokens (without program name). Used by the `replay_event_shock` binary.
pub fn replay_event_shock_run_from_args(args: Vec<String>) {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
    }
    match parse_cli_args(args).and_then(|c| run_event_shock_replay_cli(c)) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// JSON-configured run (library API).
pub fn replay_event_shock_run_from_config(cfg: EventShockReplayConfig) -> Result<(), String> {
    if cfg.events_path.is_empty() || cfg.bars_path.is_empty() {
        return Err("events_path and bars_path must be non-empty".into());
    }
    let cli = CliReplay {
        events_path: PathBuf::from(cfg.events_path),
        bars_path: PathBuf::from(cfg.bars_path),
        out_dir: if cfg.out_dir.is_empty() {
            PathBuf::from("event_shock_out")
        } else {
            PathBuf::from(cfg.out_dir)
        },
        as_of: cfg.as_of_epoch_sec,
        events_format: cfg.events_format,
        strategy_name: cfg.strategy,
        min_lead_secs: cfg.min_lead_secs,
        max_lead_secs: cfg.max_lead_secs,
        control_seed: cfg.control_seed,
        skip_verify: cfg.skip_replay_verify,
        execution_entry: parse_execution_entry(&cfg.execution_entry)?,
    };
    run_event_shock_replay_cli(cli)
}

fn run_event_shock_replay_cli(cli: CliReplay) -> Result<(), String> {
    let shocks =
        shocks_from_file(&cli.events_path, &cli.events_format).map_err(|e| e.to_string())?;
    let n_events = shocks.len();
    let lead_report = summarize_lead_times(&shocks, cli.min_lead_secs, cli.max_lead_secs);

    let bars = bars_from_file(&cli.bars_path).map_err(|e| e.to_string())?;
    let candidates = candidate_entries_from_bars(&bars);
    let replay = build_vertical_replay(shocks, bars);

    let preset = parse_strategy(&cli.strategy_name);
    let strategy_report = preset.cli_name().to_string();
    let filter = EventShockFilterConfig {
        min_severity: 0.0,
        min_confidence: 0.0,
        min_lead_secs: cli.min_lead_secs,
        max_lead_secs: cli.max_lead_secs,
        scope: ScopeFilter::Any,
    };

    let cal = SimpleWeekdayCalendar;
    let vertical = EventShockVerticalScan::new(
        cli.as_of.map(AvailableAt),
        filter,
        cal,
        preset.exit_policy(),
        preset.treatment_exposure(),
        EventShockControlConfig {
            seed: cli.control_seed,
            controls_per_treatment: 1,
            strategy_name: strategy_report.clone(),
            horizon_sessions: preset.control_horizon_sessions(),
            exposure: preset.control_exposure_clone(),
            vol_epsilon: None,
        },
        candidates,
        cli.execution_entry,
        strategy_report.clone(),
    );

    let batch = collect_vertical_trades_batch(&vertical, &replay);
    let incremental = collect_vertical_trades_incremental(&vertical, &replay);
    let checkpointed =
        collect_vertical_trades_with_checkpoint_resume(&vertical, &replay, replay.len() / 2);

    if !cli.skip_verify && (batch != incremental || batch != checkpointed) {
        return Err("replay verification failed: batch/incremental/checkpoint trades differ".into());
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

    fs::create_dir_all(&cli.out_dir).map_err(|e| e.to_string())?;
    let trades_csv = cli.out_dir.join("trades.csv");
    let mut w = csv::Writer::from_path(&trades_csv).map_err(|e| e.to_string())?;
    w.write_record([
        "event_id",
        "entry_session",
        "exit_session",
        "strategy",
        "gross_return",
        "max_drawdown",
        "holding_sessions",
        "scope",
        "matched_treatment",
    ])
    .map_err(|e| e.to_string())?;
    for t in &trade_vec {
        w.write_record([
            t.event_id.0.to_string(),
            t.entry_session.0.to_string(),
            t.exit_session.0.to_string(),
            t.strategy_name.clone(),
            format!("{:.6}", t.gross_return),
            format!("{:.6}", t.max_drawdown),
            t.holding_period_sessions.to_string(),
            event_scope_label(&t.scope),
            t.matched_treatment
                .map(|m| m.0.to_string())
                .unwrap_or_default(),
        ])
        .map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())?;

    let lead_csv = cli.out_dir.join("lead_time.csv");
    let mut wl = csv::Writer::from_path(&lead_csv).map_err(|e| e.to_string())?;
    wl.write_record([
        "n_events",
        "min_lead_secs",
        "max_lead_secs_observed",
        "band_min_secs",
        "band_max_secs",
        "n_tradable_under_band",
    ])
    .map_err(|e| e.to_string())?;
    wl.write_record([
        lead_report.n_events.to_string(),
        lead_report.min_lead_secs.to_string(),
        lead_report.max_lead_secs.to_string(),
        lead_report.band_min_secs.to_string(),
        lead_report.band_max_secs.to_string(),
        lead_report.n_tradable_under_band.to_string(),
    ])
    .map_err(|e| e.to_string())?;
    wl.flush().map_err(|e| e.to_string())?;

    let summary_csv = cli.out_dir.join("summary.csv");
    let mut ws = csv::Writer::from_path(&summary_csv).map_err(|e| e.to_string())?;
    ws.write_record([
        "n_events_ingested",
        "n_trade_rows",
        "n_treatment_trades",
        "lead_band_min_secs",
        "lead_band_max_secs",
        "n_events_tradable_lead_band",
        "strategy",
        "execution_entry",
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
    .map_err(|e| e.to_string())?;

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

    let exec_entry_str = match cli.execution_entry {
        ExecutionEntryTiming::NextSessionOpen => "next_session_open",
        ExecutionEntryTiming::EntrySessionOpen => "entry_session_open",
    };

    if let Some(s) = &last {
        ws.write_record([
            n_events.to_string(),
            trade_vec.len().to_string(),
            n_treatment.to_string(),
            cli.min_lead_secs.to_string(),
            cli.max_lead_secs.to_string(),
            lead_report.n_tradable_under_band.to_string(),
            strategy_report.clone(),
            exec_entry_str.into(),
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
        .map_err(|e| e.to_string())?;
    } else {
        ws.write_record([
            n_events.to_string(),
            trade_vec.len().to_string(),
            n_treatment.to_string(),
            cli.min_lead_secs.to_string(),
            cli.max_lead_secs.to_string(),
            lead_report.n_tradable_under_band.to_string(),
            strategy_report.clone(),
            exec_entry_str.into(),
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
        .map_err(|e| e.to_string())?;
    }
    ws.flush().map_err(|e| e.to_string())?;

    let mut treat_sorted = treat_refs.clone();
    treat_sorted.sort_by(|a, b| {
        b.gross_return
            .partial_cmp(&a.gross_return)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let winners: Vec<_> = treat_sorted.iter().take(5).copied().collect();
    let losers: Vec<_> = treat_sorted.iter().rev().take(5).copied().collect();

    let md = cli.out_dir.join("report.md");
    let mut report = String::from("# Event shock replay report\n\n");
    report.push_str(&format!(
        "Strategy: `{}` · execution entry: `{}` · events format: `{}` · replay verify: {}\n\n",
        strategy_report,
        exec_entry_str,
        cli.events_format,
        if cli.skip_verify {
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

    report.push_str("\n## Top 5 treatment winners (by gross return)\n\n");
    report.push_str(
        "| event_id | entry | exit | strategy | scope | return |\n|----------|-------|------|----------|-------|--------|\n",
    );
    for t in &winners {
        report.push_str(&format!(
            "| {} | {} | {} | `{}` | {} | {:.6} |\n",
            t.event_id.0,
            t.entry_session.0,
            t.exit_session.0,
            t.strategy_name,
            event_scope_label(&t.scope),
            t.gross_return
        ));
    }
    if winners.is_empty() {
        report.push_str("| — | — | — | — |\n");
    }

    report.push_str("\n## Top 5 treatment losers (by gross return)\n\n");
    report.push_str(
        "| event_id | entry | exit | strategy | scope | return |\n|----------|-------|------|----------|-------|--------|\n",
    );
    for t in &losers {
        report.push_str(&format!(
            "| {} | {} | {} | `{}` | {} | {:.6} |\n",
            t.event_id.0,
            t.entry_session.0,
            t.exit_session.0,
            t.strategy_name,
            event_scope_label(&t.scope),
            t.gross_return
        ));
    }
    if losers.is_empty() {
        report.push_str("| — | — | — | — |\n");
    }

    report.push_str("\n## All trade rows\n\n");
    report.push_str(
        "| event | entry | exit | strategy | scope | return | mdd | control_for |\n|---|---|---|---|---|---|---|---|\n",
    );
    for t in &trade_vec {
        let ctrl = t
            .matched_treatment
            .map(|m| format!("{}", m.0))
            .unwrap_or_else(|| "—".into());
        report.push_str(&format!(
            "| {} | {} | {} | `{}` | {} | {:.6} | {:.6} | {} |\n",
            t.event_id.0,
            t.entry_session.0,
            t.exit_session.0,
            t.strategy_name,
            event_scope_label(&t.scope),
            t.gross_return,
            t.max_drawdown,
            ctrl
        ));
    }

    report.push_str("\n## Benchmark notes\n\n");
    report.push_str(
        "Throughput and checkpoint baselines for this stack live in `docs/EVENT_SHOCK_BENCHMARKS.md` \
         (run `cargo bench -p helio_bench`). Treat documented medians as order-of-magnitude guardrails; \
         investigate **>2×** on the same workload, strong signal **>3×**.\n\n",
    );

    fs::write(&md, report).map_err(|e| e.to_string())?;

    println!(
        "Wrote {:?}, {:?}, {:?}, {:?}",
        trades_csv, lead_csv, summary_csv, md
    );
    Ok(())
}
