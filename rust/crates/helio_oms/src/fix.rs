use helio_execution::{OrderIntent, PriceMicros, QuantityMicros, Side};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{OmsCommand, TimeInForce};

const SOH: u8 = 0x01;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixField {
    pub tag: u32,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixMessage {
    fields: Vec<FixField>,
}

impl FixMessage {
    pub fn new(message_type: impl Into<String>) -> Self {
        Self {
            fields: vec![FixField {
                tag: 35,
                value: message_type.into(),
            }],
        }
    }

    pub fn push(&mut self, tag: u32, value: impl Into<String>) -> &mut Self {
        self.fields.push(FixField {
            tag,
            value: value.into(),
        });
        self
    }

    pub fn get(&self, tag: u32) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.tag == tag)
            .map(|field| field.value.as_str())
    }

    pub fn fields(&self) -> &[FixField] {
        &self.fields
    }

    pub fn encode_fix44(&self) -> Result<Vec<u8>, FixError> {
        if self.get(35).is_none() {
            return Err(FixError::MissingTag(35));
        }
        let mut body = Vec::new();
        for field in &self.fields {
            if matches!(field.tag, 8..=10) {
                continue;
            }
            validate_field(field)?;
            append_field(&mut body, field.tag, &field.value);
        }
        let mut frame = Vec::new();
        append_field(&mut frame, 8, "FIX.4.4");
        append_field(&mut frame, 9, &body.len().to_string());
        frame.extend_from_slice(&body);
        let checksum = frame.iter().fold(0_u32, |sum, byte| sum + u32::from(*byte)) % 256;
        append_field(&mut frame, 10, &format!("{checksum:03}"));
        Ok(frame)
    }

    pub fn parse(frame: &[u8]) -> Result<Self, FixError> {
        let text = std::str::from_utf8(frame).map_err(|_| FixError::NonUtf8)?;
        let raw_fields: Vec<&str> = text
            .split(char::from(SOH))
            .filter(|field| !field.is_empty())
            .collect();
        if raw_fields.len() < 4 {
            return Err(FixError::MalformedFrame);
        }
        let mut parsed = Vec::with_capacity(raw_fields.len());
        for raw in &raw_fields {
            let (tag, value) = raw.split_once('=').ok_or(FixError::MalformedFrame)?;
            let tag = tag.parse::<u32>().map_err(|_| FixError::MalformedFrame)?;
            let field = FixField {
                tag,
                value: value.to_string(),
            };
            validate_field(&field)?;
            parsed.push(field);
        }
        if parsed
            .first()
            .map(|field| (field.tag, field.value.as_str()))
            != Some((8, "FIX.4.4"))
        {
            return Err(FixError::UnsupportedBeginString);
        }
        if parsed.get(1).map(|field| field.tag) != Some(9) {
            return Err(FixError::MissingTag(9));
        }
        if parsed.last().map(|field| field.tag) != Some(10) {
            return Err(FixError::MissingTag(10));
        }

        let body_start = nth_delimiter(frame, 2).ok_or(FixError::MalformedFrame)? + 1;
        let checksum_start = frame
            .windows(4)
            .rposition(|window| window == b"\x0110=")
            .map(|position| position + 1)
            .ok_or(FixError::MissingTag(10))?;
        let declared_body = parsed[1]
            .value
            .parse::<usize>()
            .map_err(|_| FixError::MalformedFrame)?;
        if declared_body != checksum_start.saturating_sub(body_start) {
            return Err(FixError::BodyLengthMismatch);
        }
        let declared_checksum = parsed
            .last()
            .expect("validated non-empty FIX frame")
            .value
            .parse::<u32>()
            .map_err(|_| FixError::MalformedFrame)?;
        let actual_checksum = frame[..checksum_start]
            .iter()
            .fold(0_u32, |sum, byte| sum + u32::from(*byte))
            % 256;
        if declared_checksum != actual_checksum {
            return Err(FixError::ChecksumMismatch);
        }
        Ok(Self {
            fields: parsed
                .into_iter()
                .filter(|field| !matches!(field.tag, 8..=10))
                .collect(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixSessionHeader {
    pub sender_comp_id: String,
    pub target_comp_id: String,
    pub sequence_number: u64,
    /// FIX UTC timestamp in `YYYYMMDD-HH:MM:SS.sss` form.
    pub sending_time: String,
}

impl FixSessionHeader {
    fn append_to(&self, message: &mut FixMessage) {
        message
            .push(49, self.sender_comp_id.clone())
            .push(56, self.target_comp_id.clone())
            .push(34, self.sequence_number.to_string())
            .push(52, self.sending_time.clone());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixOrderMapper {
    pub account: String,
}

impl FixOrderMapper {
    pub fn new(account: impl Into<String>) -> Self {
        Self {
            account: account.into(),
        }
    }

    pub fn new_order_single(
        &self,
        header: &FixSessionHeader,
        intent: &OrderIntent,
        time_in_force: TimeInForce,
        transact_time: &str,
    ) -> Result<FixMessage, FixError> {
        validate_order_intent(intent)?;
        let mut message = FixMessage::new("D");
        header.append_to(&mut message);
        message
            .push(11, intent.client_order_id.clone())
            .push(1, self.account.clone())
            .push(55, intent.proposal.symbol.clone())
            .push(54, fix_side(intent.proposal.side))
            .push(60, transact_time)
            .push(38, format_micros(intent.proposal.quantity.0))
            .push(40, "2")
            .push(44, format_micros(intent.proposal.limit_price.0))
            .push(59, fix_time_in_force(time_in_force));
        Ok(message)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn cancel_request(
        &self,
        header: &FixSessionHeader,
        cancel_client_order_id: &str,
        original_client_order_id: &str,
        broker_order_id: Option<&str>,
        symbol: &str,
        side: Side,
        transact_time: &str,
    ) -> Result<FixMessage, FixError> {
        require_nonempty(cancel_client_order_id)?;
        require_nonempty(original_client_order_id)?;
        let mut message = FixMessage::new("F");
        header.append_to(&mut message);
        message
            .push(11, cancel_client_order_id)
            .push(41, original_client_order_id);
        if let Some(order_id) = broker_order_id {
            message.push(37, order_id);
        }
        message
            .push(1, self.account.clone())
            .push(55, symbol)
            .push(54, fix_side(side))
            .push(60, transact_time);
        Ok(message)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn cancel_replace_request(
        &self,
        header: &FixSessionHeader,
        replacement_client_order_id: &str,
        original_client_order_id: &str,
        broker_order_id: Option<&str>,
        symbol: &str,
        side: Side,
        new_quantity: QuantityMicros,
        new_limit_price: PriceMicros,
        time_in_force: TimeInForce,
        transact_time: &str,
    ) -> Result<FixMessage, FixError> {
        if new_quantity.0 == 0 || new_limit_price.0 == 0 {
            return Err(FixError::ZeroValue);
        }
        let mut message = FixMessage::new("G");
        header.append_to(&mut message);
        message
            .push(11, replacement_client_order_id)
            .push(41, original_client_order_id);
        if let Some(order_id) = broker_order_id {
            message.push(37, order_id);
        }
        message
            .push(1, self.account.clone())
            .push(55, symbol)
            .push(54, fix_side(side))
            .push(60, transact_time)
            .push(38, format_micros(new_quantity.0))
            .push(40, "2")
            .push(44, format_micros(new_limit_price.0))
            .push(59, fix_time_in_force(time_in_force));
        Ok(message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixExecutionReport {
    pub client_order_id: String,
    pub broker_order_id: String,
    pub execution_id: String,
    pub execution_type: String,
    pub order_status: String,
    pub rejection_reason: Option<String>,
    pub transact_time: Option<String>,
    pub last_quantity: Option<QuantityMicros>,
    pub last_price: Option<PriceMicros>,
}

impl TryFrom<&FixMessage> for FixExecutionReport {
    type Error = FixError;

    fn try_from(message: &FixMessage) -> Result<Self, Self::Error> {
        if message.get(35) != Some("8") {
            return Err(FixError::UnexpectedMessageType);
        }
        let last_quantity = message.get(32).map(parse_quantity).transpose()?;
        let last_price = message.get(31).map(parse_price).transpose()?;
        if last_quantity.is_some() != last_price.is_some() {
            return Err(FixError::IncompleteExecution);
        }
        Ok(Self {
            client_order_id: required(message, 11)?.to_string(),
            broker_order_id: required(message, 37)?.to_string(),
            execution_id: required(message, 17)?.to_string(),
            execution_type: required(message, 150)?.to_string(),
            order_status: required(message, 39)?.to_string(),
            rejection_reason: message.get(58).map(str::to_string),
            transact_time: message.get(60).map(str::to_string),
            last_quantity,
            last_price,
        })
    }
}

impl FixExecutionReport {
    /// Translate one venue report into idempotent canonical OMS commands.
    pub fn canonical_commands(&self, at_ns: u64) -> Result<Vec<OmsCommand>, FixError> {
        let prefix = format!("fix:{}", self.execution_id);
        match self.execution_type.as_str() {
            "0" => Ok(vec![self.acknowledgement(format!("{prefix}:ack"), at_ns)]),
            "1" | "2" | "F" => {
                let quantity = self.last_quantity.ok_or(FixError::IncompleteExecution)?;
                let price = self.last_price.ok_or(FixError::IncompleteExecution)?;
                Ok(vec![OmsCommand::RecordFill {
                    command_id: format!("{prefix}:fill"),
                    client_order_id: self.client_order_id.clone(),
                    broker_order_id: Some(self.broker_order_id.clone()),
                    execution_id: self.execution_id.clone(),
                    venue_occurred_at: self.transact_time.clone(),
                    quantity,
                    price,
                    at_ns,
                }])
            }
            "4" => Ok(vec![OmsCommand::ConfirmCanceled {
                command_id: format!("{prefix}:canceled"),
                client_order_id: self.client_order_id.clone(),
                at_ns,
            }]),
            "5" => Ok(vec![OmsCommand::ConfirmReplaced {
                command_id: format!("{prefix}:replaced"),
                client_order_id: self.client_order_id.clone(),
                broker_order_id: self.broker_order_id.clone(),
                at_ns,
            }]),
            "8" => Ok(vec![OmsCommand::Reject {
                command_id: format!("{prefix}:rejected"),
                client_order_id: self.client_order_id.clone(),
                reason: self
                    .rejection_reason
                    .clone()
                    .unwrap_or_else(|| "venue rejected order without text".into()),
                at_ns,
            }]),
            "C" => Ok(vec![OmsCommand::MarkExpired {
                command_id: format!("{prefix}:expired"),
                client_order_id: self.client_order_id.clone(),
                at_ns,
            }]),
            other => Err(FixError::UnsupportedExecutionType(other.to_string())),
        }
    }

    fn acknowledgement(&self, command_id: String, at_ns: u64) -> OmsCommand {
        OmsCommand::Acknowledge {
            command_id,
            client_order_id: self.client_order_id.clone(),
            broker_order_id: self.broker_order_id.clone(),
            at_ns,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixCancelReject {
    pub client_order_id: String,
    pub original_client_order_id: String,
    pub broker_order_id: String,
    pub response_to: String,
    pub reason: String,
}

impl TryFrom<&FixMessage> for FixCancelReject {
    type Error = FixError;

    fn try_from(message: &FixMessage) -> Result<Self, Self::Error> {
        if message.get(35) != Some("9") {
            return Err(FixError::UnexpectedMessageType);
        }
        Ok(Self {
            client_order_id: required(message, 11)?.to_string(),
            original_client_order_id: required(message, 41)?.to_string(),
            broker_order_id: required(message, 37)?.to_string(),
            response_to: required(message, 434)?.to_string(),
            reason: message
                .get(58)
                .unwrap_or("venue rejected cancel or replace without text")
                .to_string(),
        })
    }
}

impl FixCancelReject {
    pub fn canonical_command(&self, at_ns: u64) -> OmsCommand {
        OmsCommand::RejectPendingAction {
            command_id: format!(
                "fix-cancel-reject:{}:{}:{}",
                self.original_client_order_id, self.client_order_id, self.response_to
            ),
            client_order_id: self.original_client_order_id.clone(),
            reason: self.reason.clone(),
            at_ns,
        }
    }
}

/// Boundary implemented by QuickFIX, OnixS, or another certified native FIX session engine.
pub trait FixSessionPort {
    type Error;

    fn send_application_message(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
    fn next_outgoing_sequence(&self) -> u64;
    fn expected_incoming_sequence(&self) -> u64;
    fn request_resend(&mut self, begin_sequence: u64, end_sequence: u64)
        -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FixError {
    #[error("FIX frame is malformed")]
    MalformedFrame,
    #[error("FIX frame is not UTF-8")]
    NonUtf8,
    #[error("FIX.4.4 is required")]
    UnsupportedBeginString,
    #[error("required FIX tag {0} is missing")]
    MissingTag(u32),
    #[error("FIX tag or value contains a delimiter")]
    InvalidField,
    #[error("FIX BodyLength does not match the frame")]
    BodyLengthMismatch,
    #[error("FIX CheckSum does not match the frame")]
    ChecksumMismatch,
    #[error("unexpected FIX message type")]
    UnexpectedMessageType,
    #[error("unsupported FIX execution type {0}")]
    UnsupportedExecutionType(String),
    #[error("execution has only one of LastQty and LastPx")]
    IncompleteExecution,
    #[error("decimal cannot be represented exactly in micros")]
    InvalidDecimal,
    #[error("quantity and price must be nonzero")]
    ZeroValue,
}

fn required(message: &FixMessage, tag: u32) -> Result<&str, FixError> {
    message.get(tag).ok_or(FixError::MissingTag(tag))
}

fn validate_order_intent(intent: &OrderIntent) -> Result<(), FixError> {
    require_nonempty(&intent.client_order_id)?;
    require_nonempty(&intent.proposal.symbol)?;
    if intent.proposal.quantity.0 == 0 || intent.proposal.limit_price.0 == 0 {
        return Err(FixError::ZeroValue);
    }
    Ok(())
}

fn require_nonempty(value: &str) -> Result<(), FixError> {
    if value.trim().is_empty() {
        Err(FixError::InvalidField)
    } else {
        Ok(())
    }
}

fn validate_field(field: &FixField) -> Result<(), FixError> {
    if field.value.bytes().any(|byte| byte == SOH || byte == b'=') {
        Err(FixError::InvalidField)
    } else {
        Ok(())
    }
}

fn append_field(buffer: &mut Vec<u8>, tag: u32, value: &str) {
    buffer.extend_from_slice(tag.to_string().as_bytes());
    buffer.push(b'=');
    buffer.extend_from_slice(value.as_bytes());
    buffer.push(SOH);
}

fn nth_delimiter(frame: &[u8], count: usize) -> Option<usize> {
    frame
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == SOH)
        .nth(count - 1)
        .map(|(index, _)| index)
}

fn fix_side(side: Side) -> &'static str {
    match side {
        Side::Buy => "1",
        Side::Sell => "2",
    }
}

fn fix_time_in_force(time_in_force: TimeInForce) -> &'static str {
    match time_in_force {
        TimeInForce::Day => "0",
        TimeInForce::GoodTillCanceled => "1",
        TimeInForce::ImmediateOrCancel => "3",
        TimeInForce::FillOrKill => "4",
    }
}

fn format_micros(value: u64) -> String {
    let whole = value / 1_000_000;
    let fraction = value % 1_000_000;
    if fraction == 0 {
        return whole.to_string();
    }
    let mut decimal = format!("{whole}.{fraction:06}");
    while decimal.ends_with('0') {
        decimal.pop();
    }
    decimal
}

fn parse_quantity(value: &str) -> Result<QuantityMicros, FixError> {
    parse_micros(value).map(QuantityMicros)
}

fn parse_price(value: &str) -> Result<PriceMicros, FixError> {
    parse_micros(value).map(PriceMicros)
}

fn parse_micros(value: &str) -> Result<u64, FixError> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 6
    {
        return Err(FixError::InvalidDecimal);
    }
    let whole = whole.parse::<u64>().map_err(|_| FixError::InvalidDecimal)?;
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u64>()
            .map_err(|_| FixError::InvalidDecimal)?
            .checked_mul(10_u64.pow(6 - fraction.len() as u32))
            .ok_or(FixError::InvalidDecimal)?
    };
    whole
        .checked_mul(1_000_000)
        .and_then(|scaled| scaled.checked_add(fraction_value))
        .ok_or(FixError::InvalidDecimal)
}

#[cfg(test)]
mod tests {
    use helio_execution::{ExecutionMode, MoneyMicros, OrderProposal};

    use super::*;

    fn intent() -> OrderIntent {
        OrderIntent {
            client_order_id: "client-7".into(),
            proposal: OrderProposal {
                proposal_id: "proposal-7".into(),
                strategy_id: "strategy".into(),
                symbol: "SPY".into(),
                venue: "XNAS".into(),
                currency: "USD".into(),
                side: Side::Buy,
                quantity: QuantityMicros(1_250_000),
                limit_price: PriceMicros(523_125_000),
                mode: ExecutionMode::Paper,
                trading_day: 20260830,
            },
            authorized_notional: MoneyMicros(653_906_250),
            risk_policy_version: "risk-v1".into(),
            authorized_at_ns: 1,
        }
    }

    #[test]
    fn new_order_single_round_trips_with_body_length_and_checksum() {
        let mapper = FixOrderMapper::new("ACCOUNT-1");
        let message = mapper
            .new_order_single(
                &FixSessionHeader {
                    sender_comp_id: "HELIOS".into(),
                    target_comp_id: "VENUE".into(),
                    sequence_number: 42,
                    sending_time: "20260830-14:01:02.003".into(),
                },
                &intent(),
                TimeInForce::Day,
                "20260830-14:01:02.003",
            )
            .unwrap();
        let encoded = message.encode_fix44().unwrap();
        let decoded = FixMessage::parse(&encoded).unwrap();
        assert_eq!(decoded.get(35), Some("D"));
        assert_eq!(decoded.get(38), Some("1.25"));
        assert_eq!(decoded.get(44), Some("523.125"));
    }

    #[test]
    fn execution_report_becomes_idempotent_oms_commands() {
        let mut message = FixMessage::new("8");
        message
            .push(11, "client-7")
            .push(37, "broker-9")
            .push(17, "exec-3")
            .push(150, "F")
            .push(39, "1")
            .push(32, "0.25")
            .push(31, "523.125");
        let report = FixExecutionReport::try_from(&message).unwrap();
        let commands = report.canonical_commands(99).unwrap();
        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], OmsCommand::RecordFill { .. }));
    }

    #[test]
    fn checksum_corruption_is_rejected() {
        let mut message = FixMessage::new("0");
        message.push(49, "HELIOS");
        let mut frame = message.encode_fix44().unwrap();
        frame[5] = b'9';
        assert!(matches!(
            FixMessage::parse(&frame),
            Err(FixError::UnsupportedBeginString) | Err(FixError::ChecksumMismatch)
        ));
    }

    #[test]
    fn cancel_reject_restores_the_canonical_pending_action() {
        let mut message = FixMessage::new("9");
        message
            .push(11, "cancel-8")
            .push(41, "client-7")
            .push(37, "broker-9")
            .push(39, "0")
            .push(434, "1")
            .push(58, "too late to cancel");
        let reject = FixCancelReject::try_from(&message).unwrap();
        assert!(matches!(
            reject.canonical_command(100),
            OmsCommand::RejectPendingAction { .. }
        ));
    }
}
