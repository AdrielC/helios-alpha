//! Summary statistics over [`TradeResult`](crate::TradeResult).

use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{EventId, TradeResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockMetricsSummary {
    pub count: u64,
    pub mean_return: f64,
    pub median_return: f64,
    pub hit_rate: f64,
    pub std_dev: f64,
    /// Simple bootstrap mean difference vs control (treatment - control), when controls present.
    pub bootstrap_mean_diff_vs_control: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventShockMetricsFoldState {
    pub treatment_by_event: HashMap<u64, f64>,
    pub treatment_returns: Vec<f64>,
    pub pairs: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockMetricsFoldSnapshot {
    pub treatment_by_event: Vec<(u64, f64)>,
    pub treatment_returns: Vec<f64>,
    pub pairs: Vec<(f64, f64)>,
}

/// Tag treatment vs control for bootstrap pairing (optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LabeledTradeResult {
    Treatment(TradeResult),
    Control {
        matched_event_id: EventId,
        trade: TradeResult,
    },
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn std_sample(xs: &[f64], m: f64) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let v = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
    v.sqrt()
}

fn median(mut xs: Vec<f64>) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

fn hit_rate(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    xs.iter().filter(|x| **x > 0.0).count() as f64 / xs.len() as f64
}

/// Deterministic bootstrap: resample matched (treatment − control) pairs with replacement.
fn bootstrap_paired_diff_mean(pairs: &[(f64, f64)], seed: u64, iterations: u32) -> f64 {
    use std::num::Wrapping;
    if pairs.is_empty() || iterations == 0 {
        return f64::NAN;
    }
    let mut s = Wrapping(seed);
    let mut sum = 0.0f64;
    let n = pairs.len();
    for _ in 0..iterations {
        s = s * Wrapping(6364136223846793005) + Wrapping(1);
        let i = (s.0 as usize) % n;
        sum += pairs[i].0 - pairs[i].1;
    }
    sum / iterations as f64
}

#[derive(Debug, Clone, Copy)]
pub struct EventShockMetricsFoldScan {
    pub bootstrap_iterations: u32,
    pub bootstrap_seed: u64,
}

impl Default for EventShockMetricsFoldScan {
    fn default() -> Self {
        Self {
            bootstrap_iterations: 400,
            bootstrap_seed: 42,
        }
    }
}

impl Scan for EventShockMetricsFoldScan {
    type In = LabeledTradeResult;
    type Out = EventShockMetricsSummary;
    type State = EventShockMetricsFoldState;

    fn init(&self) -> Self::State {
        EventShockMetricsFoldState::default()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            LabeledTradeResult::Treatment(t) => {
                let r = t.gross_return;
                state.treatment_by_event.insert(t.event_id.0, r);
                state.treatment_returns.push(r);
            }
            LabeledTradeResult::Control {
                matched_event_id,
                trade,
            } => {
                if let Some(&tr) = state.treatment_by_event.get(&matched_event_id.0) {
                    state.pairs.push((tr, trade.gross_return));
                }
            }
        }
        let m = mean(&state.treatment_returns);
        let med = median(state.treatment_returns.clone());
        let hr = hit_rate(&state.treatment_returns);
        let sd = std_sample(&state.treatment_returns, m);
        let boot = if !state.pairs.is_empty() {
            Some(bootstrap_paired_diff_mean(
                &state.pairs,
                self.bootstrap_seed,
                self.bootstrap_iterations,
            ))
        } else {
            None
        };
        emit.emit(EventShockMetricsSummary {
            count: state.treatment_returns.len() as u64,
            mean_return: m,
            median_return: med,
            hit_rate: hr,
            std_dev: sd,
            bootstrap_mean_diff_vs_control: boot,
        });
    }
}

impl FlushableScan for EventShockMetricsFoldScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for EventShockMetricsFoldScan {
    type Snapshot = EventShockMetricsFoldSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventShockMetricsFoldSnapshot {
            treatment_by_event: state
                .treatment_by_event
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect(),
            treatment_returns: state.treatment_returns.clone(),
            pairs: state.pairs.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventShockMetricsFoldState {
            treatment_by_event: snapshot.treatment_by_event.into_iter().collect(),
            treatment_returns: snapshot.treatment_returns,
            pairs: snapshot.pairs,
        }
    }
}

impl VersionedSnapshot for EventShockMetricsFoldSnapshot {
    const VERSION: u32 = 1;
}
