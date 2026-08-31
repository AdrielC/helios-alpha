use crate::{PaperOmsPort, PaperRiskPort, RiskPortStatus};
use golem_client::bridge::GolemServer;
use helio_execution::{
    ExecutionMode, MoneyMicros, OrderIntent, OrderProposal, PortfolioRiskSnapshot, PriceMicros,
    QuantityMicros, RiskContext, RiskDecision, RiskPolicy, Side,
};
use helio_oms::{
    CommandReceipt, OmsCommand, OmsEventEnvelope, OrderSnapshot, OrderState, ReconciledState,
    TimeInForce,
};
use helio_time::VenueSchedule;
use oms_account_agent_client as oms_wire;
use risk_account_agent_client as risk_wire;
use std::fmt;
use std::sync::OnceLock;
use thiserror::Error;
use tokio::runtime::Handle;

const MAX_OMS_ORDER_BATCH: usize = 10_000;
const MAX_OMS_EVENT_BATCH: usize = 1_024;

#[derive(Clone, PartialEq, Eq)]
pub enum GolemEndpoint {
    Local,
    Cloud { token: String },
    Custom { url: String, token: String },
}

impl fmt::Debug for GolemEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => formatter.write_str("Local"),
            Self::Cloud { .. } => formatter.write_str("Cloud { token: [redacted] }"),
            Self::Custom { url, .. } => formatter
                .debug_struct("Custom")
                .field("url", url)
                .field("token", &"[redacted]")
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GolemClientSettings {
    pub endpoint: GolemEndpoint,
    pub app_name: String,
    pub environment_name: String,
}

impl GolemClientSettings {
    pub fn local(app_name: impl Into<String>, environment_name: impl Into<String>) -> Self {
        Self {
            endpoint: GolemEndpoint::Local,
            app_name: app_name.into(),
            environment_name: environment_name.into(),
        }
    }

    fn validate(&self) -> Result<(), GolemPortError> {
        if self.app_name.trim().is_empty() || self.environment_name.trim().is_empty() {
            return Err(GolemPortError::Configuration(
                "Golem app and environment names must not be empty".into(),
            ));
        }
        match &self.endpoint {
            GolemEndpoint::Local => Ok(()),
            GolemEndpoint::Cloud { token } if !token.trim().is_empty() => Ok(()),
            GolemEndpoint::Custom { url, token }
                if !url.trim().is_empty() && !token.trim().is_empty() =>
            {
                reqwest::Url::parse(url)
                    .map(|_| ())
                    .map_err(|_| GolemPortError::Configuration("invalid Golem custom URL".into()))
            }
            _ => Err(GolemPortError::Configuration(
                "Golem credentials must not be empty".into(),
            )),
        }
    }

    fn server(&self) -> Result<GolemServer, GolemPortError> {
        match &self.endpoint {
            GolemEndpoint::Local => Ok(GolemServer::Local),
            GolemEndpoint::Cloud { token } => Ok(GolemServer::Cloud {
                token: token.clone(),
            }),
            GolemEndpoint::Custom { url, token } => Ok(GolemServer::Custom {
                url: reqwest::Url::parse(url).map_err(|_| {
                    GolemPortError::Configuration("invalid Golem custom URL".into())
                })?,
                token: token.clone(),
            }),
        }
    }

    fn fingerprint(&self) -> String {
        match &self.endpoint {
            GolemEndpoint::Local => format!(
                "local\u{1f}{}\u{1f}{}",
                self.app_name, self.environment_name
            ),
            GolemEndpoint::Cloud { .. } => format!(
                "cloud\u{1f}{}\u{1f}{}",
                self.app_name, self.environment_name
            ),
            GolemEndpoint::Custom { url, .. } => format!(
                "custom:{url}\u{1f}{}\u{1f}{}",
                self.app_name, self.environment_name
            ),
        }
    }
}

#[derive(Debug, Error)]
pub enum GolemPortError {
    #[error("Golem client configuration failed: {0}")]
    Configuration(String),
    #[error("Golem bridge invocation failed: {0}")]
    Invocation(String),
    #[error("Golem agent rejected the operation: {0}")]
    Agent(String),
    #[error("Golem bridge returned invalid domain data: {0}")]
    InvalidDomain(String),
}

static CLIENT_CONFIGURATION: OnceLock<String> = OnceLock::new();

/// Connects both durable account agents and replays their configuration before command admission.
/// A configuration conflict or unavailable agent returns an error, so the caller can keep the
/// command plane read-only instead of silently falling back to process-local state.
pub async fn connect_golem_paper_ports(
    settings: &GolemClientSettings,
    account_id: &str,
    policy: &RiskPolicy,
    schedule: &VenueSchedule,
    initial_portfolio: &PortfolioRiskSnapshot,
) -> Result<(Box<dyn PaperOmsPort>, Box<dyn PaperRiskPort>), GolemPortError> {
    settings.validate()?;
    if account_id.trim().is_empty() {
        return Err(GolemPortError::Configuration(
            "Golem account identity must not be empty".into(),
        ));
    }
    configure_generated_clients(settings)?;
    let oms = oms_wire::OmsAccountAgent::get(account_id.to_owned())
        .await
        .map_err(invocation_error)?;
    let risk = risk_wire::RiskAccountAgent::get(account_id.to_owned())
        .await
        .map_err(invocation_error)?;
    let status = risk
        .configure(risk_wire::ConfigureRiskInput {
            risk_policy_json: serde_json::to_string(policy)
                .map_err(|error| GolemPortError::InvalidDomain(error.to_string()))?,
            venue_schedule_json: serde_json::to_string(schedule)
                .map_err(|error| GolemPortError::InvalidDomain(error.to_string()))?,
            initial_portfolio: risk_portfolio(initial_portfolio)?,
        })
        .await
        .map_err(invocation_error)?
        .map_err(risk_agent_error)?;
    let status = risk_status(status);
    if status.account_id != account_id || status.policy_version != policy.version {
        return Err(GolemPortError::InvalidDomain(
            "durable risk identity or policy version does not match configuration".into(),
        ));
    }
    let runtime = Handle::current();
    Ok((
        Box::new(GolemOmsPort {
            account_id: account_id.to_owned(),
            agent: oms,
            runtime: runtime.clone(),
        }),
        Box::new(GolemRiskPort {
            account_id: account_id.to_owned(),
            agent: risk,
            runtime,
            last_status: status,
        }),
    ))
}

fn configure_generated_clients(settings: &GolemClientSettings) -> Result<(), GolemPortError> {
    let fingerprint = settings.fingerprint();
    if let Some(existing) = CLIENT_CONFIGURATION.get() {
        return if existing == &fingerprint {
            Ok(())
        } else {
            Err(GolemPortError::Configuration(
                "Golem clients were already configured for another endpoint".into(),
            ))
        };
    }
    oms_wire::configure(
        settings.server()?,
        &settings.app_name,
        &settings.environment_name,
    );
    risk_wire::configure(
        settings.server()?,
        &settings.app_name,
        &settings.environment_name,
    );
    CLIENT_CONFIGURATION
        .set(fingerprint)
        .map_err(|_| GolemPortError::Configuration("Golem client configuration raced".into()))
}

struct GolemOmsPort {
    account_id: String,
    agent: oms_wire::OmsAccountAgent,
    runtime: Handle,
}

impl PaperOmsPort for GolemOmsPort {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn execute(&mut self, command: OmsCommand) -> Result<CommandReceipt, String> {
        let result = match command {
            OmsCommand::Submit {
                command_id,
                intent,
                time_in_force,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.submit(oms_wire::SubmitOrderInput {
                    command_id,
                    intent: oms_intent(intent),
                    time_in_force: oms_time_in_force(time_in_force),
                    at_ns,
                })),
            OmsCommand::Acknowledge {
                command_id,
                client_order_id,
                broker_order_id,
                at_ns,
            } => {
                self.runtime
                    .block_on(self.agent.acknowledge(oms_wire::VenueAcknowledgementInput {
                        command_id,
                        client_order_id,
                        broker_order_id,
                        at_ns,
                    }))
            }
            OmsCommand::Reject {
                command_id,
                client_order_id,
                reason,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.reject(oms_wire::OrderReasonInput {
                    command_id,
                    client_order_id,
                    reason,
                    at_ns,
                })),
            OmsCommand::RecordFill {
                command_id,
                client_order_id,
                broker_order_id,
                execution_id,
                venue_occurred_at,
                quantity,
                price,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.record_fill(oms_wire::FillInput {
                    command_id,
                    client_order_id,
                    broker_order_id,
                    execution_id,
                    venue_occurred_at,
                    quantity_micros: quantity.0,
                    price_micros: price.0,
                    at_ns,
                })),
            OmsCommand::RequestCancel {
                command_id,
                client_order_id,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.request_cancel(oms_wire::OrderActionInput {
                    command_id,
                    client_order_id,
                    at_ns,
                })),
            OmsCommand::ConfirmCanceled {
                command_id,
                client_order_id,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.confirm_canceled(oms_wire::OrderActionInput {
                    command_id,
                    client_order_id,
                    at_ns,
                })),
            OmsCommand::RequestReplace {
                command_id,
                client_order_id,
                new_quantity,
                new_limit_price,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.request_replace(oms_wire::ReplaceOrderInput {
                    command_id,
                    client_order_id,
                    new_quantity_micros: new_quantity.0,
                    new_limit_price_micros: new_limit_price.0,
                    at_ns,
                })),
            OmsCommand::ConfirmReplaced {
                command_id,
                client_order_id,
                broker_order_id,
                at_ns,
            } => {
                self.runtime
                    .block_on(self.agent.confirm_replaced(oms_wire::ConfirmReplaceInput {
                        command_id,
                        client_order_id,
                        broker_order_id,
                        at_ns,
                    }))
            }
            OmsCommand::RejectPendingAction {
                command_id,
                client_order_id,
                reason,
                at_ns,
            } => self.runtime.block_on(self.agent.reject_pending_action(
                oms_wire::RejectPendingActionInput {
                    command_id,
                    client_order_id,
                    reason,
                    at_ns,
                },
            )),
            OmsCommand::MarkExpired {
                command_id,
                client_order_id,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.mark_expired(oms_wire::OrderActionInput {
                    command_id,
                    client_order_id,
                    at_ns,
                })),
            OmsCommand::MarkUnknown {
                command_id,
                client_order_id,
                reason,
                at_ns,
            } => self
                .runtime
                .block_on(self.agent.mark_unknown(oms_wire::OrderReasonInput {
                    command_id,
                    client_order_id,
                    reason,
                    at_ns,
                })),
            OmsCommand::ReconcileUnknown {
                command_id,
                client_order_id,
                broker_order_id,
                state,
                at_ns,
            } => self.runtime.block_on(self.agent.reconcile_unknown(
                oms_wire::ReconcileUnknownInput {
                    command_id,
                    client_order_id,
                    broker_order_id,
                    state: oms_reconciled_state(state),
                    at_ns,
                },
            )),
        };
        result
            .map_err(|error| invocation_error(error).to_string())?
            .map(command_receipt)
            .map_err(|error| oms_agent_error(error).to_string())
    }

    fn order(&self, client_order_id: &str) -> Result<Option<OrderSnapshot>, String> {
        self.runtime
            .block_on(self.agent.order(client_order_id.to_owned()))
            .map_err(|error| invocation_error(error).to_string())?
            .map_err(|error| oms_agent_error(error).to_string())?
            .map(order_snapshot)
            .transpose()
            .map_err(|error| error.to_string())
    }

    fn orders(&self, limit: usize) -> Result<Vec<OrderSnapshot>, String> {
        if limit == 0 || limit > MAX_OMS_ORDER_BATCH {
            return Err(format!(
                "OMS order query limit must be 1..={MAX_OMS_ORDER_BATCH}"
            ));
        }
        let limit =
            u32::try_from(limit).map_err(|_| "OMS order query limit overflow".to_owned())?;
        self.runtime
            .block_on(self.agent.orders(limit))
            .map_err(|error| invocation_error(error).to_string())?
            .map_err(|error| oms_agent_error(error).to_string())?
            .into_iter()
            .map(order_snapshot)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<OmsEventEnvelope>, String> {
        if limit == 0 || limit > MAX_OMS_EVENT_BATCH {
            return Err(format!(
                "OMS event query limit must be 1..={MAX_OMS_EVENT_BATCH}"
            ));
        }
        let limit =
            u32::try_from(limit).map_err(|_| "OMS event query limit overflow".to_owned())?;
        let batch = self
            .runtime
            .block_on(self.agent.events_after(cursor, limit))
            .map_err(|error| invocation_error(error).to_string())?
            .map_err(|error| oms_agent_error(error).to_string())?;
        batch
            .events_json
            .into_iter()
            .map(|event| serde_json::from_str(&event).map_err(|error| error.to_string()))
            .collect()
    }
}

struct GolemRiskPort {
    account_id: String,
    agent: risk_wire::RiskAccountAgent,
    runtime: Handle,
    last_status: RiskPortStatus,
}

impl GolemRiskPort {
    fn fetch_status(&self) -> Result<RiskPortStatus, String> {
        self.runtime
            .block_on(self.agent.status())
            .map_err(|error| invocation_error(error).to_string())?
            .map(risk_status)
            .map_err(|error| risk_agent_error(error).to_string())
    }
}

impl PaperRiskPort for GolemRiskPort {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn authorize(
        &mut self,
        proposal: OrderProposal,
        context: RiskContext,
        portfolio: PortfolioRiskSnapshot,
    ) -> Result<RiskDecision, String> {
        let output = self
            .runtime
            .block_on(self.agent.authorize(risk_wire::AuthorizeRiskInput {
                proposal: risk_proposal(proposal),
                context: risk_wire::RiskContextInput {
                    now_ns: context.now_ns,
                    market_data_at_ns: context.market_data_at_ns,
                    venue_time_utc_sec: context.venue_time_utc_sec,
                },
                portfolio: risk_portfolio(&portfolio).map_err(|error| error.to_string())?,
            }))
            .map_err(|error| invocation_error(error).to_string())?
            .map_err(|error| risk_agent_error(error).to_string())?;
        self.last_status = self.fetch_status()?;
        match output {
            risk_wire::RiskDecisionOutput::Approved(approved) => {
                risk_intent(approved.intent).map(|intent| RiskDecision::Approved(Box::new(intent)))
            }
            risk_wire::RiskDecisionOutput::Rejected(rejected) => {
                Ok(RiskDecision::Rejected(risk_rejection(&rejected.reason)?))
            }
        }
    }

    fn refresh_portfolio(
        &mut self,
        portfolio: PortfolioRiskSnapshot,
        covered_client_order_ids: &[String],
    ) -> Result<RiskPortStatus, String> {
        let status = self
            .runtime
            .block_on(
                self.agent
                    .refresh_portfolio(risk_wire::RefreshPortfolioInput {
                        portfolio: risk_portfolio(&portfolio).map_err(|error| error.to_string())?,
                        covered_client_order_ids: covered_client_order_ids.to_vec(),
                    }),
            )
            .map_err(|error| invocation_error(error).to_string())?
            .map(risk_status)
            .map_err(|error| risk_agent_error(error).to_string())?;
        self.last_status = status.clone();
        Ok(status)
    }

    fn set_kill_switch(&mut self, active: bool) -> Result<RiskPortStatus, String> {
        let status = self
            .runtime
            .block_on(self.agent.set_kill_switch(active))
            .map_err(|error| invocation_error(error).to_string())?
            .map(risk_status)
            .map_err(|error| risk_agent_error(error).to_string())?;
        self.last_status = status.clone();
        Ok(status)
    }

    fn status(&self) -> Result<RiskPortStatus, String> {
        Ok(self.last_status.clone())
    }
}

fn command_receipt(output: oms_wire::CommandReceiptOutput) -> CommandReceipt {
    CommandReceipt {
        command_id: output.command_id,
        client_order_id: output.client_order_id,
        version: output.version,
        replayed: output.replayed,
        event_count: output.event_count,
    }
}

fn oms_intent(intent: OrderIntent) -> oms_wire::OrderIntentInput {
    oms_wire::OrderIntentInput {
        client_order_id: intent.client_order_id,
        proposal_id: intent.proposal.proposal_id,
        strategy_id: intent.proposal.strategy_id,
        symbol: intent.proposal.symbol,
        venue: intent.proposal.venue,
        currency: intent.proposal.currency,
        side: oms_side(intent.proposal.side),
        quantity_micros: intent.proposal.quantity.0,
        limit_price_micros: intent.proposal.limit_price.0,
        execution_mode: oms_execution_mode(intent.proposal.mode),
        trading_day: intent.proposal.trading_day,
        authorized_notional_micros: intent.authorized_notional.0,
        risk_policy_version: intent.risk_policy_version,
        authorized_at_ns: intent.authorized_at_ns,
    }
}

fn order_snapshot(view: oms_wire::OrderView) -> Result<OrderSnapshot, GolemPortError> {
    Ok(OrderSnapshot {
        client_order_id: view.client_order_id,
        broker_order_id: view.broker_order_id,
        state: order_state(view.state),
        intent: oms_domain_intent(view.intent),
        time_in_force: domain_time_in_force(view.time_in_force),
        working_quantity: QuantityMicros(view.working_quantity_micros),
        working_limit_price: PriceMicros(view.working_limit_price_micros),
        filled_quantity: QuantityMicros(view.filled_quantity_micros),
        average_fill_price: view.average_fill_price_micros.map(PriceMicros),
        filled_notional: MoneyMicros(view.filled_notional_micros),
        version: view.version,
        last_update_at_ns: view.last_update_at_ns,
        uncertainty_reason: view.uncertainty_reason,
    })
}

fn oms_domain_intent(intent: oms_wire::OrderIntentInput) -> OrderIntent {
    OrderIntent {
        client_order_id: intent.client_order_id,
        proposal: OrderProposal {
            proposal_id: intent.proposal_id,
            strategy_id: intent.strategy_id,
            symbol: intent.symbol,
            venue: intent.venue,
            currency: intent.currency,
            side: domain_oms_side(intent.side),
            quantity: QuantityMicros(intent.quantity_micros),
            limit_price: PriceMicros(intent.limit_price_micros),
            mode: domain_oms_execution_mode(intent.execution_mode),
            trading_day: intent.trading_day,
        },
        authorized_notional: MoneyMicros(intent.authorized_notional_micros),
        risk_policy_version: intent.risk_policy_version,
        authorized_at_ns: intent.authorized_at_ns,
    }
}

fn risk_proposal(proposal: OrderProposal) -> risk_wire::RiskProposalInput {
    risk_wire::RiskProposalInput {
        proposal_id: proposal.proposal_id,
        strategy_id: proposal.strategy_id,
        symbol: proposal.symbol,
        venue: proposal.venue,
        currency: proposal.currency,
        side: match proposal.side {
            Side::Buy => risk_wire::SideInput::Buy,
            Side::Sell => risk_wire::SideInput::Sell,
        },
        quantity_micros: proposal.quantity.0,
        limit_price_micros: proposal.limit_price.0,
        execution_mode: match proposal.mode {
            ExecutionMode::Paper => risk_wire::ExecutionModeInput::Paper,
            ExecutionMode::Live => risk_wire::ExecutionModeInput::Live,
        },
        trading_day: proposal.trading_day,
    }
}

fn risk_portfolio(
    portfolio: &PortfolioRiskSnapshot,
) -> Result<risk_wire::PortfolioRiskInput, GolemPortError> {
    Ok(risk_wire::PortfolioRiskInput {
        as_of_ns: portfolio.as_of_ns,
        trading_day: portfolio.trading_day,
        gross_exposure_micros: portfolio.gross_exposure.0,
        strategy_exposure: portfolio
            .strategy_exposure
            .iter()
            .map(|(name, amount)| risk_wire::NamedExposureInput {
                name: name.clone(),
                amount_micros: amount.0,
            })
            .collect(),
        symbol_positions: portfolio
            .symbol_positions_micros
            .iter()
            .map(|(symbol, quantity)| {
                i64::try_from(*quantity)
                    .map(|quantity_micros| risk_wire::SymbolPositionInput {
                        symbol: symbol.clone(),
                        quantity_micros,
                    })
                    .map_err(|_| {
                        GolemPortError::InvalidDomain(
                            "portfolio position exceeds Golem i64 wire range".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
        daily_order_count: portfolio.daily_order_count,
    })
}

fn risk_intent(intent: risk_wire::OrderIntentInput) -> Result<OrderIntent, String> {
    Ok(OrderIntent {
        client_order_id: intent.client_order_id,
        proposal: OrderProposal {
            proposal_id: intent.proposal_id,
            strategy_id: intent.strategy_id,
            symbol: intent.symbol,
            venue: intent.venue,
            currency: intent.currency,
            side: match intent.side {
                risk_wire::SideInput::Buy => Side::Buy,
                risk_wire::SideInput::Sell => Side::Sell,
            },
            quantity: QuantityMicros(intent.quantity_micros),
            limit_price: PriceMicros(intent.limit_price_micros),
            mode: match intent.execution_mode {
                risk_wire::ExecutionModeInput::Paper => ExecutionMode::Paper,
                risk_wire::ExecutionModeInput::Live => ExecutionMode::Live,
            },
            trading_day: intent.trading_day,
        },
        authorized_notional: MoneyMicros(intent.authorized_notional_micros),
        risk_policy_version: intent.risk_policy_version,
        authorized_at_ns: intent.authorized_at_ns,
    })
}

fn risk_status(output: risk_wire::RiskStatusOutput) -> RiskPortStatus {
    RiskPortStatus {
        account_id: output.account_id,
        policy_version: output.policy_version,
        portfolio_as_of_ns: output.portfolio_as_of_ns,
        trading_day: output.trading_day,
        gross_exposure_micros: output.gross_exposure_micros,
        reserved_gross_micros: output.reserved_gross_micros,
        outstanding_reservations: output.outstanding_reservations,
        kill_switch_active: output.kill_switch_active,
    }
}

fn risk_rejection(code: &str) -> Result<helio_execution::RiskRejection, String> {
    use helio_execution::RiskRejection as R;
    match code {
        "kill_switch_active" => Ok(R::KillSwitchActive),
        "live_execution_disabled" => Ok(R::LiveExecutionDisabled),
        "venue_not_allowed" => Ok(R::VenueNotAllowed),
        "venue_session_closed" => Ok(R::VenueSessionClosed),
        "venue_calendar_unavailable" => Ok(R::VenueCalendarUnavailable),
        "stale_market_data" => Ok(R::StaleMarketData),
        "stale_portfolio" => Ok(R::StalePortfolio),
        "trading_day_mismatch" => Ok(R::TradingDayMismatch),
        "order_notional_limit" => Ok(R::OrderNotionalLimit),
        "gross_exposure_limit" => Ok(R::GrossExposureLimit),
        "strategy_exposure_limit" => Ok(R::StrategyExposureLimit),
        "symbol_position_limit" => Ok(R::SymbolPositionLimit),
        "daily_order_limit" => Ok(R::DailyOrderLimit),
        "zero_order_value" => Ok(R::ZeroOrderValue),
        "invalid_proposal" => Ok(R::InvalidProposal),
        "arithmetic_overflow" => Ok(R::ArithmeticOverflow),
        other => Err(format!("unknown durable risk rejection code {other}")),
    }
}

fn oms_side(side: Side) -> oms_wire::SideInput {
    match side {
        Side::Buy => oms_wire::SideInput::Buy,
        Side::Sell => oms_wire::SideInput::Sell,
    }
}

fn domain_oms_side(side: oms_wire::SideInput) -> Side {
    match side {
        oms_wire::SideInput::Buy => Side::Buy,
        oms_wire::SideInput::Sell => Side::Sell,
    }
}

fn oms_execution_mode(mode: ExecutionMode) -> oms_wire::ExecutionModeInput {
    match mode {
        ExecutionMode::Paper => oms_wire::ExecutionModeInput::Paper,
        ExecutionMode::Live => oms_wire::ExecutionModeInput::Live,
    }
}

fn domain_oms_execution_mode(mode: oms_wire::ExecutionModeInput) -> ExecutionMode {
    match mode {
        oms_wire::ExecutionModeInput::Paper => ExecutionMode::Paper,
        oms_wire::ExecutionModeInput::Live => ExecutionMode::Live,
    }
}

fn oms_time_in_force(value: TimeInForce) -> oms_wire::TimeInForceInput {
    match value {
        TimeInForce::Day => oms_wire::TimeInForceInput::Day,
        TimeInForce::GoodTillCanceled => oms_wire::TimeInForceInput::GoodTillCanceled,
        TimeInForce::ImmediateOrCancel => oms_wire::TimeInForceInput::ImmediateOrCancel,
        TimeInForce::FillOrKill => oms_wire::TimeInForceInput::FillOrKill,
    }
}

fn domain_time_in_force(value: oms_wire::TimeInForceInput) -> TimeInForce {
    match value {
        oms_wire::TimeInForceInput::Day => TimeInForce::Day,
        oms_wire::TimeInForceInput::GoodTillCanceled => TimeInForce::GoodTillCanceled,
        oms_wire::TimeInForceInput::ImmediateOrCancel => TimeInForce::ImmediateOrCancel,
        oms_wire::TimeInForceInput::FillOrKill => TimeInForce::FillOrKill,
    }
}

fn order_state(value: oms_wire::OrderStateOutput) -> OrderState {
    match value {
        oms_wire::OrderStateOutput::PendingSubmit => OrderState::PendingSubmit,
        oms_wire::OrderStateOutput::Working => OrderState::Working,
        oms_wire::OrderStateOutput::PartiallyFilled => OrderState::PartiallyFilled,
        oms_wire::OrderStateOutput::PendingCancel => OrderState::PendingCancel,
        oms_wire::OrderStateOutput::PendingReplace => OrderState::PendingReplace,
        oms_wire::OrderStateOutput::Filled => OrderState::Filled,
        oms_wire::OrderStateOutput::Canceled => OrderState::Canceled,
        oms_wire::OrderStateOutput::Rejected => OrderState::Rejected,
        oms_wire::OrderStateOutput::Expired => OrderState::Expired,
        oms_wire::OrderStateOutput::Unknown => OrderState::Unknown,
    }
}

fn oms_reconciled_state(value: ReconciledState) -> oms_wire::ReconciledStateInput {
    match value {
        ReconciledState::Working => oms_wire::ReconciledStateInput::Working,
        ReconciledState::Canceled => oms_wire::ReconciledStateInput::Canceled,
        ReconciledState::Rejected => oms_wire::ReconciledStateInput::Rejected,
        ReconciledState::Expired => oms_wire::ReconciledStateInput::Expired,
    }
}

fn invocation_error(error: impl ToString) -> GolemPortError {
    GolemPortError::Invocation(error.to_string())
}

fn oms_agent_error(error: oms_wire::OmsAgentError) -> GolemPortError {
    GolemPortError::Agent(format!("{error:?}"))
}

fn risk_agent_error(error: risk_wire::RiskAgentError) -> GolemPortError {
    GolemPortError::Agent(format!("{error:?}"))
}
