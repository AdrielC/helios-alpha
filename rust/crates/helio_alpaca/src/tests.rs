use std::collections::VecDeque;

use helio_execution::{
    BrokerLifecyclePort, BrokerPort, MoneyMicros, OrderIntent, OrderProposal, PriceMicros,
    QuantityMicros,
};

use super::*;

#[derive(Debug, Default)]
struct FakeTransport {
    responses: VecDeque<Result<HttpResponse, TransportError>>,
    seen: Vec<HttpRequest>,
}

impl FakeTransport {
    fn with(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        Self {
            responses: responses.into_iter().map(Ok).collect(),
            seen: Vec::new(),
        }
    }
}

impl AlpacaTransport for FakeTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.seen.push(request);
        self.responses
            .pop_front()
            .unwrap_or(Err(TransportError::Unavailable))
    }
}

fn response(status: u16, body: serde_json::Value) -> HttpResponse {
    HttpResponse {
        status,
        body: serde_json::to_vec(&body).unwrap(),
    }
}

fn order(status: &str, filled_qty: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "broker-order-1",
        "client_order_id": "client-order-1",
        "status": status,
        "symbol": "SPY",
        "side": "buy",
        "qty": "2",
        "filled_qty": filled_qty,
        "filled_avg_price": if filled_qty == "0" { serde_json::Value::Null } else { serde_json::json!("499.125") },
        "submitted_at": "2026-08-31T17:20:00.123456789Z",
        "updated_at": "2026-08-31T17:20:01.123456789Z"
    })
}

fn broker(transport: FakeTransport) -> AlpacaBroker<FakeTransport> {
    AlpacaBroker::try_new(
        AlpacaConfig::paper(),
        AlpacaCredentials::try_new("paper-key", "paper-secret").unwrap(),
        transport,
    )
    .unwrap()
}

fn intent() -> OrderIntent {
    OrderIntent {
        client_order_id: "client-order-1".into(),
        proposal: OrderProposal {
            proposal_id: "proposal-1".into(),
            strategy_id: "strategy-1".into(),
            symbol: "SPY".into(),
            venue: "ALPACA".into(),
            currency: "USD".into(),
            side: Side::Buy,
            quantity: QuantityMicros(2_000_000),
            limit_price: PriceMicros(499_125_000),
            mode: ExecutionMode::Paper,
            trading_day: 20260831,
        },
        authorized_notional: MoneyMicros(998_250_000),
        risk_policy_version: "paper-risk-v1".into(),
        authorized_at_ns: 1,
    }
}

#[test]
fn credentials_and_request_debug_are_redacted() {
    let credentials = AlpacaCredentials::try_new("very-secret-key", "more-secret").unwrap();
    let credentials_debug = format!("{credentials:?}");
    assert!(!credentials_debug.contains("very-secret-key"));
    assert!(!credentials_debug.contains("more-secret"));
    let mut headers = BTreeMap::new();
    credentials.apply(&mut headers);
    let request = HttpRequest {
        method: HttpMethod::Post,
        path_and_query: "/v2/orders".into(),
        headers,
        body: br#"{"secret":"body"}"#.to_vec(),
    };
    let request_debug = format!("{request:?}");
    assert!(!request_debug.contains("very-secret-key"));
    assert!(!request_debug.contains("secret"));
}

#[test]
fn broker_port_submits_exact_fixed_point_paper_limit_order() {
    let mut adapter = broker(FakeTransport::with([response(200, order("new", "0"))]));
    let acknowledgement = adapter.submit(&intent()).unwrap();
    assert_eq!(acknowledgement.client_order_id, "client-order-1");

    let transport = adapter.into_transport();
    assert_eq!(transport.seen.len(), 1);
    let request = &transport.seen[0];
    assert_eq!(request.method, HttpMethod::Post);
    assert_eq!(request.path_and_query, "/v2/orders");
    assert_eq!(request.headers["APCA-API-KEY-ID"], "paper-key");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request.body).unwrap(),
        serde_json::json!({
            "symbol": "SPY",
            "qty": "2",
            "side": "buy",
            "type": "limit",
            "time_in_force": "day",
            "client_order_id": "client-order-1",
            "extended_hours": false,
            "limit_price": "499.125"
        })
    );
}

#[test]
fn market_and_extended_hours_order_constraints_fail_closed() {
    let mut adapter = broker(FakeTransport::default());
    let base = AlpacaOrderRequest {
        client_order_id: "market-1".into(),
        symbol: "SPY".into(),
        side: Side::Buy,
        quantity: BrokerDecimal::try_new("1").unwrap(),
        order_type: AlpacaOrderType::Market,
        time_in_force: AlpacaTimeInForce::Day,
        limit_price: None,
        extended_hours: true,
    };
    assert!(matches!(
        adapter.submit_order(&base),
        Err(BrokerError::Rejected(_))
    ));
    let mut malformed = base;
    malformed.extended_hours = false;
    malformed.limit_price = Some(BrokerDecimal::try_new("1").unwrap());
    assert!(matches!(
        adapter.submit_order(&malformed),
        Err(BrokerError::Rejected(_))
    ));
}

#[test]
fn ambiguous_mutation_never_reports_a_rejection_or_success() {
    let transport = FakeTransport {
        responses: VecDeque::from([Err(TransportError::OutcomeUnknown)]),
        seen: Vec::new(),
    };
    let mut adapter = broker(transport);
    assert_eq!(
        adapter.submit(&intent()),
        Err(BrokerError::AmbiguousOutcome)
    );
}

#[test]
fn reconciliation_attaches_exact_fill_activities() {
    let fills = serde_json::json!([{
        "id": "fill-9",
        "activity_type": "FILL",
        "transaction_time": "2026-08-31T17:20:01.5Z",
        "type": "partial_fill",
        "price": "499.125",
        "qty": "0.5",
        "side": "buy",
        "symbol": "SPY",
        "leaves_qty": "1.5",
        "order_id": "broker-order-1",
        "cum_qty": "0.5"
    }]);
    let mut adapter = broker(FakeTransport::with([
        response(200, order("partially_filled", "0.5")),
        response(200, fills),
    ]));
    let snapshot = adapter
        .fetch_order_by_client_order_id("client-order-1")
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.state, BrokerOrderState::PartiallyFilled);
    assert_eq!(snapshot.executions.len(), 1);
    assert_eq!(snapshot.executions[0].execution_id, "fill-9");
    assert_eq!(snapshot.executions[0].quantity.as_str(), "0.5");
}

#[test]
fn cancel_is_async_and_returns_refetched_broker_truth() {
    let mut adapter = broker(FakeTransport::with([
        response(200, order("new", "0")),
        HttpResponse {
            status: 204,
            body: Vec::new(),
        },
        response(200, order("canceled", "0")),
    ]));
    let snapshot = adapter.cancel_by_client_order_id("client-order-1").unwrap();
    assert_eq!(snapshot.state, BrokerOrderState::Canceled);
    let transport = adapter.into_transport();
    assert_eq!(transport.seen[1].method, HttpMethod::Delete);
    assert_eq!(
        transport.seen[1].path_and_query,
        "/v2/orders/broker-order-1"
    );
}

#[test]
fn positions_preserve_signed_exact_decimals() {
    let positions = serde_json::json!([{
        "asset_id": "asset-1",
        "symbol": "SPY",
        "exchange": "ARCA",
        "asset_class": "us_equity",
        "qty": "-2.5000",
        "qty_available": "-2.5",
        "side": "short",
        "avg_entry_price": "500.00",
        "market_value": "-1245.125",
        "cost_basis": "-1250",
        "unrealized_pl": "4.875",
        "unrealized_plpc": "0.0039",
        "current_price": "498.05",
        "lastday_price": "497.90",
        "change_today": "0.000301"
    }]);
    let mut adapter = broker(FakeTransport::with([response(200, positions)]));
    let positions = adapter.positions().unwrap();
    assert_eq!(positions[0].quantity.as_str(), "-2.5");
    assert_eq!(positions[0].market_value.as_str(), "-1245.125");
}

#[test]
fn startup_inventory_is_bounded_and_queries_only_open_orders() {
    let mut adapter = broker(FakeTransport::with([response(
        200,
        serde_json::json!([order("new", "0")]),
    )]));
    let orders = adapter.open_orders_for_reconciliation().unwrap();
    assert_eq!(orders.len(), 1);
    let transport = adapter.into_transport();
    assert_eq!(
        transport.seen[0].path_and_query,
        "/v2/orders?status=open&limit=100&direction=asc&nested=false"
    );
}

#[test]
fn market_frames_are_causal_bounded_and_restartable() {
    let mut normalizer = AlpacaMarketNormalizer::new("v2/iex");
    let records = normalizer
        .normalize_frame(
            br#"[
                {"T":"t","S":"SPY","i":91,"x":"V","p":499.125,"s":2,"c":["@"],"t":"2026-08-31T17:20:00.123456789Z","z":"C"},
                {"T":"q","S":"SPY","bx":"V","bp":499.12,"bs":10,"ax":"V","ap":499.13,"as":12,"c":["R"],"t":"2026-08-31T17:20:00.223456789Z","z":"C"}
            ]"#,
            1_788_196_801_000_000_000,
        )
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].offset.sequence, 1);
    assert_eq!(records[1].offset.sequence, 2);
    assert_eq!(records[0].available_at, records[0].observed_at);

    let checkpoint = normalizer.checkpoint();
    let mut resumed = AlpacaMarketNormalizer::resume("v2/iex", checkpoint).unwrap();
    let next = resumed
        .normalize_frame(
            br#"[{"T":"b","S":"SPY","o":499,"h":500,"l":498,"c":499.5,"v":1000,"n":44,"vw":499.25,"t":"2026-08-31T17:21:00Z"}]"#,
            1_788_196_861_000_000_000,
        )
        .unwrap();
    assert_eq!(next[0].offset.sequence, 3);
}

#[test]
fn invalid_market_frame_is_transactional() {
    let mut normalizer = AlpacaMarketNormalizer::new("v2/iex");
    let before = normalizer.checkpoint();
    assert!(normalizer
        .normalize_frame(br#"[{"T":"t","S":"SPY","i":1}]"#, 1_788_196_801_000_000_000,)
        .is_err());
    assert_eq!(normalizer.checkpoint(), before);
}

#[test]
fn binary_trade_update_payload_preserves_execution_identity_and_decimal_text() {
    let frame = parse_trading_frame(
        br#"{
            "stream":"trade_updates",
            "data":{
                "event":"partial_fill",
                "execution_id":"exec-7",
                "timestamp":"2026-08-31T17:20:01.5Z",
                "price":"499.1250",
                "qty":"0.5000",
                "order":{
                    "id":"broker-order-1",
                    "client_order_id":"client-order-1",
                    "symbol":"SPY",
                    "side":"buy",
                    "status":"partially_filled",
                    "qty":"2.0",
                    "filled_qty":"0.5",
                    "filled_avg_price":"499.125"
                }
            }
        }"#,
    )
    .unwrap();
    let AlpacaTradingFrame::TradeUpdate { update } = frame else {
        panic!("expected trade update")
    };
    assert_eq!(update.execution_id.as_deref(), Some("exec-7"));
    assert_eq!(update.execution_quantity.unwrap().as_str(), "0.5");
    assert_eq!(update.execution_price.unwrap().as_str(), "499.125");
}

#[test]
fn signed_decimal_canonicalizes_and_rejects_exponents() {
    assert_eq!(
        SignedDecimal::try_new("-001.2300").unwrap().as_str(),
        "-1.23"
    );
    assert_eq!(SignedDecimal::try_new("-0.000").unwrap().as_str(), "0");
    assert!(SignedDecimal::try_new("1e9").is_err());
}

#[test]
fn fixed_point_conversion_is_exact_and_never_rounds_silently() {
    assert_eq!(
        broker_decimal_to_micros(&BrokerDecimal::try_new("499.125").unwrap()),
        Ok(499_125_000)
    );
    assert_eq!(
        signed_decimal_to_micros(&SignedDecimal::try_new("-2.5").unwrap()),
        Ok(-2_500_000)
    );
    assert_eq!(
        broker_decimal_to_micros(&BrokerDecimal::try_new("0.0000001").unwrap()),
        Err(MicrosConversionError::PrecisionLoss)
    );
}
