use crate::store::{CommandExecutionError, CommandExecutor, CommandOutcome, OperatorStore};
use crate::types::{
    ActivitySeverity, ActivityView, CommandAction, CommandRequest, CommandStatus, FillView,
    Liquidity, OrderState as ViewOrderState, OrderType, OrderView, PositionView,
    ReconciliationState, Side as ViewSide, TimeInForce as ViewTimeInForce,
};
use crate::{LocalRiskPort, PaperOmsPort, PaperRiskPort};
use async_trait::async_trait;
use helio_alpaca::{
    broker_decimal_to_micros, signed_decimal_to_micros, AlpacaBroker, AlpacaEnvironment,
    AlpacaOrderRequest, AlpacaOrderType, AlpacaPosition, AlpacaTimeInForce, AlpacaTradeUpdate,
    AlpacaTransport,
};
use helio_execution::{
    BrokerError, BrokerLifecyclePort, BrokerOrderSnapshot, BrokerOrderState, ExecutionMode,
    MoneyMicros, OrderProposal, PortfolioRiskSnapshot, PriceMicros, QuantityMicros, RiskContext,
    RiskDecision, RiskPolicy, Side,
};
use helio_oms::{OmsCommand, OrderSnapshot, OrderState, ReferenceOms, TimeInForce};
use helio_time::VenueSchedule;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketReference {
    pub symbol: String,
    pub price: PriceMicros,
    pub observed_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MarketReferenceError {
    #[error("market reference is unavailable")]
    Unavailable,
    #[error("market reference regressed")]
    Regression,
    #[error("market reference price is zero")]
    ZeroPrice,
    #[error("market reference lock was poisoned")]
    Poisoned,
}

pub trait MarketReferencePort: Send + Sync {
    fn latest(&self, symbol: &str) -> Result<MarketReference, MarketReferenceError>;
}

pub trait ExecutionClock: Send + Sync {
    fn now_ns(&self) -> Result<u64, PaperExecutorError>;
}

#[async_trait]
pub trait AlpacaTradeUpdatePort: Send + Sync {
    async fn reconcile_trade_update(
        &self,
        update: AlpacaTradeUpdate,
        store: Arc<OperatorStore>,
    ) -> Result<(), CommandExecutionError>;
}

#[derive(Debug, Default)]
pub struct SystemExecutionClock;

impl ExecutionClock for SystemExecutionClock {
    fn now_ns(&self) -> Result<u64, PaperExecutorError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PaperExecutorError::Clock)?
            .as_nanos();
        u64::try_from(nanos).map_err(|_| PaperExecutorError::Clock)
    }
}

#[derive(Debug, Default)]
pub struct InMemoryMarketReferencePort {
    references: RwLock<HashMap<String, MarketReference>>,
}

impl InMemoryMarketReferencePort {
    pub fn update(&self, reference: MarketReference) -> Result<(), MarketReferenceError> {
        if reference.price.0 == 0 {
            return Err(MarketReferenceError::ZeroPrice);
        }
        let mut references = self
            .references
            .write()
            .map_err(|_| MarketReferenceError::Poisoned)?;
        if references
            .get(&reference.symbol)
            .is_some_and(|current| reference.observed_at_ns < current.observed_at_ns)
        {
            return Err(MarketReferenceError::Regression);
        }
        references.insert(reference.symbol.clone(), reference);
        Ok(())
    }
}

impl MarketReferencePort for InMemoryMarketReferencePort {
    fn latest(&self, symbol: &str) -> Result<MarketReference, MarketReferenceError> {
        self.references
            .read()
            .map_err(|_| MarketReferenceError::Poisoned)?
            .get(symbol)
            .cloned()
            .ok_or(MarketReferenceError::Unavailable)
    }
}

#[derive(Debug, Error)]
pub enum PaperExecutorError {
    #[error("Alpaca paper executor requires the paper environment")]
    NotPaper,
    #[error("Alpaca venue and venue schedule differ")]
    VenueMismatch,
    #[error("paper risk policy must disable live execution and allow the configured venue")]
    InvalidRiskPolicy,
    #[error("venue schedule is invalid or does not cover initialization time")]
    InvalidSchedule,
    #[error("system clock is unavailable")]
    Clock,
    #[error("paper executor lock was poisoned")]
    Poisoned,
    #[error("market reference failed: {0}")]
    MarketReference(#[from] MarketReferenceError),
    #[error("broker failed: {0}")]
    Broker(#[from] BrokerError),
    #[error("OMS failed: {0}")]
    Oms(String),
    #[error("risk failed: {0}")]
    Risk(String),
    #[error("fixed-point conversion failed")]
    FixedPoint,
    #[error("account is not admitted for trading: {0}")]
    AccountBlocked(String),
    #[error("startup reconciliation did not establish complete broker and OMS agreement: {0}")]
    StartupReconciliation(String),
    #[error("unsupported command for the paper executor")]
    Unsupported,
}

struct PaperState<T> {
    broker: AlpacaBroker<T>,
    oms: Box<dyn PaperOmsPort>,
    risk: Box<dyn PaperRiskPort>,
    schedule: VenueSchedule,
    daily_order_count: u32,
    known_order_ids: BTreeSet<String>,
}

pub struct AlpacaPaperCommandExecutor<T> {
    state: Arc<Mutex<PaperState<T>>>,
    market: Arc<dyn MarketReferencePort>,
    clock: Arc<dyn ExecutionClock>,
}

pub struct PaperServicePorts {
    oms: Box<dyn PaperOmsPort>,
    risk: Box<dyn PaperRiskPort>,
}

impl PaperServicePorts {
    pub fn new(oms: Box<dyn PaperOmsPort>, risk: Box<dyn PaperRiskPort>) -> Self {
        Self { oms, risk }
    }
}

impl std::fmt::Debug for PaperServicePorts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PaperServicePorts")
            .field("oms_account_id", &self.oms.account_id())
            .field("risk_account_id", &self.risk.account_id())
            .finish_non_exhaustive()
    }
}

impl<T> std::fmt::Debug for AlpacaPaperCommandExecutor<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AlpacaPaperCommandExecutor")
            .finish_non_exhaustive()
    }
}

impl<T> AlpacaPaperCommandExecutor<T>
where
    T: AlpacaTransport,
{
    pub fn try_new(
        account_id: impl Into<String>,
        broker: AlpacaBroker<T>,
        risk_policy: RiskPolicy,
        schedule: VenueSchedule,
        market: Arc<dyn MarketReferencePort>,
        clock: Arc<dyn ExecutionClock>,
    ) -> Result<Self, PaperExecutorError> {
        let account_id = account_id.into();
        let portfolio =
            validate_paper_configuration(&broker, &risk_policy, &schedule, clock.as_ref())?;
        let oms = Box::new(
            ReferenceOms::try_new(account_id.clone())
                .map_err(|error| PaperExecutorError::Oms(error.to_string()))?,
        );
        let risk = Box::new(
            LocalRiskPort::new(account_id.clone(), risk_policy, portfolio, schedule.clone())
                .map_err(|error| PaperExecutorError::Risk(error.to_string()))?,
        );
        Self::from_validated_ports(account_id, broker, schedule, market, clock, oms, risk)
    }

    pub fn try_new_with_ports(
        account_id: impl Into<String>,
        broker: AlpacaBroker<T>,
        risk_policy: RiskPolicy,
        schedule: VenueSchedule,
        market: Arc<dyn MarketReferencePort>,
        clock: Arc<dyn ExecutionClock>,
        ports: PaperServicePorts,
    ) -> Result<Self, PaperExecutorError> {
        let account_id = account_id.into();
        validate_paper_configuration(&broker, &risk_policy, &schedule, clock.as_ref())?;
        let PaperServicePorts { oms, risk } = ports;
        if oms.account_id() != account_id || risk.account_id() != account_id {
            return Err(PaperExecutorError::Oms(
                "durable port account identity does not match the operator account".into(),
            ));
        }
        let risk_status = risk.status().map_err(PaperExecutorError::Risk)?;
        if risk_status.policy_version != risk_policy.version {
            return Err(PaperExecutorError::Risk(
                "durable risk policy version differs from the admitted policy".into(),
            ));
        }
        Self::from_validated_ports(account_id, broker, schedule, market, clock, oms, risk)
    }

    fn from_validated_ports(
        account_id: String,
        broker: AlpacaBroker<T>,
        schedule: VenueSchedule,
        market: Arc<dyn MarketReferencePort>,
        clock: Arc<dyn ExecutionClock>,
        oms: Box<dyn PaperOmsPort>,
        risk: Box<dyn PaperRiskPort>,
    ) -> Result<Self, PaperExecutorError> {
        if oms.account_id() != account_id || risk.account_id() != account_id {
            return Err(PaperExecutorError::Oms(
                "port account identity does not match the operator account".into(),
            ));
        }
        Ok(Self {
            state: Arc::new(Mutex::new(PaperState {
                broker,
                oms,
                risk,
                schedule,
                daily_order_count: 0,
                known_order_ids: BTreeSet::new(),
            })),
            market,
            clock,
        })
    }
}

impl<T> AlpacaPaperCommandExecutor<T>
where
    T: AlpacaTransport + Send + 'static,
{
    /// Reconciles every active broker liability before the command executor is exposed.
    /// Any broker-only or missing active order fails admission, leaving the gateway read-only.
    pub async fn startup_reconcile(
        &self,
        store: Arc<OperatorStore>,
    ) -> Result<(), CommandExecutionError> {
        const MAX_OMS_RECONCILIATION_ORDERS: usize = 10_000;

        let state = self.state.clone();
        let clock = self.clock.clone();
        let results = tokio::task::spawn_blocking(move || {
            let now_ns = clock.now_ns()?;
            let now_sec =
                i64::try_from(now_ns / 1_000_000_000).map_err(|_| PaperExecutorError::Clock)?;
            let mut state = state.lock().map_err(|_| PaperExecutorError::Poisoned)?;
            let trading_day = state
                .schedule
                .session_on_or_after(now_sec)
                .map_err(|_| PaperExecutorError::InvalidSchedule)?
                .ok_or(PaperExecutorError::InvalidSchedule)?
                .label
                .0;

            let durable_orders = state
                .oms
                .orders(MAX_OMS_RECONCILIATION_ORDERS)
                .map_err(PaperExecutorError::Oms)?;
            let durable_by_id = durable_orders
                .iter()
                .map(|order| (order.client_order_id.clone(), order))
                .collect::<BTreeMap<_, _>>();
            let broker_open = state.broker.open_orders_for_reconciliation()?;
            let broker_only = broker_open
                .iter()
                .filter(|order| {
                    !durable_by_id.contains_key(&order.acknowledgement.client_order_id)
                })
                .map(|order| order.acknowledgement.client_order_id.clone())
                .collect::<Vec<_>>();
            if !broker_only.is_empty() {
                return Err(PaperExecutorError::StartupReconciliation(format!(
                    "active Alpaca orders are absent from the durable OMS: {}",
                    broker_only.join(", ")
                )));
            }

            state.known_order_ids = durable_orders
                .iter()
                .map(|order| order.client_order_id.clone())
                .collect();
            state.daily_order_count = u32::try_from(
                durable_orders
                    .iter()
                    .filter(|order| order.intent.proposal.trading_day == trading_day)
                    .count(),
            )
            .map_err(|_| PaperExecutorError::FixedPoint)?;

            let mut covered = durable_orders
                .iter()
                .filter(|order| order.state.is_terminal())
                .map(|order| order.client_order_id.clone())
                .collect::<Vec<_>>();
            let mut reconciled = Vec::new();
            let mut missing = Vec::new();
            for order in durable_orders
                .iter()
                .filter(|order| !order.state.is_terminal())
            {
                let Some(broker) = state
                    .broker
                    .fetch_order_by_client_order_id(&order.client_order_id)?
                else {
                    state
                        .oms
                        .execute(OmsCommand::MarkUnknown {
                            command_id: format!(
                                "startup:{}:missing-broker-order",
                                order.client_order_id
                            ),
                            client_order_id: order.client_order_id.clone(),
                            reason: "Active durable OMS order was absent from Alpaca during startup reconciliation".into(),
                            at_ns: now_ns,
                        })
                        .map_err(PaperExecutorError::Oms)?;
                    missing.push(order.client_order_id.clone());
                    continue;
                };
                reconcile_broker_snapshot(&mut state, &order.client_order_id, &broker, now_ns)?;
                let current = state
                    .oms
                    .order(&order.client_order_id)
                    .map_err(PaperExecutorError::Oms)?
                    .ok_or_else(|| {
                        PaperExecutorError::Oms(
                            "order disappeared during startup reconciliation".into(),
                        )
                    })?;
                if current.state.is_terminal() {
                    covered.push(order.client_order_id.clone());
                }
                reconciled.push((order.client_order_id.clone(), broker));
            }
            if !missing.is_empty() {
                return Err(PaperExecutorError::StartupReconciliation(format!(
                    "active durable orders were absent from Alpaca: {}",
                    missing.join(", ")
                )));
            }

            covered.sort();
            covered.dedup();
            let (positions, portfolio) = broker_portfolio(&mut state, now_ns, trading_day)?;
            let risk = state
                .risk
                .refresh_portfolio(portfolio, &covered)
                .map_err(PaperExecutorError::Risk)?;
            let mut projected = vec![PaperExecutionResult {
                outcome: CommandOutcome {
                    status: CommandStatus::Completed,
                    message: "Startup reconciliation completed".into(),
                },
                order: None,
                fills: Vec::new(),
                positions: positions.clone(),
                gross_exposure_micros: risk.gross_exposure_micros,
                reserved_gross_micros: risk.reserved_gross_micros,
                pending_reconciliations: 0,
                daily_order_count: u64::from(state.daily_order_count),
                activity: None,
            }];
            for (client_order_id, broker) in reconciled {
                projected.push(project_result(
                    &mut state,
                    &client_order_id,
                    &broker,
                    positions.clone(),
                    "startup-reconciliation",
                    "Durable OMS state reconciled with Alpaca before command admission",
                )?);
            }
            Ok::<_, PaperExecutorError>(projected)
        })
        .await
        .map_err(|error| CommandExecutionError::Infrastructure(error.to_string()))?
        .map_err(map_executor_error)?;

        for result in &results {
            apply_execution_result(&store, result).await?;
        }
        Ok(())
    }
}

fn validate_paper_configuration<T: AlpacaTransport>(
    broker: &AlpacaBroker<T>,
    risk_policy: &RiskPolicy,
    schedule: &VenueSchedule,
    clock: &dyn ExecutionClock,
) -> Result<PortfolioRiskSnapshot, PaperExecutorError> {
    if broker.environment() != AlpacaEnvironment::Paper {
        return Err(PaperExecutorError::NotPaper);
    }
    schedule
        .validate()
        .map_err(|_| PaperExecutorError::InvalidSchedule)?;
    if broker.venue() != schedule.metadata.venue {
        return Err(PaperExecutorError::VenueMismatch);
    }
    if risk_policy.live_enabled
        || !risk_policy.allowed_venues.contains(broker.venue())
        || risk_policy.version.trim().is_empty()
    {
        return Err(PaperExecutorError::InvalidRiskPolicy);
    }
    let initialized_at_ns = clock.now_ns()?;
    let initialized_at_sec = i64::try_from(initialized_at_ns / 1_000_000_000)
        .map_err(|_| PaperExecutorError::InvalidSchedule)?;
    let trading_day = schedule
        .session_on_or_after(initialized_at_sec)
        .map_err(|_| PaperExecutorError::InvalidSchedule)?
        .ok_or(PaperExecutorError::InvalidSchedule)?
        .label
        .0;
    Ok(PortfolioRiskSnapshot::empty(initialized_at_ns, trading_day))
}

#[derive(Debug)]
struct PaperExecutionResult {
    outcome: CommandOutcome,
    order: Option<OrderView>,
    fills: Vec<FillView>,
    positions: Vec<PositionView>,
    gross_exposure_micros: u64,
    reserved_gross_micros: u64,
    pending_reconciliations: u64,
    daily_order_count: u64,
    activity: Option<ActivityView>,
}

#[async_trait]
impl<T> CommandExecutor for AlpacaPaperCommandExecutor<T>
where
    T: AlpacaTransport + Send + 'static,
{
    async fn execute(
        &self,
        actor: &str,
        command: &CommandRequest,
        store: &OperatorStore,
    ) -> Result<CommandOutcome, CommandExecutionError> {
        match command.action {
            CommandAction::SubmitOrder | CommandAction::CancelOrder => {
                let state = self.state.clone();
                let market = self.market.clone();
                let clock = self.clock.clone();
                let command = command.clone();
                let actor = actor.to_owned();
                let result = tokio::task::spawn_blocking(move || {
                    let mut state = state.lock().map_err(|_| PaperExecutorError::Poisoned)?;
                    let now_ns = clock.now_ns()?;
                    match command.action {
                        CommandAction::SubmitOrder => {
                            execute_submit(&mut state, market.as_ref(), &command, &actor, now_ns)
                        }
                        CommandAction::CancelOrder => {
                            execute_cancel(&mut state, &command, &actor, now_ns)
                        }
                        _ => Err(PaperExecutorError::Unsupported),
                    }
                })
                .await
                .map_err(|error| CommandExecutionError::Infrastructure(error.to_string()))?
                .map_err(map_executor_error)?;
                apply_execution_result(store, &result).await?;
                Ok(result.outcome)
            }
            CommandAction::ActivateKillSwitch => {
                let state = self.state.clone();
                tokio::task::spawn_blocking(move || {
                    state
                        .lock()
                        .map_err(|_| PaperExecutorError::Poisoned)?
                        .risk
                        .set_kill_switch(true)
                        .map_err(PaperExecutorError::Risk)
                })
                .await
                .map_err(|error| CommandExecutionError::Infrastructure(error.to_string()))?
                .map_err(map_executor_error)?;
                store
                    .mutate_snapshot(|snapshot| {
                        snapshot.risk.kill_switch_active = true;
                        snapshot.risk.capital_gate_reason = "Operator kill switch is active".into();
                        Ok(())
                    })
                    .await
                    .map_err(|error| CommandExecutionError::Infrastructure(error.to_string()))?;
                Ok(CommandOutcome {
                    status: CommandStatus::Completed,
                    message: "Kill switch activated".into(),
                })
            }
            CommandAction::PauseStrategy | CommandAction::ResumeStrategy => {
                let target = command.target_id.clone();
                let resume = command.action == CommandAction::ResumeStrategy;
                store
                    .mutate_snapshot(|snapshot| {
                        let strategy = snapshot
                            .strategies
                            .iter_mut()
                            .find(|strategy| strategy.id == target)
                            .ok_or_else(|| {
                                crate::store::StoreError::InvalidSnapshot(
                                    "strategy not found".into(),
                                )
                            })?;
                        strategy.state = if resume {
                            crate::types::StrategyState::Running
                        } else {
                            crate::types::StrategyState::Paused
                        };
                        Ok(())
                    })
                    .await
                    .map_err(|error| CommandExecutionError::Invalid(error.to_string()))?;
                Ok(CommandOutcome {
                    status: CommandStatus::Completed,
                    message: if resume {
                        "Strategy resumed".into()
                    } else {
                        "Strategy paused".into()
                    },
                })
            }
            CommandAction::PauseBeforeStage => {
                let target = command.target_id.clone();
                store
                    .mutate_snapshot(|snapshot| {
                        let stage = snapshot
                            .stages
                            .iter_mut()
                            .find(|stage| stage.id == target)
                            .ok_or_else(|| {
                                crate::store::StoreError::InvalidSnapshot("stage not found".into())
                            })?;
                        if !stage.can_pause_before {
                            return Err(crate::store::StoreError::InvalidSnapshot(
                                "stage cannot be paused at this boundary".into(),
                            ));
                        }
                        stage.state = crate::types::StageState::Paused;
                        Ok(())
                    })
                    .await
                    .map_err(|error| CommandExecutionError::Invalid(error.to_string()))?;
                Ok(CommandOutcome {
                    status: CommandStatus::Completed,
                    message: "Stage paused at its admitted boundary".into(),
                })
            }
            CommandAction::FlattenPosition => Ok(CommandOutcome {
                status: CommandStatus::Rejected,
                message: "Flatten requires a separately reviewed liquidation plan".into(),
            }),
        }
    }
}

#[async_trait]
impl<T> AlpacaTradeUpdatePort for AlpacaPaperCommandExecutor<T>
where
    T: AlpacaTransport + Send + 'static,
{
    async fn reconcile_trade_update(
        &self,
        update: AlpacaTradeUpdate,
        store: Arc<OperatorStore>,
    ) -> Result<(), CommandExecutionError> {
        let state = self.state.clone();
        let clock = self.clock.clone();
        let result = tokio::task::spawn_blocking(move || {
            let now_ns = clock.now_ns()?;
            let mut state = state.lock().map_err(|_| PaperExecutorError::Poisoned)?;
            if state
                .oms
                .order(&update.client_order_id)
                .map_err(|error| PaperExecutorError::Oms(error.to_string()))?
                .is_none()
            {
                return Ok(None);
            }
            let broker = state
                .broker
                .fetch_order_by_client_order_id(&update.client_order_id)?
                .ok_or_else(|| {
                    PaperExecutorError::Oms("trade update order was not found at broker".into())
                })?;
            reconcile_broker_snapshot(&mut state, &update.client_order_id, &broker, now_ns)?;
            let now_sec =
                i64::try_from(now_ns / 1_000_000_000).map_err(|_| PaperExecutorError::Clock)?;
            let trading_day = state
                .schedule
                .session_on_or_after(now_sec)
                .map_err(|_| PaperExecutorError::InvalidSchedule)?
                .ok_or(PaperExecutorError::InvalidSchedule)?
                .label
                .0;
            let (positions, portfolio) = broker_portfolio(&mut state, now_ns, trading_day)?;
            let covered = if broker.state.is_terminal() {
                vec![update.client_order_id.clone()]
            } else {
                Vec::new()
            };
            state
                .risk
                .refresh_portfolio(portfolio, &covered)
                .map_err(PaperExecutorError::Risk)?;
            project_result(
                &mut state,
                &update.client_order_id,
                &broker,
                positions,
                "alpaca-trade-updates",
                "OMS reconciled from Alpaca trade update",
            )
            .map(Some)
        })
        .await
        .map_err(|error| CommandExecutionError::Infrastructure(error.to_string()))?
        .map_err(map_executor_error)?;
        if let Some(result) = result {
            apply_execution_result(&store, &result).await?;
        }
        Ok(())
    }
}

fn execute_submit<T: AlpacaTransport>(
    state: &mut PaperState<T>,
    market: &dyn MarketReferencePort,
    command: &CommandRequest,
    actor: &str,
    now_ns: u64,
) -> Result<PaperExecutionResult, PaperExecutorError> {
    let order = command
        .order
        .as_ref()
        .ok_or(PaperExecutorError::Unsupported)?;
    let now_sec = i64::try_from(now_ns / 1_000_000_000).map_err(|_| PaperExecutorError::Clock)?;
    let reference = market.latest(&order.instrument)?;
    let trading_day = state
        .schedule
        .active_session_at(now_sec)
        .map_err(|_| PaperExecutorError::InvalidSchedule)?
        .ok_or_else(|| PaperExecutorError::Risk("venue session is closed".into()))?
        .label
        .0;

    let (positions, portfolio) = broker_portfolio(state, now_ns, trading_day)?;
    let gross_before_order = portfolio.gross_exposure.0;

    let risk_price = match order.order_type {
        OrderType::Limit => PriceMicros(parse_positive_u64(
            order.limit_price_micros.as_deref(),
            "limit price",
        )?),
        OrderType::Market => reference.price,
    };
    let quantity = QuantityMicros(parse_positive_u64(
        Some(&order.quantity_micros),
        "quantity",
    )?);
    let proposal = OrderProposal {
        proposal_id: command.target_id.clone(),
        strategy_id: order.strategy_id.clone().unwrap_or_else(|| "manual".into()),
        symbol: order.instrument.clone(),
        venue: state.schedule.metadata.venue.clone(),
        currency: "USD".into(),
        side: execution_side(&order.side),
        quantity,
        limit_price: risk_price,
        mode: ExecutionMode::Paper,
        trading_day,
    };
    let intent = match state
        .risk
        .authorize(
            proposal,
            RiskContext {
                now_ns,
                market_data_at_ns: reference.observed_at_ns,
                venue_time_utc_sec: now_sec,
            },
            portfolio,
        )
        .map_err(PaperExecutorError::Risk)?
    {
        RiskDecision::Approved(intent) => *intent,
        RiskDecision::Rejected(reason) => {
            return rejected_result(
                state,
                format!("Risk rejected order: {reason:?}"),
                positions,
                gross_before_order,
            );
        }
    };
    let tif = oms_time_in_force(&order.time_in_force);
    state
        .oms
        .execute(OmsCommand::Submit {
            command_id: format!("operator:{}:submit", command.target_id),
            intent: intent.clone(),
            time_in_force: tif,
            at_ns: now_ns,
        })
        .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;

    let existing = state
        .broker
        .fetch_order_by_client_order_id(&intent.client_order_id)?;
    let broker_order = match existing {
        Some(existing) => existing,
        None => {
            let request = AlpacaOrderRequest {
                client_order_id: intent.client_order_id.clone(),
                symbol: intent.proposal.symbol.clone(),
                side: intent.proposal.side,
                quantity: helio_execution::BrokerDecimal::try_new(micros_decimal(quantity.0))
                    .map_err(|_| PaperExecutorError::FixedPoint)?,
                order_type: match order.order_type {
                    OrderType::Market => AlpacaOrderType::Market,
                    OrderType::Limit => AlpacaOrderType::Limit,
                },
                time_in_force: alpaca_time_in_force(&order.time_in_force),
                limit_price: match order.order_type {
                    OrderType::Market => None,
                    OrderType::Limit => Some(
                        helio_execution::BrokerDecimal::try_new(micros_decimal(risk_price.0))
                            .map_err(|_| PaperExecutorError::FixedPoint)?,
                    ),
                },
                extended_hours: false,
            };
            match state.broker.submit_order(&request) {
                Ok(snapshot) => snapshot,
                Err(BrokerError::AmbiguousOutcome) => state
                    .broker
                    .fetch_order_by_client_order_id(&intent.client_order_id)?
                    .ok_or(BrokerError::AmbiguousOutcome)?,
                Err(BrokerError::Rejected(reason)) => {
                    state
                        .oms
                        .execute(OmsCommand::Reject {
                            command_id: format!("operator:{}:reject", command.target_id),
                            client_order_id: intent.client_order_id.clone(),
                            reason: reason.clone(),
                            at_ns: now_ns,
                        })
                        .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
                    let gross = portfolio_gross(state, now_ns, trading_day)?;
                    return rejected_result(
                        state,
                        format!("Broker rejected order: {reason}"),
                        positions,
                        gross,
                    );
                }
                Err(error) => return Err(error.into()),
            }
        }
    };
    apply_broker_snapshot(state, &intent.client_order_id, &broker_order, now_ns)?;
    let was_known = state.known_order_ids.contains(&intent.client_order_id);
    state.known_order_ids.insert(intent.client_order_id.clone());
    if !was_known {
        state.daily_order_count = state.daily_order_count.saturating_add(1);
    }
    let (positions, portfolio) = broker_portfolio(state, now_ns, trading_day)?;
    let covered = if broker_order.state.is_terminal() {
        vec![intent.client_order_id.clone()]
    } else {
        Vec::new()
    };
    state
        .risk
        .refresh_portfolio(portfolio, &covered)
        .map_err(PaperExecutorError::Risk)?;
    project_result(
        state,
        &intent.client_order_id,
        &broker_order,
        positions,
        actor,
        "Order admitted and submitted to Alpaca paper trading",
    )
}

fn execute_cancel<T: AlpacaTransport>(
    state: &mut PaperState<T>,
    command: &CommandRequest,
    actor: &str,
    now_ns: u64,
) -> Result<PaperExecutionResult, PaperExecutorError> {
    state
        .oms
        .execute(OmsCommand::RequestCancel {
            command_id: format!("operator:{}:cancel-request", command.target_id),
            client_order_id: command.target_id.clone(),
            at_ns: now_ns,
        })
        .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
    let broker_order = state.broker.cancel_by_client_order_id(&command.target_id)?;
    if broker_order.state == BrokerOrderState::Canceled {
        state
            .oms
            .execute(OmsCommand::ConfirmCanceled {
                command_id: format!("operator:{}:cancel-confirm", command.target_id),
                client_order_id: command.target_id.clone(),
                at_ns: now_ns,
            })
            .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
    }
    let now_sec = i64::try_from(now_ns / 1_000_000_000).map_err(|_| PaperExecutorError::Clock)?;
    let session = state
        .schedule
        .session_on_or_after(now_sec)
        .map_err(|_| PaperExecutorError::InvalidSchedule)?
        .ok_or(PaperExecutorError::InvalidSchedule)?;
    let (positions, _) = broker_portfolio(state, now_ns, session.label.0)?;
    project_result(
        state,
        &command.target_id,
        &broker_order,
        positions,
        actor,
        "Cancel request accepted by Alpaca paper trading",
    )
}

fn apply_broker_snapshot<T: AlpacaTransport>(
    state: &mut PaperState<T>,
    client_order_id: &str,
    broker: &BrokerOrderSnapshot,
    at_ns: u64,
) -> Result<(), PaperExecutorError> {
    let current = state
        .oms
        .order(client_order_id)
        .map_err(|error| PaperExecutorError::Oms(error.to_string()))?
        .ok_or_else(|| PaperExecutorError::Oms("submitted order disappeared".into()))?;
    if broker.state == BrokerOrderState::Failed && current.state == OrderState::PendingSubmit {
        state
            .oms
            .execute(OmsCommand::Reject {
                command_id: format!("broker:{client_order_id}:reject"),
                client_order_id: client_order_id.into(),
                reason: "Alpaca reported a terminal order failure".into(),
                at_ns,
            })
            .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
        return Ok(());
    }
    if current.broker_order_id.is_none() {
        state
            .oms
            .execute(OmsCommand::Acknowledge {
                command_id: format!("broker:{client_order_id}:ack"),
                client_order_id: client_order_id.into(),
                broker_order_id: broker.acknowledgement.broker_order_id.clone(),
                at_ns,
            })
            .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
    }
    for execution in &broker.executions {
        state
            .oms
            .execute(OmsCommand::RecordFill {
                command_id: format!("broker:{client_order_id}:fill:{}", execution.execution_id),
                client_order_id: client_order_id.into(),
                broker_order_id: Some(broker.acknowledgement.broker_order_id.clone()),
                execution_id: execution.execution_id.clone(),
                venue_occurred_at: Some(execution.occurred_at.clone()),
                quantity: QuantityMicros(
                    broker_decimal_to_micros(&execution.quantity)
                        .map_err(|_| PaperExecutorError::FixedPoint)?,
                ),
                price: PriceMicros(
                    broker_decimal_to_micros(&execution.effective_price)
                        .map_err(|_| PaperExecutorError::FixedPoint)?,
                ),
                at_ns,
            })
            .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
    }
    Ok(())
}

fn reconcile_broker_snapshot<T: AlpacaTransport>(
    state: &mut PaperState<T>,
    client_order_id: &str,
    broker: &BrokerOrderSnapshot,
    at_ns: u64,
) -> Result<(), PaperExecutorError> {
    apply_broker_snapshot(state, client_order_id, broker, at_ns)?;
    let current = state
        .oms
        .order(client_order_id)
        .map_err(|error| PaperExecutorError::Oms(error.to_string()))?
        .ok_or_else(|| PaperExecutorError::Oms("reconciled order disappeared".into()))?;
    match broker.state {
        BrokerOrderState::Canceled => {
            if matches!(
                current.state,
                OrderState::Working | OrderState::PartiallyFilled
            ) {
                state
                    .oms
                    .execute(OmsCommand::RequestCancel {
                        command_id: format!("broker:{client_order_id}:venue-cancel-request"),
                        client_order_id: client_order_id.into(),
                        at_ns,
                    })
                    .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
            }
            let current = state
                .oms
                .order(client_order_id)
                .map_err(|error| PaperExecutorError::Oms(error.to_string()))?
                .ok_or_else(|| PaperExecutorError::Oms("canceled order disappeared".into()))?;
            if current.state == OrderState::PendingCancel {
                state
                    .oms
                    .execute(OmsCommand::ConfirmCanceled {
                        command_id: format!("broker:{client_order_id}:venue-cancel-confirm"),
                        client_order_id: client_order_id.into(),
                        at_ns,
                    })
                    .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
            }
        }
        BrokerOrderState::Failed if !current.state.is_terminal() => {
            state
                .oms
                .execute(OmsCommand::MarkUnknown {
                    command_id: format!("broker:{client_order_id}:terminal-failure"),
                    client_order_id: client_order_id.into(),
                    reason: "Alpaca reported terminal failure after acknowledgement".into(),
                    at_ns,
                })
                .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
        }
        BrokerOrderState::Filled => {
            let broker_filled = broker_decimal_to_micros(&broker.filled_quantity)
                .map_err(|_| PaperExecutorError::FixedPoint)?;
            let current = state
                .oms
                .order(client_order_id)
                .map_err(|error| PaperExecutorError::Oms(error.to_string()))?
                .ok_or_else(|| PaperExecutorError::Oms("filled order disappeared".into()))?;
            if current.filled_quantity.0 != broker_filled && !current.state.is_terminal() {
                state
                    .oms
                    .execute(OmsCommand::MarkUnknown {
                        command_id: format!("broker:{client_order_id}:fill-mismatch"),
                        client_order_id: client_order_id.into(),
                        reason:
                            "Broker cumulative fill did not match reconciled execution activities"
                                .into(),
                        at_ns,
                    })
                    .map_err(|error| PaperExecutorError::Oms(error.to_string()))?;
            }
        }
        BrokerOrderState::Pending
        | BrokerOrderState::Open
        | BrokerOrderState::PartiallyFilled
        | BrokerOrderState::Failed => {}
    }
    Ok(())
}

fn project_result<T: AlpacaTransport>(
    state: &mut PaperState<T>,
    client_order_id: &str,
    broker: &BrokerOrderSnapshot,
    positions: Vec<PositionView>,
    actor: &str,
    message: &str,
) -> Result<PaperExecutionResult, PaperExecutorError> {
    let order = state
        .oms
        .order(client_order_id)
        .map_err(|error| PaperExecutorError::Oms(error.to_string()))?
        .ok_or_else(|| PaperExecutorError::Oms("order projection disappeared".into()))?;
    let order_view = order_view(&order, broker)?;
    let fills = fill_views(&order, broker)?;
    let gross = positions.iter().try_fold(0_u64, |total, position| {
        let value = position
            .market_value_micros
            .parse::<i128>()
            .map_err(|_| PaperExecutorError::FixedPoint)?
            .unsigned_abs();
        let value = u64::try_from(value).map_err(|_| PaperExecutorError::FixedPoint)?;
        total
            .checked_add(value)
            .ok_or(PaperExecutorError::FixedPoint)
    })?;
    let reserved = reserved_gross(state)?;
    Ok(PaperExecutionResult {
        outcome: CommandOutcome {
            status: CommandStatus::Accepted,
            message: message.into(),
        },
        order: Some(order_view),
        fills,
        positions,
        gross_exposure_micros: gross,
        reserved_gross_micros: reserved,
        pending_reconciliations: u64::from(!broker.state.is_terminal()),
        daily_order_count: u64::from(state.daily_order_count),
        activity: Some(ActivityView {
            id: format!("operator:{client_order_id}:{}", order.version),
            sequence: order.version,
            occurred_at: format_ns(order.last_update_at_ns)?,
            category: "order".into(),
            source: actor.into(),
            stage: "oms".into(),
            entity: client_order_id.into(),
            outcome: format!("{:?}", order.state).to_lowercase(),
            severity: if order.state == OrderState::Unknown {
                ActivitySeverity::Warning
            } else {
                ActivitySeverity::Normal
            },
        }),
    })
}

fn rejected_result<T>(
    state: &PaperState<T>,
    message: String,
    positions: Vec<PositionView>,
    gross_exposure_micros: u64,
) -> Result<PaperExecutionResult, PaperExecutorError> {
    Ok(PaperExecutionResult {
        outcome: CommandOutcome {
            status: CommandStatus::Rejected,
            message,
        },
        order: None,
        fills: Vec::new(),
        positions,
        gross_exposure_micros,
        reserved_gross_micros: reserved_gross(state)?,
        pending_reconciliations: 0,
        daily_order_count: u64::from(state.daily_order_count),
        activity: None,
    })
}

fn reserved_gross<T>(state: &PaperState<T>) -> Result<u64, PaperExecutorError> {
    state
        .risk
        .status()
        .map(|status| status.reserved_gross_micros)
        .map_err(PaperExecutorError::Risk)
}

async fn apply_execution_result(
    store: &OperatorStore,
    result: &PaperExecutionResult,
) -> Result<(), CommandExecutionError> {
    store
        .mutate_snapshot(|snapshot| {
            if let Some(order) = &result.order {
                upsert(&mut snapshot.orders, order.clone(), |value| {
                    value.client_order_id.clone()
                });
            }
            for fill in &result.fills {
                upsert(&mut snapshot.fills, fill.clone(), |value| {
                    value.execution_id.clone()
                });
            }
            snapshot.positions = result.positions.clone();
            snapshot.risk.gross_exposure_micros = result.gross_exposure_micros.to_string();
            snapshot.risk.reserved_gross_micros = result.reserved_gross_micros.to_string();
            snapshot.risk.pending_reconciliations = result.pending_reconciliations;
            snapshot.risk.daily_order_count = result.daily_order_count;
            if let Some(activity) = &result.activity {
                snapshot.activity.insert(0, activity.clone());
                snapshot.activity.truncate(1_000);
            }
            Ok(())
        })
        .await
        .map(|_| ())
        .map_err(|error| CommandExecutionError::Infrastructure(error.to_string()))
}

fn upsert<T, F>(values: &mut Vec<T>, value: T, identity: F)
where
    F: Fn(&T) -> String,
{
    let id = identity(&value);
    if let Some(index) = values.iter().position(|current| identity(current) == id) {
        values[index] = value;
    } else {
        values.insert(0, value);
    }
}

fn broker_portfolio<T: AlpacaTransport>(
    state: &mut PaperState<T>,
    now_ns: u64,
    trading_day: i32,
) -> Result<(Vec<PositionView>, PortfolioRiskSnapshot), PaperExecutorError> {
    let account = state.broker.account()?;
    if account.trading_blocked
        || account.account_blocked
        || account.trade_suspended_by_user
        || account.status != "ACTIVE"
    {
        return Err(PaperExecutorError::AccountBlocked(account.status));
    }
    let positions = state.broker.positions()?;
    let mut symbol_positions_micros = BTreeMap::new();
    let mut gross_exposure = 0_u64;
    for position in &positions {
        symbol_positions_micros.insert(
            position.symbol.clone(),
            signed_decimal_to_micros(&position.quantity)
                .map_err(|_| PaperExecutorError::FixedPoint)?,
        );
        let market_value = signed_decimal_to_micros(&position.market_value)
            .map_err(|_| PaperExecutorError::FixedPoint)?
            .unsigned_abs();
        gross_exposure = gross_exposure
            .checked_add(u64::try_from(market_value).map_err(|_| PaperExecutorError::FixedPoint)?)
            .ok_or(PaperExecutorError::FixedPoint)?;
    }
    let views = positions
        .into_iter()
        .map(position_view)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        views,
        PortfolioRiskSnapshot {
            as_of_ns: now_ns,
            trading_day,
            gross_exposure: MoneyMicros(gross_exposure),
            strategy_exposure: BTreeMap::new(),
            symbol_positions_micros,
            daily_order_count: state.daily_order_count,
        },
    ))
}

fn portfolio_gross<T: AlpacaTransport>(
    state: &mut PaperState<T>,
    now_ns: u64,
    trading_day: i32,
) -> Result<u64, PaperExecutorError> {
    broker_portfolio(state, now_ns, trading_day).map(|(_, portfolio)| portfolio.gross_exposure.0)
}

fn position_view(position: AlpacaPosition) -> Result<PositionView, PaperExecutorError> {
    let quantity =
        signed_decimal_to_micros(&position.quantity).map_err(|_| PaperExecutorError::FixedPoint)?;
    let average = signed_decimal_to_micros(&position.average_entry_price)
        .map_err(|_| PaperExecutorError::FixedPoint)?;
    let mark = signed_decimal_to_micros(&position.current_price)
        .map_err(|_| PaperExecutorError::FixedPoint)?;
    let market_value = signed_decimal_to_micros(&position.market_value)
        .map_err(|_| PaperExecutorError::FixedPoint)?;
    let unrealized = signed_decimal_to_micros(&position.unrealized_pl)
        .map_err(|_| PaperExecutorError::FixedPoint)?;
    Ok(PositionView {
        instrument: position.symbol,
        strategy: "account".into(),
        quantity_micros: quantity.to_string(),
        average_price_micros: average.to_string(),
        mark_price_micros: mark.to_string(),
        market_value_micros: market_value.to_string(),
        unrealized_pnl_micros: unrealized.to_string(),
        day_pnl_micros: None,
        day_change_bps: None,
        currency: "USD".into(),
        freshness_ms: 0,
    })
}

fn order_view(
    order: &OrderSnapshot,
    broker: &BrokerOrderSnapshot,
) -> Result<OrderView, PaperExecutorError> {
    Ok(OrderView {
        client_order_id: order.client_order_id.clone(),
        broker_order_id: order.broker_order_id.clone(),
        instrument: order.intent.proposal.symbol.clone(),
        side: view_side(order.intent.proposal.side),
        state: view_order_state(order.state),
        quantity_micros: order.working_quantity.0.to_string(),
        filled_quantity_micros: order.filled_quantity.0.to_string(),
        limit_price_micros: order.working_limit_price.0.to_string(),
        average_price_micros: order.average_fill_price.map(|price| price.0.to_string()),
        venue: order.intent.proposal.venue.clone(),
        strategy: order.intent.proposal.strategy_id.clone(),
        submitted_at: format_ns(broker.acknowledgement.accepted_at_ns)?,
        reconciliation: if broker_and_oms_match(broker.state, order.state) {
            ReconciliationState::Matched
        } else {
            ReconciliationState::Pending
        },
        oms_version: Some(order.version),
        time_in_force: Some(view_time_in_force(order.time_in_force)),
        uncertainty_reason: order.uncertainty_reason.clone(),
    })
}

fn broker_and_oms_match(broker: BrokerOrderState, oms: OrderState) -> bool {
    matches!(
        (broker, oms),
        (BrokerOrderState::Pending, OrderState::PendingSubmit)
            | (BrokerOrderState::Open, OrderState::Working)
            | (
                BrokerOrderState::PartiallyFilled,
                OrderState::PartiallyFilled
            )
            | (BrokerOrderState::Filled, OrderState::Filled)
            | (BrokerOrderState::Canceled, OrderState::Canceled)
            | (BrokerOrderState::Failed, OrderState::Rejected)
    )
}

fn fill_views(
    order: &OrderSnapshot,
    broker: &BrokerOrderSnapshot,
) -> Result<Vec<FillView>, PaperExecutorError> {
    broker
        .executions
        .iter()
        .map(|execution| {
            Ok(FillView {
                execution_id: execution.execution_id.clone(),
                client_order_id: order.client_order_id.clone(),
                instrument: order.intent.proposal.symbol.clone(),
                side: view_side(order.intent.proposal.side),
                quantity_micros: broker_decimal_to_micros(&execution.quantity)
                    .map_err(|_| PaperExecutorError::FixedPoint)?
                    .to_string(),
                price_micros: broker_decimal_to_micros(&execution.effective_price)
                    .map_err(|_| PaperExecutorError::FixedPoint)?
                    .to_string(),
                venue: order.intent.proposal.venue.clone(),
                strategy: order.intent.proposal.strategy_id.clone(),
                executed_at: execution.occurred_at.clone(),
                liquidity: Liquidity::Unknown,
            })
        })
        .collect()
}

fn execution_side(side: &ViewSide) -> Side {
    match side {
        ViewSide::Buy => Side::Buy,
        ViewSide::Sell => Side::Sell,
    }
}

fn view_side(side: Side) -> ViewSide {
    match side {
        Side::Buy => ViewSide::Buy,
        Side::Sell => ViewSide::Sell,
    }
}

fn oms_time_in_force(value: &ViewTimeInForce) -> TimeInForce {
    match value {
        ViewTimeInForce::Day => TimeInForce::Day,
        ViewTimeInForce::GoodTillCanceled => TimeInForce::GoodTillCanceled,
        ViewTimeInForce::ImmediateOrCancel => TimeInForce::ImmediateOrCancel,
        ViewTimeInForce::FillOrKill => TimeInForce::FillOrKill,
    }
}

fn view_time_in_force(value: TimeInForce) -> ViewTimeInForce {
    match value {
        TimeInForce::Day => ViewTimeInForce::Day,
        TimeInForce::GoodTillCanceled => ViewTimeInForce::GoodTillCanceled,
        TimeInForce::ImmediateOrCancel => ViewTimeInForce::ImmediateOrCancel,
        TimeInForce::FillOrKill => ViewTimeInForce::FillOrKill,
    }
}

fn alpaca_time_in_force(value: &ViewTimeInForce) -> AlpacaTimeInForce {
    match value {
        ViewTimeInForce::Day => AlpacaTimeInForce::Day,
        ViewTimeInForce::GoodTillCanceled => AlpacaTimeInForce::GoodTilCanceled,
        ViewTimeInForce::ImmediateOrCancel => AlpacaTimeInForce::ImmediateOrCancel,
        ViewTimeInForce::FillOrKill => AlpacaTimeInForce::FillOrKill,
    }
}

fn view_order_state(state: OrderState) -> ViewOrderState {
    match state {
        OrderState::PendingSubmit => ViewOrderState::PendingSubmit,
        OrderState::Working => ViewOrderState::Working,
        OrderState::PartiallyFilled => ViewOrderState::PartiallyFilled,
        OrderState::PendingCancel => ViewOrderState::PendingCancel,
        OrderState::PendingReplace => ViewOrderState::PendingReplace,
        OrderState::Filled => ViewOrderState::Filled,
        OrderState::Canceled => ViewOrderState::Canceled,
        OrderState::Rejected => ViewOrderState::Rejected,
        OrderState::Expired => ViewOrderState::Expired,
        OrderState::Unknown => ViewOrderState::Unknown,
    }
}

fn parse_positive_u64(value: Option<&str>, name: &str) -> Result<u64, PaperExecutorError> {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            PaperExecutorError::Risk(format!("{name} must be a positive fixed-point integer"))
        })
}

fn micros_decimal(value: u64) -> String {
    let whole = value / 1_000_000;
    let remainder = value % 1_000_000;
    if remainder == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{remainder:06}")
            .trim_end_matches('0')
            .to_owned()
    }
}

fn format_ns(value: u64) -> Result<String, PaperExecutorError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(value))
        .map_err(|_| PaperExecutorError::Clock)?
        .format(&Rfc3339)
        .map_err(|_| PaperExecutorError::Clock)
}

fn map_executor_error(error: PaperExecutorError) -> CommandExecutionError {
    match error {
        PaperExecutorError::MarketReference(_)
        | PaperExecutorError::Risk(_)
        | PaperExecutorError::AccountBlocked(_)
        | PaperExecutorError::Unsupported => CommandExecutionError::Invalid(error.to_string()),
        _ => CommandExecutionError::Infrastructure(error.to_string()),
    }
}

#[cfg(test)]
mod tests;
