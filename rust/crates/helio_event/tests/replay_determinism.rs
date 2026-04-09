//! Golden-path: checkpoint mid-stream, restore, continue — combined outputs match one uninterrupted run.

use helio_event::{
    AvailabilityTagged, CausalEventStudyConfig, CausalEventStudyPipeline, EventStudyFoldScan,
    ForwardHorizonOutput, ReplayRecord, TreatmentEvent,
};
use helio_scan::{
    CheckpointKeyFn, Emit, FlushReason, FlushableScan, HashMapStore, Persisted, Runner, Scan,
    SessionDate, SnapshotStore, SnapshottingScan, VecEmitter,
};
use helio_time::AvailableAt;

fn synthetic_stream() -> Vec<ReplayRecord> {
    vec![
        ReplayRecord::Bar {
            session_day: 100,
            close: 100.0,
        },
        ReplayRecord::Treatment(AvailabilityTagged {
            value: TreatmentEvent {
                id: 1,
                day: 100,
                strength: 2.0,
                horizon_trading_days: 2,
            },
            observed_at: None,
            available_at: AvailableAt(0),
            effective_at: None,
            session_date: None,
        }),
        ReplayRecord::Bar {
            session_day: 101,
            close: 101.0,
        },
        ReplayRecord::Bar {
            session_day: 102,
            close: 104.0,
        },
        ReplayRecord::Treatment(AvailabilityTagged {
            value: TreatmentEvent {
                id: 2,
                day: 200,
                strength: 1.0,
                horizon_trading_days: 1,
            },
            observed_at: None,
            available_at: AvailableAt(500),
            effective_at: None,
            session_date: None,
        }),
        ReplayRecord::Bar {
            session_day: 103,
            close: 105.0,
        },
        ReplayRecord::Bar {
            session_day: 104,
            close: 106.0,
        },
    ]
}

fn collect_outcomes(
    pipe: &CausalEventStudyPipeline,
    records: &[ReplayRecord],
) -> Vec<ForwardHorizonOutput> {
    let mut st = pipe.init();
    let mut e = VecEmitter::new();
    for r in records {
        pipe.step(&mut st, r.clone(), &mut e);
    }
    pipe.flush(&mut st, FlushReason::EndOfInput, &mut e);
    e.into_inner()
}

#[test]
fn uninterrupted_matches_checkpoint_resume() {
    let cfg = CausalEventStudyConfig {
        decision_available: AvailableAt(100),
        overlap: helio_event::OverlapConfig { max_gap_days: 5 },
    };
    let pipe = CausalEventStudyPipeline::new(cfg);
    let records = synthetic_stream();
    let full = collect_outcomes(&pipe, &records);

    let checkpoint_after: usize = 3;
    let mut e_first = VecEmitter::new();
    let mut st = pipe.init();
    for r in records.iter().take(checkpoint_after) {
        pipe.step(&mut st, r.clone(), &mut e_first);
    }
    let snap = pipe.snapshot(&st);

    let mut e_rest = VecEmitter::new();
    let mut st2 = pipe.restore(snap);
    for r in records.iter().skip(checkpoint_after) {
        pipe.step(&mut st2, r.clone(), &mut e_rest);
    }
    pipe.flush(&mut st2, FlushReason::EndOfInput, &mut e_rest);

    let mut combined = e_first.into_inner();
    combined.extend(e_rest.into_inner());
    assert_eq!(full, combined);
}

#[test]
fn persisted_checkpoint_matches_manual_resume() {
    #[derive(Clone)]
    struct Key;
    impl CheckpointKeyFn<u64> for Key {
        type Key = &'static str;
        fn key_for_offset(&self, _offset: &u64) -> Self::Key {
            "cp"
        }
    }

    let cfg = CausalEventStudyConfig {
        decision_available: AvailableAt(1000),
        overlap: helio_event::OverlapConfig { max_gap_days: 2 },
    };
    let records = synthetic_stream();
    let pipe_plain = CausalEventStudyPipeline::new(cfg);
    let full = collect_outcomes(&pipe_plain, &records);

    let inner = CausalEventStudyPipeline::new(cfg);
    let persisted = Persisted::new(inner, HashMapStore::default(), Key);
    let mut r = Runner::new(persisted);
    let mut e = VecEmitter::new();
    for (i, rec) in records.iter().enumerate() {
        r.step(rec.clone(), &mut e);
        if i + 1 == 3 {
            r.flush(FlushReason::Checkpoint(7u64), &mut e);
        }
    }
    r.flush(FlushReason::EndOfInput, &mut e);
    assert_eq!(full, e.into_inner());

    let cp = r.machine.store.borrow_mut().get(&"cp").unwrap().unwrap();
    assert_eq!(cp.offset, 7);

    let mut e_first = VecEmitter::new();
    let mut st = pipe_plain.init();
    for rec in records.iter().take(3) {
        pipe_plain.step(&mut st, rec.clone(), &mut e_first);
    }
    let manual_snap = pipe_plain.snapshot(&st);
    assert_eq!(manual_snap, cp.snapshot);

    let mut r2 = Runner::new(Persisted::new(
        CausalEventStudyPipeline::new(cfg),
        HashMapStore::default(),
        Key,
    ));
    r2.state = r2.machine.restore(cp.snapshot.clone());
    let mut e2 = VecEmitter::new();
    for rec in records.iter().skip(3) {
        r2.step(rec.clone(), &mut e2);
    }
    r2.flush(FlushReason::EndOfInput, &mut e2);

    let mut combined = e_first.into_inner();
    combined.extend(e2.into_inner());
    assert_eq!(full, combined);
}

#[test]
fn session_close_flushes_mid_horizon_no_complete_fold_update() {
    let cfg = CausalEventStudyConfig {
        decision_available: AvailableAt(1000),
        overlap: helio_event::OverlapConfig { max_gap_days: 5 },
    };
    let pipe = CausalEventStudyPipeline::new(cfg);
    let fold = EventStudyFoldScan;
    let records = vec![
        ReplayRecord::Bar {
            session_day: 1,
            close: 100.0,
        },
        ReplayRecord::Treatment(AvailabilityTagged {
            value: TreatmentEvent {
                id: 1,
                day: 1,
                strength: 1.0,
                horizon_trading_days: 10,
            },
            observed_at: None,
            available_at: AvailableAt(0),
            effective_at: None,
            session_date: None,
        }),
        ReplayRecord::Bar {
            session_day: 2,
            close: 102.0,
        },
    ];
    let mut st = pipe.init();
    let mut st_f = fold.init();
    let mut e_out = VecEmitter::new();
    let mut e_sum = VecEmitter::new();
    for r in &records {
        let mut b = VecEmitter::new();
        pipe.step(&mut st, r.clone(), &mut b);
        for o in b.into_inner() {
            fold.step(&mut st_f, o.clone(), &mut e_sum);
            e_out.emit(o);
        }
    }
    let mut b = VecEmitter::new();
    pipe.flush(&mut st, FlushReason::SessionClose(SessionDate(2)), &mut b);
    for o in b.into_inner() {
        fold.step(&mut st_f, o.clone(), &mut e_sum);
        e_out.emit(o);
    }
    let outs = e_out.into_inner();
    assert!(outs
        .iter()
        .any(|o| matches!(o, ForwardHorizonOutput::Incomplete(_))));
    assert!(e_sum.into_inner().is_empty());
}

#[test]
fn fold_tracks_complete_outcomes_only() {
    let cfg = CausalEventStudyConfig {
        decision_available: AvailableAt(100),
        overlap: helio_event::OverlapConfig { max_gap_days: 5 },
    };
    let pipe = CausalEventStudyPipeline::new(cfg);
    let fold = EventStudyFoldScan;
    let records = synthetic_stream();
    let mut st_p = pipe.init();
    let mut st_f = fold.init();
    let mut e_sum = VecEmitter::new();
    for r in &records {
        let mut b = VecEmitter::new();
        pipe.step(&mut st_p, r.clone(), &mut b);
        for o in b.into_inner() {
            fold.step(&mut st_f, o, &mut e_sum);
        }
    }
    let mut b = VecEmitter::new();
    pipe.flush(&mut st_p, FlushReason::EndOfInput, &mut b);
    for o in b.into_inner() {
        fold.step(&mut st_f, o, &mut e_sum);
    }
    let sums = e_sum.into_inner();
    let n_complete = collect_outcomes(&pipe, &records)
        .into_iter()
        .filter(|o| matches!(o, ForwardHorizonOutput::Complete(_)))
        .count();
    assert_eq!(sums.len(), n_complete);
    assert!(!sums.is_empty());
    assert_eq!(sums.last().unwrap().count, n_complete as u64);
}
