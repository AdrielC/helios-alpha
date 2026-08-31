use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FeedMode {
    Demo,
    Shadow,
    Paper,
    Live,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DataClass {
    Synthetic,
    Observed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    Healthy,
    Degraded,
    Stale,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalState {
    Observing,
    Eligible,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    PendingSubmit,
    Working,
    PartiallyFilled,
    PendingCancel,
    PendingReplace,
    Filled,
    Canceled,
    Rejected,
    Expired,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StrategyState {
    Running,
    Paused,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageState {
    Running,
    Paused,
    Blocked,
    Replaying,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Critical,
    Warning,
    Info,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AlertStatus {
    Open,
    Acknowledged,
    Resolved,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationsContext {
    pub organization_id: String,
    pub organization_name: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub account_id: String,
    pub account_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SignalPoint {
    pub offset_seconds: f64,
    pub value_bps: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SignalView {
    pub id: String,
    pub strategy_id: String,
    pub hypothesis: String,
    pub instrument: String,
    pub state: SignalState,
    pub posterior_bps: f64,
    pub trigger: String,
    pub horizon: String,
    pub observed_at: String,
    pub available_at: String,
    pub decision_cut: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
    pub lineage: Vec<String>,
    pub trace: Vec<SignalPoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PositionView {
    pub instrument: String,
    pub strategy: String,
    pub quantity_micros: String,
    pub average_price_micros: String,
    pub mark_price_micros: String,
    pub market_value_micros: String,
    pub unrealized_pnl_micros: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_pnl_micros: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_change_bps: Option<i64>,
    pub currency: String,
    pub freshness_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimeInForce {
    Day,
    GoodTillCanceled,
    ImmediateOrCancel,
    FillOrKill,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReconciliationState {
    Matched,
    Pending,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrderView {
    pub client_order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_order_id: Option<String>,
    pub instrument: String,
    pub side: Side,
    pub state: OrderState,
    pub quantity_micros: String,
    pub filled_quantity_micros: String,
    pub limit_price_micros: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_price_micros: Option<String>,
    pub venue: String,
    pub strategy: String,
    pub submitted_at: String,
    pub reconciliation: ReconciliationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oms_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<TimeInForce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncertainty_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Liquidity {
    Maker,
    Taker,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FillView {
    pub execution_id: String,
    pub client_order_id: String,
    pub instrument: String,
    pub side: Side,
    pub quantity_micros: String,
    pub price_micros: String,
    pub venue: String,
    pub strategy: String,
    pub executed_at: String,
    pub liquidity: Liquidity,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub name: String,
    pub channel: String,
    pub health: HealthState,
    pub lag_ms: u64,
    pub watermark: String,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RiskView {
    pub gross_exposure_micros: String,
    pub gross_limit_micros: String,
    pub reserved_gross_micros: String,
    pub daily_order_count: u64,
    pub daily_order_limit: u64,
    pub pending_reconciliations: u64,
    pub open_incidents: u64,
    pub kill_switch_active: bool,
    pub capital_gate: CapitalGate,
    pub capital_gate_reason: String,
    pub checkpoint_age_ms: u64,
    pub source_lag_ms: u64,
    pub clock_offset_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CapitalGate {
    Closed,
    Authorized,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StrategyView {
    pub id: String,
    pub name: String,
    pub state: StrategyState,
    pub generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_signal_id: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StageView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub state: StageState,
    pub lag_ms: u64,
    pub checkpoint: String,
    pub detail: String,
    pub can_pause_before: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelatedEntity {
    pub kind: String,
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AlertView {
    pub id: String,
    pub severity: AlertSeverity,
    pub status: AlertStatus,
    pub category: String,
    pub title: String,
    pub detail: String,
    pub opened_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_entity: Option<RelatedEntity>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricPoint {
    pub timestamp: String,
    pub value: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceLine {
    pub label: String,
    pub value: f64,
    pub tone: ReferenceTone,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MetricUnit {
    #[serde(rename = "USD")]
    Usd,
    #[serde(rename = "%")]
    Percent,
    Ms,
    Count,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MetricTone {
    Cyan,
    Green,
    Coral,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReferenceTone {
    Neutral,
    Warning,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricSeriesView {
    pub id: String,
    pub label: String,
    pub unit: MetricUnit,
    pub tone: MetricTone,
    pub points: Vec<MetricPoint>,
    pub reference_lines: Vec<ReferenceLine>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivitySeverity {
    Normal,
    Warning,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub id: String,
    pub sequence: u64,
    pub occurred_at: String,
    pub category: String,
    pub source: String,
    pub stage: String,
    pub entity: String,
    pub outcome: String,
    pub severity: ActivitySeverity,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperationsSnapshot {
    pub schema_version: u8,
    pub sequence: u64,
    pub mode: FeedMode,
    pub provider: String,
    pub observed_at: String,
    pub data_class: DataClass,
    pub context: OperationsContext,
    pub strategies: Vec<StrategyView>,
    pub stages: Vec<StageView>,
    pub signals: Vec<SignalView>,
    pub positions: Vec<PositionView>,
    pub orders: Vec<OrderView>,
    pub fills: Vec<FillView>,
    pub sources: Vec<SourceView>,
    pub alerts: Vec<AlertView>,
    pub metrics: Vec<MetricSeriesView>,
    pub activity: Vec<ActivityView>,
    pub risk: RiskView,
}

impl OperationsSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 2 {
            return Err("operations snapshot schemaVersion must be 2".into());
        }
        if self.provider.trim().is_empty() || self.context.account_id.trim().is_empty() {
            return Err("provider and accountId must be non-empty".into());
        }
        if self.signals.iter().any(|signal| {
            !signal.posterior_bps.is_finite() || !(0.0..=10_000.0).contains(&signal.posterior_bps)
        }) {
            return Err("signal posteriorBps must be finite and between 0 and 10000".into());
        }
        for value in [
            &self.risk.gross_exposure_micros,
            &self.risk.gross_limit_micros,
            &self.risk.reserved_gross_micros,
        ] {
            value
                .parse::<i128>()
                .map_err(|_| "risk micros must be exact signed integers".to_string())?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SeriesDomain {
    Market,
    Signal,
    Source,
    Risk,
    Portfolio,
    Execution,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SeriesRender {
    Candlestick,
    Bar,
    Histogram,
    Line,
    Area,
    Baseline,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimeSeriesDescriptor {
    pub id: String,
    pub label: String,
    pub short_label: String,
    pub domain: SeriesDomain,
    pub unit: String,
    pub precision: u8,
    pub color: String,
    pub render: SeriesRender,
    pub provenance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_names: Option<Vec<String>>,
    pub freshness: String,
    pub default_visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_weight: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum TimeSeriesPoint {
    Scalar {
        timestamp: String,
        available_at: String,
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
    Ohlc {
        timestamp: String,
        available_at: String,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
    },
}

impl TimeSeriesPoint {
    pub fn timestamp(&self) -> &str {
        match self {
            Self::Scalar { timestamp, .. } | Self::Ohlc { timestamp, .. } => timestamp,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimeSeriesData {
    pub descriptor: TimeSeriesDescriptor,
    pub points: Vec<TimeSeriesPoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectedTimeSeries {
    pub id: String,
    pub points: Vec<TimeSeriesPoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimeSeriesProjection {
    pub schema_version: u8,
    pub projection_id: String,
    pub sequence: u64,
    pub observed_at: String,
    pub series: Vec<ProjectedTimeSeries>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MarkerKind {
    Order,
    Ack,
    Fill,
    Cancel,
    Replace,
    Alert,
    Model,
    Risk,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineMarker {
    pub id: String,
    pub timestamp: String,
    pub available_at: String,
    pub kind: MarkerKind,
    pub label: String,
    pub entity_id: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimeSeriesWindow {
    pub schema_version: u8,
    pub sequence: u64,
    pub from: String,
    pub to: String,
    pub series: Vec<TimeSeriesData>,
    pub markers: Vec<TimelineMarker>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimeSeriesRequest {
    pub context: OperationsContext,
    pub series_ids: Vec<String>,
    pub from: String,
    pub to: String,
    pub max_points: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ForecastState {
    Monitoring,
    Eligible,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForecastInputRequirement {
    pub series_id: String,
    pub role: String,
    pub required: bool,
    pub max_age_seconds: u64,
    pub source_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForecastBundle {
    pub schema_version: u8,
    pub bundle_version: u32,
    pub definition_sha256: String,
    pub id: String,
    pub label: String,
    pub thesis: String,
    pub horizon: String,
    pub state: ForecastState,
    pub strategy_ids: Vec<String>,
    pub series_ids: Vec<String>,
    pub shared_series_ids: Vec<String>,
    pub input_contract: Vec<ForecastInputRequirement>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandAction {
    SubmitOrder,
    PauseStrategy,
    ResumeStrategy,
    PauseBeforeStage,
    CancelOrder,
    FlattenPosition,
    ActivateKillSwitch,
}

impl CommandAction {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SubmitOrder => "submit_order",
            Self::PauseStrategy => "pause_strategy",
            Self::ResumeStrategy => "resume_strategy",
            Self::PauseBeforeStage => "pause_before_stage",
            Self::CancelOrder => "cancel_order",
            Self::FlattenPosition => "flatten_position",
            Self::ActivateKillSwitch => "activate_kill_switch",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrderRequest {
    pub instrument: String,
    pub side: Side,
    pub quantity_micros: String,
    pub order_type: OrderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price_micros: Option<String>,
    pub time_in_force: TimeInForce,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandRequest {
    pub schema_version: u8,
    pub action: CommandAction,
    pub target_id: String,
    pub reason: String,
    pub confirmation: String,
    pub expected_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<OrderRequest>,
}

impl CommandRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("command schemaVersion must be 1".into());
        }
        if self.reason.trim().chars().count() < 12 {
            return Err("command reason must contain at least 12 characters".into());
        }
        if self.target_id.trim().is_empty() || self.confirmation.trim().is_empty() {
            return Err("targetId and confirmation must be non-empty".into());
        }
        if matches!(self.action, CommandAction::SubmitOrder) != self.order.is_some() {
            return Err("submit_order requires order and other actions forbid it".into());
        }
        if let Some(order) = &self.order {
            let quantity = order
                .quantity_micros
                .parse::<u128>()
                .map_err(|_| "quantityMicros must be an exact positive integer".to_string())?;
            if quantity == 0 || order.instrument.trim().is_empty() {
                return Err("order instrument and positive quantity are required".into());
            }
            match (&order.order_type, &order.limit_price_micros) {
                (OrderType::Limit, Some(price)) if price.parse::<u128>().unwrap_or(0) > 0 => {}
                (OrderType::Market, None) => {}
                _ => {
                    return Err(
                        "limit orders require a positive price and market orders forbid one".into(),
                    )
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommandStatus {
    Accepted,
    Completed,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceipt {
    pub schema_version: u8,
    pub command_id: String,
    pub idempotency_key: String,
    pub action: CommandAction,
    pub target_id: String,
    pub status: CommandStatus,
    pub submitted_at: String,
    pub message: String,
    pub expected_sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandSession {
    pub schema_version: u8,
    pub operator: String,
    pub expires_at: String,
    pub csrf_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntityHistoryEvent {
    pub schema_version: u8,
    pub cursor: u64,
    pub event_id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub occurred_at: String,
    pub observed_at: String,
    pub actor: String,
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub schema_version: u8,
    pub events: Vec<EntityHistoryEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedWorkspace {
    pub schema_version: u8,
    pub workspace_id: String,
    pub owner: String,
    pub scope: String,
    pub name: String,
    pub revision: u64,
    pub updated_at: String,
    pub definition: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvestigationCitation {
    pub id: String,
    pub source_id: String,
    pub label: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvestigationRequest {
    pub schema_version: u8,
    pub context: OperationsContext,
    pub snapshot_sequence: u64,
    pub from: String,
    pub to: String,
    pub cursor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_id: Option<String>,
    pub series_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvestigationResult {
    pub schema_version: u8,
    pub investigation_id: String,
    pub generated_at: String,
    pub model_id: String,
    pub summary: String,
    pub limitation: String,
    pub suggested_series_ids: Vec<String>,
    pub citations: Vec<InvestigationCitation>,
}
