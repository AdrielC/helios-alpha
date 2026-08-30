use helio_execution::{OrderIntent, PriceMicros, QuantityMicros};
use thiserror::Error;

use crate::{OmsCommand, OmsError, OmsPort, OrderState, TimeInForce};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OmsConformanceError {
    #[error("OMS command failed during {stage}: {source}")]
    Command {
        stage: &'static str,
        source: OmsError,
    },
    #[error("OMS did not preserve the expected state during {0}")]
    State(&'static str),
    #[error("OMS did not expose committed events by cursor")]
    Cursor,
    #[error("OMS does not advertise the required Helios v1 capabilities")]
    Capabilities,
}

/// Run the portable behavior required from either the built-in or an external OMS.
///
/// The supplied intent must use a fresh client order identity in the target system.
pub fn verify_oms_conformance(
    oms: &mut impl OmsPort,
    intent: OrderIntent,
    base_time_ns: u64,
) -> Result<(), OmsConformanceError> {
    let capabilities = oms.capabilities();
    if capabilities.protocol_version != 1
        || !capabilities.supports_limit_orders
        || !capabilities.supports_cancel
        || !capabilities.supports_fractional_quantity
        || !capabilities.supports_event_cursor
        || !capabilities.time_in_force.contains(&TimeInForce::Day)
    {
        return Err(OmsConformanceError::Capabilities);
    }
    let order_id = intent.client_order_id.clone();
    let submit = OmsCommand::Submit {
        command_id: format!("conformance:{order_id}:submit"),
        intent: intent.clone(),
        time_in_force: TimeInForce::Day,
        at_ns: base_time_ns,
    };
    let first = oms
        .execute(submit.clone())
        .map_err(|source| OmsConformanceError::Command {
            stage: "submit",
            source,
        })?;
    let replay = oms
        .execute(submit)
        .map_err(|source| OmsConformanceError::Command {
            stage: "submit replay",
            source,
        })?;
    if first.replayed || !replay.replayed || first.version != replay.version {
        return Err(OmsConformanceError::State("idempotent submit"));
    }

    oms.execute(OmsCommand::Acknowledge {
        command_id: format!("conformance:{order_id}:ack"),
        client_order_id: order_id.clone(),
        broker_order_id: format!("conformance-venue-{order_id}"),
        at_ns: base_time_ns.saturating_add(1),
    })
    .map_err(|source| OmsConformanceError::Command {
        stage: "acknowledge",
        source,
    })?;
    let fill_quantity = QuantityMicros(intent.proposal.quantity.0 / 2);
    if fill_quantity.0 > 0 {
        oms.execute(OmsCommand::RecordFill {
            command_id: format!("conformance:{order_id}:fill"),
            client_order_id: order_id.clone(),
            broker_order_id: Some(format!("conformance-venue-{order_id}")),
            execution_id: format!("conformance-exec-{order_id}"),
            venue_occurred_at: None,
            quantity: fill_quantity,
            price: PriceMicros(intent.proposal.limit_price.0),
            at_ns: base_time_ns.saturating_add(2),
        })
        .map_err(|source| OmsConformanceError::Command {
            stage: "partial fill",
            source,
        })?;
    }
    let snapshot = oms
        .order(&order_id)
        .map_err(|source| OmsConformanceError::Command {
            stage: "query",
            source,
        })?
        .ok_or(OmsConformanceError::State("query after fill"))?;
    let expected = if fill_quantity.0 > 0 {
        OrderState::PartiallyFilled
    } else {
        OrderState::Working
    };
    if snapshot.state != expected || snapshot.filled_quantity != fill_quantity {
        return Err(OmsConformanceError::State("exact partial fill"));
    }

    oms.execute(OmsCommand::RequestCancel {
        command_id: format!("conformance:{order_id}:cancel"),
        client_order_id: order_id.clone(),
        at_ns: base_time_ns.saturating_add(3),
    })
    .map_err(|source| OmsConformanceError::Command {
        stage: "cancel request",
        source,
    })?;
    oms.execute(OmsCommand::ConfirmCanceled {
        command_id: format!("conformance:{order_id}:canceled"),
        client_order_id: order_id.clone(),
        at_ns: base_time_ns.saturating_add(4),
    })
    .map_err(|source| OmsConformanceError::Command {
        stage: "cancel confirmation",
        source,
    })?;
    if oms
        .order(&order_id)
        .map_err(|source| OmsConformanceError::Command {
            stage: "terminal query",
            source,
        })?
        .is_none_or(|snapshot| snapshot.state != OrderState::Canceled)
    {
        return Err(OmsConformanceError::State("terminal cancellation"));
    }
    let events = oms
        .events_after(0, 32)
        .map_err(|source| OmsConformanceError::Command {
            stage: "event cursor",
            source,
        })?;
    if events.is_empty()
        || events
            .windows(2)
            .any(|pair| pair[0].cursor >= pair[1].cursor)
    {
        return Err(OmsConformanceError::Cursor);
    }
    Ok(())
}
