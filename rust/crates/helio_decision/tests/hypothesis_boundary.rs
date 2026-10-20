mod common;
use common::*;
use helio_decision::*;
use helio_hypothesis::*;
use helio_time::{AvailableAt, EffectiveAt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct State {
    request_id: String,
}
#[derive(Clone)]
enum Evidence {
    Observation(DecisionRequest),
    Decision(DecisionReceipt),
}
#[derive(Debug, PartialEq)]
enum Action {
    Infer(DecisionRequest),
    Assessed(DecisionResponse),
}
struct Model;
impl HypothesisModel<String> for Model {
    type Evidence = Evidence;
    type State = State;
    type Output = Action;
    type Error = &'static str;
    fn open(
        &self,
        _: &String,
        evidence: CausalEvidence<Evidence>,
    ) -> Result<HypothesisTransition<State, Action>, Self::Error> {
        let Evidence::Observation(request) = evidence.payload else {
            return Err("expected observation");
        };
        Ok(HypothesisTransition::new(State {
            request_id: request.request_id.clone(),
        })
        .schedule(TimerId(1), AvailableAt(request.deadline_ms as i64))
        .emit(Action::Infer(request)))
    }
    fn update(
        &self,
        _: &String,
        state: &State,
        evidence: CausalEvidence<Evidence>,
    ) -> Result<HypothesisTransition<State, Action>, Self::Error> {
        let Evidence::Decision(receipt) = evidence.payload else {
            return Err("expected receipt");
        };
        if receipt.request_id != state.request_id {
            return Err("wrong identity");
        }
        // Completion removes outstanding timers; adding Cancel is an invalid
        // completion transition in the hypothesis kernel.
        let next = HypothesisTransition::new(state.clone()).complete();
        Ok(match receipt.outcome {
            Ok(response) => next.emit(Action::Assessed(response)),
            Err(_) => next,
        })
    }
    fn on_timer(
        &self,
        _: &String,
        state: &State,
        _: TimerId,
        _: AvailableAt,
    ) -> Result<HypothesisTransition<State, Action>, Self::Error> {
        Ok(HypothesisTransition::new(state.clone()).complete())
    }
    fn validate(&self, _: &String, state: &State) -> Result<(), Self::Error> {
        if state.request_id.is_empty() {
            Err("missing identity")
        } else {
            Ok(())
        }
    }
}

#[test]
fn snapshot_restore_preserves_external_request_then_accepts_delayed_receipt() {
    let build = || {
        KeyedHypothesisMachine::try_new(
            Model,
            HypothesisConfig::try_new(4, 8, 2, 4, 10_000).unwrap(),
        )
        .unwrap()
    };
    let mut engine = HypothesisEngine::<String, _, String>::new(build());
    let r = request();
    let events = engine.process(HypothesisInput::Open {
        key: "incident".into(),
        evidence: CausalEvidence::new(
            0,
            EffectiveAt(50),
            AvailableAt(100),
            Evidence::Observation(r.clone()),
        ),
    });
    assert!(events.iter().any(|e| matches!(e, HypothesisEvent::ModelOutput { output: Action::Infer(request), .. } if request == &r)));
    let snapshot = engine.snapshot();
    let mut restored =
        HypothesisEngine::<String, _, String>::try_from_snapshot(build(), snapshot).unwrap();
    let receipt = DecisionReceipt {
        schema_version: SCHEMA_VERSION,
        request_id: r.request_id,
        fingerprint: "test".into(),
        effective_at_ms: 50,
        available_at_ms: 100,
        received_at_ms: 150,
        latency_ms: 50,
        outcome: adapter(Protocol::OpenAi)
            .decode(&request(), predicate_response(Protocol::OpenAi, 0.8)),
    };
    let evidence = receipt.into_evidence(1).unwrap();
    let events = restored.process(HypothesisInput::Evidence {
        key: "incident".into(),
        evidence: CausalEvidence::new(
            evidence.sequence,
            evidence.effective_at,
            evidence.available_at,
            Evidence::Decision(evidence.payload),
        ),
    });
    assert!(events.iter().any(|e| matches!(
        e,
        HypothesisEvent::ModelOutput {
            output: Action::Assessed(_),
            ..
        }
    )));
}
