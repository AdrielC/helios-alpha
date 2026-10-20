//! Feeds synthetic API JSON through the real recovery adapter for the isolated Golem verifier.
//! No native transport, broker credentials, or order submission path is instantiated.

use std::collections::VecDeque;
use std::io::{self, Read};

use helio_crypto::{CryptoOrderIntent, CryptoOrderRecord, CryptoPreparationContext};
use helio_robinhood::{
    ClockError, CryptoJournalBinding, HttpMethod, HttpRequest, HttpResponse, RequestSigner,
    RobinhoodCryptoBroker, RobinhoodCryptoConfig, RobinhoodTransport, SignedHeaders, SigningError,
    TransportError, UnixClock,
};
use serde::Deserialize;

const FIXTURE_ACCOUNT: &str = "syntheticJournalApiAccount";
const MAX_FIXTURE_BYTES: u64 = 1_048_576;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Fixture {
    Recover {
        intent: CryptoOrderIntent,
        response_body: String,
        received_ms: u64,
    },
    Prepare {
        intent: CryptoOrderIntent,
        context: Box<CryptoPreparationContext>,
        now_ms: u64,
    },
    RecoverAndCancel {
        record: Box<CryptoOrderRecord>,
        /// Null simulates a lost HTTP reply.
        responses: Vec<Option<FixtureResponse>>,
        received_ms: u64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureResponse {
    status: u16,
    body: String,
}

struct FixtureClock(u64);
impl UnixClock for FixtureClock {
    fn unix_seconds(&mut self) -> Result<u64, ClockError> {
        Ok(self.0 / 1000)
    }
    fn unix_millis(&mut self) -> Result<u64, ClockError> {
        Ok(self.0)
    }
}

struct FixtureSigner;
impl RequestSigner for FixtureSigner {
    fn sign(
        &self,
        timestamp: u64,
        _: &str,
        _: HttpMethod,
        _: &[u8],
    ) -> Result<SignedHeaders, SigningError> {
        Ok(SignedHeaders {
            api_key: "synthetic-journal-key".into(),
            signature: "synthetic-journal-signature".into(),
            timestamp: timestamp.to_string(),
        })
    }
}

struct FixtureTransport {
    responses: VecDeque<Option<FixtureResponse>>,
    reads: usize,
    cancellations: usize,
}
impl RobinhoodTransport for FixtureTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        if !request.body.is_empty() {
            return Err(TransportError::Unavailable);
        }
        match request.method {
            HttpMethod::Get
                if request.path_and_query
                    == format!(
                        "/api/v2/crypto/trading/orders/?account_number={FIXTURE_ACCOUNT}"
                    ) =>
            {
                self.reads += 1;
            }
            HttpMethod::Post
                if request
                    .path_and_query
                    .strip_prefix("/api/v2/crypto/trading/orders/")
                    .and_then(|v| v.strip_suffix("/cancel/"))
                    .is_some_and(|v| {
                        uuid::Uuid::parse_str(v).is_ok_and(|id| id.to_string() == v)
                    }) =>
            {
                self.cancellations += 1;
            }
            _ => return Err(TransportError::Unavailable),
        }
        let response = self
            .responses
            .pop_front()
            .ok_or(TransportError::Unavailable)?
            .ok_or(TransportError::OutcomeUnknown)?;
        Ok(HttpResponse {
            status: response.status,
            body: response.body.into_bytes(),
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(MAX_FIXTURE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FIXTURE_BYTES {
        return Err("synthetic fixture exceeded the byte limit".into());
    }
    let fixture: Fixture = serde_json::from_slice(&bytes)?;
    let (intent, responses, at_ms, context, record) = match fixture {
        Fixture::Recover {
            intent,
            response_body,
            received_ms,
        } => (
            intent,
            VecDeque::from([Some(FixtureResponse {
                status: 200,
                body: response_body,
            })]),
            received_ms,
            None,
            None,
        ),
        Fixture::Prepare {
            intent,
            context,
            now_ms,
        } => (intent, VecDeque::new(), now_ms, Some(context), None),
        Fixture::RecoverAndCancel {
            record,
            responses,
            received_ms,
        } => (
            record.intent.clone(),
            responses.into(),
            received_ms,
            None,
            Some(record),
        ),
    };
    let binding = CryptoJournalBinding::try_new(FIXTURE_ACCOUNT, &intent.account_key)?;
    let mut broker = RobinhoodCryptoBroker::try_new(
        RobinhoodCryptoConfig::try_new(FIXTURE_ACCOUNT)?,
        FixtureTransport {
            responses,
            reads: 0,
            cancellations: 0,
        },
        FixtureClock(at_ms),
        FixtureSigner,
    )?;
    let mut output = if let Some(record) = record {
        match broker.recover_and_cancel_crypto_order(&binding, &record) {
            Ok(observation) => serde_json::json!({"observation": observation}),
            Err(error) => serde_json::json!({"error": format!("{error:?}")}),
        }
    } else if let Some(context) = context {
        let draft = broker.prepare_crypto_limit_order(&binding, intent, &context, at_ms)?;
        serde_json::json!({
            "preparation": draft.preparation(),
            "request_body": std::str::from_utf8(draft.body())?,
            "body_sha256": draft.body_sha256(),
        })
    } else {
        serde_json::json!({"observation": broker.read_crypto_order(&binding, &intent)?})
    };
    output["schema_version"] = 1.into();
    output["execution_authority"] = false.into();
    output["synthetic_get_requests"] = broker.transport().reads.into();
    output["synthetic_cancel_requests"] = broker.transport().cancellations.into();
    println!("{output}");
    Ok(())
}
