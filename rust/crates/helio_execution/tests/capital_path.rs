use std::collections::BTreeSet;

use helio_execution::{
    evaluate_capital_admission, BrokerAcknowledgement, CapitalAdmissionPolicy, EvidenceArtifact,
    EvidenceLedger, ExecutionMode, MoneyMicros, OrderGateway, OrderGatewayPolicy, OrderIntent,
    OrderProposal, PaperBroker, PaperBrokerFault, PortfolioRiskSnapshot, PriceMicros,
    QuantityMicros, ReadinessReport, RiskAuthority, RiskContext, RiskDecision, RiskPolicy, Side,
};
use helio_scan::{
    drain_outbox, AtomicCommitBundle, Checkpoint, CommitFault, InMemoryAtomicCommitStore,
    OutboxStatus, OutputId, TransactionalOutput,
};
use helio_time::VenueSchedule;

fn risk_policy() -> RiskPolicy {
    RiskPolicy {
        version: "risk-production-1".into(),
        live_enabled: true,
        allowed_venues: BTreeSet::from(["XNYS".into()]),
        max_market_data_age_ns: 1_000,
        max_portfolio_age_ns: 2_000,
        max_order_notional: MoneyMicros(100_000_000),
        max_gross_exposure: MoneyMicros(1_000_000_000),
        max_strategy_exposure: MoneyMicros(500_000_000),
        max_symbol_position_micros: 10_000_000,
        max_daily_orders: 10,
    }
}

fn proposal(id: &str, mode: ExecutionMode) -> OrderProposal {
    OrderProposal {
        proposal_id: id.into(),
        strategy_id: "space-weather-shock-v1".into(),
        symbol: "GRID".into(),
        venue: "XNYS".into(),
        currency: "USD".into(),
        side: Side::Buy,
        quantity: QuantityMicros(1_000_000),
        limit_price: PriceMicros(25_000_000),
        mode,
        trading_day: 20_782,
    }
}

fn authorize(id: &str, mode: ExecutionMode) -> OrderIntent {
    let schedule: VenueSchedule = serde_json::from_str(include_str!(
        "../../helio_time/tests/fixtures/xnys_2026_thanksgiving.json"
    ))
    .unwrap();
    schedule.validate().unwrap();
    let mut authority = RiskAuthority::new(
        risk_policy(),
        PortfolioRiskSnapshot::empty(9_000, 20_782),
        schedule,
    );
    let decision = authority
        .authorize(
            proposal(id, mode),
            RiskContext {
                now_ns: 10_000,
                market_data_at_ns: 9_500,
                venue_time_utc_sec: 1_795_620_000,
            },
        )
        .unwrap();
    match decision {
        RiskDecision::Approved(intent) => *intent,
        RiskDecision::Rejected(reason) => panic!("test order rejected: {reason:?}"),
    }
}

fn gateway(broker: PaperBroker) -> OrderGateway<PaperBroker> {
    OrderGateway::new(
        OrderGatewayPolicy {
            environment: "production".into(),
            max_risk_authorization_age_ns: 1_000,
            allowed_risk_policy_versions: BTreeSet::from(["risk-production-1".into()]),
        },
        broker,
    )
}

#[test]
fn atomic_outbox_to_broker_survives_both_ambiguous_boundaries() {
    let intent = authorize("shock/42/order/0", ExecutionMode::Paper);
    let output_id = OutputId::try_new(intent.client_order_id.clone()).unwrap();
    let bundle = AtomicCommitBundle {
        transaction_id: "space-weather/partition-0/100-100".into(),
        expected_next_offset: 100,
        next_offset: 101,
        checkpoint: Checkpoint::new("hypothesis-state-v7".to_owned(), 101),
        outputs: vec![TransactionalOutput {
            id: output_id.clone(),
            source_offset: 100,
            payload: intent,
        }],
    };
    let mut store =
        InMemoryAtomicCommitStore::<String, OrderIntent, BrokerAcknowledgement>::new(100);
    assert!(store
        .commit_with_fault(bundle.clone(), CommitFault::AfterCommit)
        .is_err());
    assert!(store.commit(bundle).unwrap().replayed);

    let mut broker = PaperBroker::new(10_001);
    broker.inject_fault(PaperBrokerFault::AcceptThenTimeoutOnce);
    let mut gateway = gateway(broker);
    assert_eq!(drain_outbox(&mut store, &mut gateway), Ok(1));
    assert_eq!(gateway.broker().accepted_order_count(), 1);
    assert!(matches!(
        store.state().outbox[&output_id].status,
        OutboxStatus::Delivered { .. }
    ));
}

#[test]
fn live_gateway_accepts_only_an_admitted_authorization() {
    let policy = CapitalAdmissionPolicy::production_default("capital-1", 500);
    let mut evidence = EvidenceLedger::default();
    for kind in policy.required_evidence() {
        evidence.record(EvidenceArtifact {
            kind: *kind,
            artifact_id: format!("ci-{kind:?}"),
            sha256: "b".repeat(64),
            environment: "production".into(),
            observed_at_ns: 9_900,
            expires_at_ns: 20_000,
            passed: true,
        });
    }
    let authorization = evaluate_capital_admission(
        &policy,
        &evidence,
        &ReadinessReport {
            observed_at_ns: 10_000,
            blockers: Vec::new(),
        },
        10_000,
    )
    .unwrap();
    let mut gateway = gateway(PaperBroker::new(10_001));
    let receipt = gateway
        .dispatch(
            &authorize("shock/43/order/0", ExecutionMode::Live),
            Some(&authorization),
            10_001,
        )
        .unwrap();
    assert_eq!(receipt.acknowledgement.client_order_id, "shock/43/order/0");
    assert_eq!(gateway.broker().accepted_order_count(), 1);
}
