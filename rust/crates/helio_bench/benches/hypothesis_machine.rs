//! Keyed lifecycle hot paths and bounded timer-frontier pressure.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_hypothesis::{
    CausalEvidence, HypothesisConfig, HypothesisInput, HypothesisModel, HypothesisTransition,
    KeyedHypothesisMachine, TimerId,
};
use helio_scan::{DiscardEmitter, Scan};
use helio_time::{AvailableAt, EffectiveAt};

const UPDATES: u64 = 65_536;
const KEYS: u64 = 1_024;
const TIMER_KEYS: u64 = 4_096;

#[derive(Debug, Clone, Copy)]
struct CounterModel;

impl HypothesisModel<u64> for CounterModel {
    type Evidence = u64;
    type State = u64;
    type Output = ();
    type Error = std::convert::Infallible;

    fn open(
        &self,
        _key: &u64,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        Ok(HypothesisTransition::new(evidence.payload))
    }

    fn update(
        &self,
        _key: &u64,
        state: &Self::State,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        Ok(HypothesisTransition::new(
            state.wrapping_add(evidence.payload),
        ))
    }

    fn on_timer(
        &self,
        _key: &u64,
        state: &Self::State,
        _timer_id: TimerId,
        _at: AvailableAt,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        Ok(HypothesisTransition::new(*state))
    }

    fn validate(&self, _key: &u64, _state: &Self::State) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct DeadlineModel {
    deadline: AvailableAt,
}

impl HypothesisModel<u64> for DeadlineModel {
    type Evidence = ();
    type State = ();
    type Output = ();
    type Error = std::convert::Infallible;

    fn open(
        &self,
        _key: &u64,
        _evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        Ok(HypothesisTransition::new(()).schedule(TimerId(0), self.deadline))
    }

    fn update(
        &self,
        _key: &u64,
        _state: &Self::State,
        _evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        Ok(HypothesisTransition::new(()))
    }

    fn on_timer(
        &self,
        _key: &u64,
        _state: &Self::State,
        _timer_id: TimerId,
        _at: AvailableAt,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        Ok(HypothesisTransition::new(()).complete())
    }

    fn validate(&self, _key: &u64, _state: &Self::State) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn counter_machine(max_active: usize) -> KeyedHypothesisMachine<u64, CounterModel, ()> {
    KeyedHypothesisMachine::try_new(
        CounterModel,
        HypothesisConfig::try_new(max_active, 0, 0, 0, 1).unwrap(),
    )
    .unwrap()
}

fn evidence(sequence: u64, available_at: i64, payload: u64) -> CausalEvidence<u64> {
    CausalEvidence::new(
        sequence,
        EffectiveAt(available_at),
        AvailableAt(available_at),
        payload,
    )
}

fn single_key_updates(c: &mut Criterion) {
    let machine = counter_machine(1);
    let mut group = c.benchmark_group("hypothesis_single_key");
    group.throughput(Throughput::Elements(UPDATES));
    group.bench_function("update_65536", |b| {
        b.iter(|| {
            let mut state = machine.init();
            let mut emit = DiscardEmitter;
            machine.step(
                &mut state,
                HypothesisInput::Open {
                    key: 0,
                    evidence: evidence(0, 0, 0),
                },
                &mut emit,
            );
            for sequence in 1..=UPDATES {
                machine.step(
                    &mut state,
                    HypothesisInput::Evidence {
                        key: 0,
                        evidence: evidence(sequence, sequence as i64, black_box(1)),
                    },
                    &mut emit,
                );
            }
            black_box(state.get(&0).map(|record| record.model_state))
        });
    });
    group.finish();
}

fn round_robin_updates(c: &mut Criterion) {
    let machine = counter_machine(KEYS as usize);
    let mut group = c.benchmark_group("hypothesis_round_robin");
    group.throughput(Throughput::Elements(UPDATES));
    group.bench_function("1024_keys_65536_updates", |b| {
        b.iter(|| {
            let mut state = machine.init();
            let mut emit = DiscardEmitter;
            for key in 0..KEYS {
                machine.step(
                    &mut state,
                    HypothesisInput::Open {
                        key,
                        evidence: evidence(0, 0, 0),
                    },
                    &mut emit,
                );
            }
            for index in 0..UPDATES {
                let key = index % KEYS;
                let sequence = index / KEYS + 1;
                machine.step(
                    &mut state,
                    HypothesisInput::Evidence {
                        key,
                        evidence: evidence(sequence, index as i64 + 1, black_box(1)),
                    },
                    &mut emit,
                );
            }
            black_box(state.active_count())
        });
    });
    group.finish();
}

fn timer_frontier(c: &mut Criterion) {
    let machine = KeyedHypothesisMachine::<u64, _, ()>::try_new(
        DeadlineModel {
            deadline: AvailableAt(10),
        },
        HypothesisConfig::try_new(TIMER_KEYS as usize, 0, 1, 1, TIMER_KEYS as usize).unwrap(),
    )
    .unwrap();
    let mut initial = machine.init();
    let mut emit = DiscardEmitter;
    for key in 0..TIMER_KEYS {
        machine.step(
            &mut initial,
            HypothesisInput::Open {
                key,
                evidence: CausalEvidence::new(0, EffectiveAt(0), AvailableAt(0), ()),
            },
            &mut emit,
        );
    }

    let idle_machine = KeyedHypothesisMachine::<u64, _, ()>::try_new(
        DeadlineModel {
            deadline: AvailableAt(i64::MAX),
        },
        HypothesisConfig::try_new(TIMER_KEYS as usize, 0, 1, 1, TIMER_KEYS as usize).unwrap(),
    )
    .unwrap();
    let mut idle_state = idle_machine.init();
    for key in 0..TIMER_KEYS {
        idle_machine.step(
            &mut idle_state,
            HypothesisInput::Open {
                key,
                evidence: CausalEvidence::new(0, EffectiveAt(0), AvailableAt(0), ()),
            },
            &mut emit,
        );
    }
    let mut next_frontier = 0_i64;
    let mut no_due_group = c.benchmark_group("hypothesis_frontier_no_due_timers");
    no_due_group.throughput(Throughput::Elements(1));
    no_due_group.bench_function("advance_with_4096_future_timers", |b| {
        b.iter(|| {
            next_frontier += 1;
            idle_machine.step(
                &mut idle_state,
                HypothesisInput::Advance {
                    to: AvailableAt(next_frontier),
                },
                &mut DiscardEmitter,
            );
            black_box(idle_state.frontier())
        });
    });
    no_due_group.finish();

    let mut group = c.benchmark_group("hypothesis_timers");
    group.throughput(Throughput::Elements(TIMER_KEYS));
    group.bench_function("fire_and_complete_4096", |b| {
        b.iter_batched(
            || initial.clone(),
            |mut state| {
                machine.step(
                    &mut state,
                    HypothesisInput::Advance {
                        to: AvailableAt(10),
                    },
                    &mut DiscardEmitter,
                );
                black_box(state.active_count())
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    single_key_updates,
    round_robin_updates,
    timer_frontier
);
criterion_main!(benches);
