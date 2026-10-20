//! One bounded recovery request over private STDIN. Never an order-submission interface.

use std::io::{self, Read};
use std::time::Duration;

use helio_crypto::CryptoOrderRecord;
use helio_execution::BrokerError;
use helio_robinhood::{
    CryptoJournalBinding, Ed25519RequestSigner, HttpMethod, HttpRequest, HttpResponse,
    ReqwestRobinhoodTransport, RobinhoodCryptoBroker, RobinhoodCryptoConfig, RobinhoodTransport,
    SystemUnixClock, TransportError,
};
use serde::Deserialize;
use serde_json::json;
use zeroize::Zeroize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    account_number: String,
    journal_account_key: String,
    api_key: String,
    private_key_base64: String,
    cancel_allowed: bool,
    record: CryptoOrderRecord,
}

struct RecoveryTransport {
    inner: ReqwestRobinhoodTransport,
    cancel_allowed: bool,
    gets: u32,
    cancels: u32,
}

impl RobinhoodTransport for RecoveryTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        // Even a future adapter regression cannot turn this entrypoint into a submitter.
        if !request.body.is_empty() {
            return Err(TransportError::Unavailable);
        }
        match request.method {
            HttpMethod::Get
                if self.gets < 4
                    && request
                        .path_and_query
                        .starts_with("/api/v2/crypto/trading/orders/?") =>
            {
                self.gets += 1;
            }
            HttpMethod::Post
                if self.cancel_allowed
                    && self.cancels == 0
                    && request
                        .path_and_query
                        .strip_prefix("/api/v2/crypto/trading/orders/")
                        .and_then(|path| path.strip_suffix("/cancel/"))
                        .is_some_and(|id| {
                            uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id)
                        }) =>
            {
                self.cancels += 1;
            }
            _ => return Err(TransportError::Unavailable),
        }
        self.inner.execute(request)
    }
}

fn run() -> Result<serde_json::Value, &'static str> {
    let mut raw = Vec::new();
    io::stdin()
        .take(262_145)
        .read_to_end(&mut raw)
        .map_err(|_| "input")?;
    if raw.len() > 262_144 {
        raw.zeroize();
        return Err("input_bound");
    }
    let parsed = serde_json::from_slice::<Request>(&raw);
    raw.zeroize();
    let mut request = parsed.map_err(|_| "request_shape")?;
    if request.schema_version != 1 {
        request.private_key_base64.zeroize();
        return Err("request_version");
    }
    let signer = Ed25519RequestSigner::from_base64(request.api_key, &request.private_key_base64);
    request.private_key_base64.zeroize();
    let signer = signer.map_err(|_| "credentials")?;
    let binding =
        CryptoJournalBinding::try_new(&request.account_number, &request.journal_account_key)
            .map_err(|_| "binding")?;
    let config = RobinhoodCryptoConfig::try_new(request.account_number).map_err(|_| "account")?;
    let transport = RecoveryTransport {
        inner: ReqwestRobinhoodTransport::try_new(Duration::from_secs(3))
            .map_err(|_| "transport")?,
        cancel_allowed: request.cancel_allowed,
        gets: 0,
        cancels: 0,
    };
    let mut broker = RobinhoodCryptoBroker::try_new(config, transport, SystemUnixClock, signer)
        .map_err(|_| "configuration")?;
    // Removing the marker for a read-only invocation affects this ephemeral copy only.
    // The durable journal retains the original cancellation request.
    if !request.cancel_allowed {
        request.record.cancel_requested_ms = None;
    }
    let result = broker.recover_and_cancel_crypto_order(&binding, &request.record);
    let mut output = match result {
        Ok(observation) => json!({"observation":observation}),
        Err(BrokerError::AmbiguousOutcome) => json!({"error":"ambiguous"}),
        Err(_) => json!({"error":"unavailable"}),
    };
    output["get_requests"] = broker.transport().gets.into();
    output["cancel_requests"] = broker.transport().cancels.into();
    Ok(output)
}

fn main() {
    let mut result = run().unwrap_or_else(|reason| json!({"error":reason}));
    result["schema_version"] = 1.into();
    // Only bounded observations and fixed error codes are emitted, never credentials or bodies.
    println!("{result}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_transport_rejects_submissions_and_read_only_cancellation_before_http() {
        for allowed in [false, true] {
            let mut transport = RecoveryTransport {
                inner: ReqwestRobinhoodTransport::try_new(Duration::from_secs(1)).unwrap(),
                cancel_allowed: allowed,
                gets: 0,
                cancels: 0,
            };
            let request = |path: &str| HttpRequest {
                method: HttpMethod::Post,
                path_and_query: path.into(),
                body: Vec::new(),
                headers: Default::default(),
            };
            assert!(transport
                .execute(request(
                    "/api/v2/crypto/trading/orders/?account_number=test"
                ))
                .is_err());
            if !allowed {
                assert!(transport.execute(request(
                    "/api/v2/crypto/trading/orders/11299b2b-61e3-43e7-b9f7-dee77210bb29/cancel/"
                )).is_err());
            }
            assert_eq!((transport.gets, transport.cancels), (0, 0));
        }
    }
}
