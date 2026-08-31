use super::*;
use crate::fixtures::empty_snapshot;
use crate::types::{OrderRequest, TimeInForce as ViewTimeInForce};
use helio_alpaca::{
    AlpacaConfig, AlpacaCredentials, HttpMethod, HttpRequest, HttpResponse, TransportError,
};
use helio_execution::MoneyMicros;
use helio_scan::SessionDate;
use helio_time::{compute_source_sha256, VenueSchedule, VenueScheduleMetadata, VenueSession};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
struct FakeTransport {
    responses: Arc<Mutex<VecDeque<Result<HttpResponse, TransportError>>>>,
    seen: Arc<Mutex<Vec<HttpRequest>>>,
}

impl FakeTransport {
    fn with(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().map(Ok).collect())),
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl AlpacaTransport for FakeTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.seen.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(TransportError::Unavailable))
    }
}

#[derive(Debug)]
struct ManualClock(u64);

impl ExecutionClock for ManualClock {
    fn now_ns(&self) -> Result<u64, PaperExecutorError> {
        Ok(self.0)
    }
}

fn json(status: u16, body: serde_json::Value) -> HttpResponse {
    HttpResponse {
        status,
        body: serde_json::to_vec(&body).unwrap(),
    }
}

fn no_content() -> HttpResponse {
    HttpResponse {
        status: 204,
        body: Vec::new(),
    }
}

fn account() -> HttpResponse {
    json(
        200,
        serde_json::json!({
            "id": "account-1",
            "status": "ACTIVE",
            "currency": "USD",
            "buying_power": "100000",
            "cash": "100000",
            "portfolio_value": "100000",
            "equity": "100000",
            "last_equity": "100000",
            "long_market_value": "0",
            "short_market_value": "0",
            "initial_margin": "0",
            "maintenance_margin": "0",
            "daytrade_count": 0,
            "trading_blocked": false,
            "transfers_blocked": false,
            "account_blocked": false,
            "trade_suspended_by_user": false
        }),
    )
}

fn positions() -> HttpResponse {
    json(200, serde_json::json!([]))
}

fn order(status: &str) -> HttpResponse {
    json(200, order_value(status, "manual-order-1"))
}

fn order_value(status: &str, client_order_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "broker-order-1",
        "client_order_id": client_order_id,
        "status": status,
        "symbol": "SPY",
        "side": "buy",
        "qty": "1",
        "filled_qty": "0",
        "filled_avg_price": null,
        "submitted_at": "1970-01-01T02:46:40Z",
        "updated_at": "1970-01-01T02:46:40Z"
    })
}

fn filled_order() -> HttpResponse {
    json(
        200,
        serde_json::json!({
            "id": "broker-order-1",
            "client_order_id": "manual-order-1",
            "status": "filled",
            "symbol": "SPY",
            "side": "buy",
            "qty": "1",
            "filled_qty": "1",
            "filled_avg_price": "25",
            "submitted_at": "1970-01-01T02:46:40Z",
            "updated_at": "1970-01-01T02:46:40.1Z"
        }),
    )
}

fn fill_activities() -> HttpResponse {
    json(
        200,
        serde_json::json!([{
            "id": "execution-1",
            "transaction_time": "1970-01-01T02:46:40.1Z",
            "price": "25",
            "qty": "1",
            "order_id": "broker-order-1"
        }]),
    )
}

fn filled_position() -> HttpResponse {
    json(
        200,
        serde_json::json!([{
            "asset_id": "asset-spy",
            "symbol": "SPY",
            "exchange": "ARCA",
            "asset_class": "us_equity",
            "qty": "1",
            "qty_available": "1",
            "side": "long",
            "avg_entry_price": "25",
            "market_value": "25",
            "cost_basis": "25",
            "unrealized_pl": "0",
            "unrealized_plpc": "0",
            "current_price": "25",
            "lastday_price": "25",
            "change_today": "0"
        }]),
    )
}

fn missing_order() -> HttpResponse {
    json(
        404,
        serde_json::json!({"code": 40410000, "message": "not found"}),
    )
}

fn schedule() -> VenueSchedule {
    let sessions = vec![VenueSession {
        label: SessionDate(1),
        open_utc: 9_500,
        close_utc: 10_500,
        breaks: Vec::new(),
    }];
    let mut metadata = VenueScheduleMetadata {
        schema_version: 1,
        venue: "XNYS".into(),
        timezone: "America/New_York".into(),
        source: "test-calendar".into(),
        source_version: "1".into(),
        source_sha256: "0".repeat(64),
        generated_at_utc: 9_000,
        valid_from_utc: 9_000,
        valid_until_utc: 11_000,
    };
    metadata.source_sha256 = compute_source_sha256(&metadata, &sessions).unwrap();
    VenueSchedule::try_new(metadata, sessions).unwrap()
}

fn risk_policy() -> RiskPolicy {
    RiskPolicy {
        version: "paper-risk-v1".into(),
        live_enabled: false,
        allowed_venues: BTreeSet::from(["XNYS".into()]),
        max_market_data_age_ns: 1_000,
        max_portfolio_age_ns: 1_000,
        max_order_notional: MoneyMicros(100_000_000),
        max_gross_exposure: MoneyMicros(1_000_000_000),
        max_strategy_exposure: MoneyMicros(500_000_000),
        max_symbol_position_micros: 10_000_000,
        max_daily_orders: 10,
    }
}

#[test]
fn checked_in_paper_policy_is_parseable_and_cannot_admit_live_execution() {
    let policy: RiskPolicy =
        serde_json::from_str(include_str!("../../../../../config/risk/alpaca-paper.json")).unwrap();
    assert!(!policy.live_enabled);
    assert_eq!(policy.allowed_venues, BTreeSet::from(["XNYS".into()]));
    assert!(policy.max_order_notional.0 > 0);
}

fn command(action: CommandAction) -> CommandRequest {
    let submits_order = action == CommandAction::SubmitOrder;
    CommandRequest {
        schema_version: 1,
        action,
        target_id: "manual-order-1".into(),
        reason: "Paper execution acceptance test".into(),
        confirmation: "CONFIRM".into(),
        expected_sequence: 1,
        order: submits_order.then(|| OrderRequest {
            instrument: "SPY".into(),
            side: ViewSide::Buy,
            quantity_micros: "1000000".into(),
            order_type: OrderType::Limit,
            limit_price_micros: Some("25000000".into()),
            time_in_force: ViewTimeInForce::Day,
            strategy_id: Some("manual".into()),
        }),
    }
}

#[test]
fn reconciliation_requires_explicit_broker_and_oms_state_agreement() {
    assert!(broker_and_oms_match(
        BrokerOrderState::Open,
        OrderState::Working
    ));
    assert!(!broker_and_oms_match(
        BrokerOrderState::Pending,
        OrderState::Working
    ));
    assert!(!broker_and_oms_match(
        BrokerOrderState::Failed,
        OrderState::Unknown
    ));
}

fn executor(
    transport: FakeTransport,
    market_at_ns: u64,
) -> (AlpacaPaperCommandExecutor<FakeTransport>, FakeTransport) {
    let mut config = AlpacaConfig::paper();
    config.venue = "XNYS".into();
    let retained = transport.clone();
    let broker = AlpacaBroker::try_new(
        config,
        AlpacaCredentials::try_new("paper-key", "paper-secret").unwrap(),
        transport,
    )
    .unwrap();
    let market = Arc::new(InMemoryMarketReferencePort::default());
    market
        .update(MarketReference {
            symbol: "SPY".into(),
            price: PriceMicros(25_000_000),
            observed_at_ns: market_at_ns,
        })
        .unwrap();
    (
        AlpacaPaperCommandExecutor::try_new(
            "paper-primary",
            broker,
            risk_policy(),
            schedule(),
            market,
            Arc::new(ManualClock(10_000_000_000_000)),
        )
        .unwrap(),
        retained,
    )
}

#[tokio::test]
async fn paper_limit_order_flows_through_risk_oms_broker_and_projection_once() {
    let responses = [
        account(),
        positions(),
        missing_order(),
        order("new"),
        account(),
        positions(),
        account(),
        positions(),
        order("new"),
        account(),
        positions(),
    ];
    let (executor, transport) = executor(FakeTransport::with(responses), 10_000_000_000_000 - 100);
    let store = OperatorStore::new(empty_snapshot()).unwrap();
    let request = command(CommandAction::SubmitOrder);

    let first = executor
        .execute("operator", &request, &store)
        .await
        .unwrap();
    let replay = executor
        .execute("operator", &request, &store)
        .await
        .unwrap();
    assert_eq!(first.status, CommandStatus::Accepted);
    assert_eq!(replay.status, CommandStatus::Accepted);
    let snapshot = store.snapshot().await;
    assert_eq!(snapshot.orders.len(), 1);
    assert_eq!(snapshot.orders[0].state, ViewOrderState::Working);
    assert_eq!(snapshot.orders[0].oms_version, Some(2));
    assert_eq!(snapshot.risk.daily_order_count, 1);
    assert_eq!(snapshot.risk.reserved_gross_micros, "25000000");

    let seen = transport.seen.lock().unwrap();
    assert_eq!(
        seen.iter()
            .filter(|request| request.method == HttpMethod::Post)
            .count(),
        1
    );
}

#[tokio::test]
async fn stale_market_reference_rejects_before_broker_submission() {
    let (executor, transport) = executor(
        FakeTransport::with([account(), positions()]),
        10_000_000_000_000 - 10_000,
    );
    let store = OperatorStore::new(empty_snapshot()).unwrap();
    let receipt = executor
        .execute("operator", &command(CommandAction::SubmitOrder), &store)
        .await
        .unwrap();
    assert_eq!(receipt.status, CommandStatus::Rejected);
    assert!(store.snapshot().await.orders.is_empty());
    assert!(transport
        .seen
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.method == HttpMethod::Get));
}

#[tokio::test]
async fn cancel_is_confirmed_by_broker_truth_before_oms_terminal_state() {
    let responses = [
        account(),
        positions(),
        missing_order(),
        order("new"),
        account(),
        positions(),
        order("new"),
        no_content(),
        order("canceled"),
        account(),
        positions(),
    ];
    let (executor, _) = executor(FakeTransport::with(responses), 10_000_000_000_000 - 100);
    let store = OperatorStore::new(empty_snapshot()).unwrap();
    executor
        .execute("operator", &command(CommandAction::SubmitOrder), &store)
        .await
        .unwrap();
    let receipt = executor
        .execute("operator", &command(CommandAction::CancelOrder), &store)
        .await
        .unwrap();
    assert_eq!(receipt.status, CommandStatus::Accepted);
    assert_eq!(
        store.snapshot().await.orders[0].state,
        ViewOrderState::Canceled
    );
}

#[tokio::test]
async fn asynchronous_trade_update_reconciles_fill_position_and_risk_reservation() {
    let responses = [
        account(),
        positions(),
        missing_order(),
        order("new"),
        account(),
        positions(),
        filled_order(),
        fill_activities(),
        account(),
        filled_position(),
    ];
    let (executor, _) = executor(FakeTransport::with(responses), 10_000_000_000_000 - 100);
    let store = OperatorStore::new(empty_snapshot()).unwrap();
    executor
        .execute("operator", &command(CommandAction::SubmitOrder), &store)
        .await
        .unwrap();
    executor
        .reconcile_trade_update(
            AlpacaTradeUpdate {
                event: "fill".into(),
                execution_id: Some("execution-1".into()),
                timestamp: "1970-01-01T02:46:40.1Z".into(),
                order_id: "broker-order-1".into(),
                client_order_id: "manual-order-1".into(),
                symbol: "SPY".into(),
                side: "buy".into(),
                status: "filled".into(),
                quantity: helio_execution::BrokerDecimal::try_new("1").unwrap(),
                filled_quantity: helio_execution::BrokerDecimal::try_new("1").unwrap(),
                filled_average_price: Some(helio_execution::BrokerDecimal::try_new("25").unwrap()),
                execution_price: Some(helio_execution::BrokerDecimal::try_new("25").unwrap()),
                execution_quantity: Some(helio_execution::BrokerDecimal::try_new("1").unwrap()),
            },
            store.clone(),
        )
        .await
        .unwrap();
    let snapshot = store.snapshot().await;
    assert_eq!(snapshot.orders[0].state, ViewOrderState::Filled);
    assert_eq!(snapshot.fills.len(), 1);
    assert_eq!(snapshot.positions.len(), 1);
    assert_eq!(snapshot.risk.reserved_gross_micros, "0");
    assert_eq!(snapshot.risk.gross_exposure_micros, "25000000");
}

#[tokio::test]
async fn startup_reconciliation_admits_only_complete_active_order_agreement() {
    let responses = [
        account(),
        positions(),
        missing_order(),
        order("new"),
        account(),
        positions(),
        json(
            200,
            serde_json::json!([order_value("new", "manual-order-1")]),
        ),
        order("new"),
        account(),
        positions(),
    ];
    let (executor, _) = executor(FakeTransport::with(responses), 10_000_000_000_000 - 100);
    let store = OperatorStore::new(empty_snapshot()).unwrap();
    executor
        .execute("operator", &command(CommandAction::SubmitOrder), &store)
        .await
        .unwrap();

    executor.startup_reconcile(store.clone()).await.unwrap();

    let snapshot = store.snapshot().await;
    assert_eq!(snapshot.orders.len(), 1);
    assert_eq!(snapshot.orders[0].state, ViewOrderState::Working);
    assert_eq!(snapshot.risk.reserved_gross_micros, "25000000");
}

#[tokio::test]
async fn startup_reconciliation_fails_closed_on_broker_only_active_order() {
    let responses = [json(
        200,
        serde_json::json!([order_value("new", "external-order-9")]),
    )];
    let (executor, _) = executor(FakeTransport::with(responses), 10_000_000_000_000 - 100);
    let store = OperatorStore::new(empty_snapshot()).unwrap();

    let error = executor.startup_reconcile(store).await.unwrap_err();

    assert!(error.to_string().contains("absent from the durable OMS"));
}

#[tokio::test]
async fn startup_reconciliation_marks_missing_durable_order_unknown_before_failing() {
    let responses = [
        account(),
        positions(),
        missing_order(),
        order("new"),
        account(),
        positions(),
        json(200, serde_json::json!([])),
        missing_order(),
    ];
    let (executor, _) = executor(FakeTransport::with(responses), 10_000_000_000_000 - 100);
    let store = OperatorStore::new(empty_snapshot()).unwrap();
    executor
        .execute("operator", &command(CommandAction::SubmitOrder), &store)
        .await
        .unwrap();

    assert!(executor.startup_reconcile(store).await.is_err());
    let state = executor.state.lock().unwrap();
    assert_eq!(
        state.oms.order("manual-order-1").unwrap().unwrap().state,
        OrderState::Unknown
    );
}
