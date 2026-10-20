use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{Client, Url};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

use crate::*;

pub type DecisionFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

/// Effects are replaceable in tests; the adapter stays pure.
pub trait DecisionTransport: Send + Sync {
    /// Non-secret destination identity, including account/routing configuration.
    fn identity(&self) -> Value;
    fn send(&self, body: Value) -> DecisionFuture<'_, Result<Value, DecisionError>>;
}

/// Redirects are disabled. Credentials are sent only to the selected official origin.
pub struct HttpTransport {
    client: Client,
    endpoint: Url,
    key: Zeroizing<String>,
    protocol: Protocol,
    max_bytes: usize,
}
impl HttpTransport {
    pub fn new(
        adapter: &ProviderAdapter,
        key: String,
        cloudflare_account: Option<&str>,
        max_bytes: usize,
    ) -> Result<Self, DecisionError> {
        if key.trim().is_empty() || key.contains(['\r', '\n']) || max_bytes == 0 {
            return Err(DecisionError::InvalidRequest);
        }
        let endpoint = match adapter.protocol {
            Protocol::OpenAi => "https://api.openai.com/v1/decisions".to_owned(),
            Protocol::SystemOne => "https://api.typesafe.ai/v1/systemone".to_owned(),
            Protocol::Cloudflare => {
                let account = cloudflare_account
                    .filter(|v| v.len() == 32 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or(DecisionError::InvalidRequest)?;
                if !matches!(adapter.model.as_str(), "clef" | "clef-flash") {
                    return Err(DecisionError::Unsupported);
                }
                format!("https://api.cloudflare.com/client/v4/accounts/{account}/ai/run/@cf/cloudflare/{}", adapter.model)
            }
        };
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| DecisionError::Unavailable)?;
        Ok(Self {
            client,
            endpoint: Url::parse(&endpoint).map_err(|_| DecisionError::InvalidRequest)?,
            key: Zeroizing::new(key),
            protocol: adapter.protocol,
            max_bytes,
        })
    }
}
impl DecisionTransport for HttpTransport {
    fn identity(&self) -> Value {
        json!({"endpoint":self.endpoint.as_str()})
    }
    fn send(&self, body: Value) -> DecisionFuture<'_, Result<Value, DecisionError>> {
        Box::pin(async move {
            let bytes = serde_json::to_vec(&body).map_err(|_| DecisionError::InvalidRequest)?;
            if bytes.len() > self.max_bytes {
                return Err(DecisionError::TooLarge);
            }
            let mut request = self
                .client
                .post(self.endpoint.clone())
                .bearer_auth(self.key.as_str())
                .header("Content-Type", "application/json");
            if self.protocol == Protocol::SystemOne
                && (body.get("assets").is_some()
                    || body["questions"]
                        .as_object()
                        .is_some_and(|qs| qs.values().any(|q| q["type"] == "box")))
            {
                request = request.header("TypeSafe-Danger-Zone-Images", "true");
            }
            let mut response = request.body(bytes).send().await.map_err(|e| {
                if e.is_timeout() {
                    DecisionError::Expired
                } else {
                    DecisionError::Unavailable
                }
            })?;
            match response.status().as_u16() {
                200..=299 => {}
                401 | 403 => return Err(DecisionError::Authentication),
                429 | 529 => return Err(DecisionError::RateLimited),
                500..=599 => return Err(DecisionError::Unavailable),
                _ => return Err(DecisionError::Rejected),
            }
            if response
                .content_length()
                .is_some_and(|n| n > self.max_bytes as u64)
            {
                return Err(DecisionError::TooLarge);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| DecisionError::Unavailable)?
            {
                if chunk.len() > self.max_bytes.saturating_sub(bytes.len()) {
                    return Err(DecisionError::TooLarge);
                }
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&bytes).map_err(|_| DecisionError::InvalidResponse)
        })
    }
}

pub enum Claim {
    New,
    Replay(DecisionReceipt),
}
pub trait DecisionJournal: Send + Sync {
    /// Atomically reserve an unseen ID, or return its existing outcome.
    /// Apply admission only to new IDs: a stored historical answer can be replayed.
    fn claim(
        &self,
        request: &DecisionRequest,
        fingerprint: &str,
        now_ms: u64,
    ) -> Result<Claim, DecisionError>;
    fn finish(&self, receipt: &DecisionReceipt) -> Result<(), DecisionError>;
}

/// Local single-machine durability. A pending row after a crash is deliberately
/// ambiguous: do not make a second billable call under the same request identity.
pub struct SqliteJournal {
    connection: Mutex<Connection>,
    max_records: usize,
}
impl SqliteJournal {
    pub fn open(path: impl AsRef<Path>, max_records: usize) -> Result<Self, DecisionError> {
        let connection = Connection::open(path).map_err(|_| DecisionError::Journal)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(|_| DecisionError::Journal)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| DecisionError::Journal)?;
        if version != 0 && version != 1 {
            return Err(DecisionError::Journal);
        }
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS decisions (
               request_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL,
               request_json TEXT NOT NULL, receipt_json TEXT
             );
             PRAGMA user_version=1;",
            )
            .map_err(|_| DecisionError::Journal)?;
        Ok(Self {
            connection: Mutex::new(connection),
            max_records,
        })
    }
}
impl DecisionJournal for SqliteJournal {
    fn claim(
        &self,
        request: &DecisionRequest,
        fp: &str,
        now_ms: u64,
    ) -> Result<Claim, DecisionError> {
        let mut connection = self.connection.lock().map_err(|_| DecisionError::Journal)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| DecisionError::Journal)?;
        let existing: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT fingerprint,request_json,receipt_json FROM decisions WHERE request_id=?1",
                [&request.request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|_| DecisionError::Journal)?;
        if let Some((existing_fp, stored_request, receipt)) = existing {
            if existing_fp != fp {
                return Err(DecisionError::IdentityConflict);
            }
            let stored_request: DecisionRequest =
                serde_json::from_str(&stored_request).map_err(|_| DecisionError::Journal)?;
            if stored_request != *request {
                return Err(DecisionError::Journal);
            }
            let receipt = receipt.ok_or(DecisionError::Pending)?;
            let receipt: DecisionReceipt =
                serde_json::from_str(&receipt).map_err(|_| DecisionError::Journal)?;
            if receipt.fingerprint != fp {
                return Err(DecisionError::Journal);
            }
            receipt
                .validate_for(request)
                .map_err(|_| DecisionError::Journal)?;
            return Ok(Claim::Replay(receipt));
        }
        request.admit(now_ms)?;
        let count: usize = tx
            .query_row("SELECT count(*) FROM decisions", [], |r| r.get(0))
            .map_err(|_| DecisionError::Journal)?;
        if count >= self.max_records {
            return Err(DecisionError::Busy);
        }
        tx.execute(
            "INSERT INTO decisions(request_id,fingerprint,request_json) VALUES(?1,?2,?3)",
            params![
                request.request_id,
                fp,
                serde_json::to_string(request).map_err(|_| DecisionError::Journal)?
            ],
        )
        .map_err(|_| DecisionError::Journal)?;
        tx.commit().map_err(|_| DecisionError::Journal)?;
        Ok(Claim::New)
    }
    fn finish(&self, receipt: &DecisionReceipt) -> Result<(), DecisionError> {
        let connection = self.connection.lock().map_err(|_| DecisionError::Journal)?;
        let request: String = connection
            .query_row(
                "SELECT request_json FROM decisions WHERE request_id=?1 AND fingerprint=?2",
                params![receipt.request_id, receipt.fingerprint],
                |row| row.get(0),
            )
            .map_err(|_| DecisionError::Journal)?;
        let request: DecisionRequest =
            serde_json::from_str(&request).map_err(|_| DecisionError::Journal)?;
        receipt
            .validate_for(&request)
            .map_err(|_| DecisionError::Journal)?;
        let changed = connection.execute(
            "UPDATE decisions SET receipt_json=?1 WHERE request_id=?2 AND fingerprint=?3 AND receipt_json IS NULL",
            params![serde_json::to_string(receipt).map_err(|_| DecisionError::Journal)?, receipt.request_id, receipt.fingerprint]
        ).map_err(|_| DecisionError::Journal)?;
        if changed != 1 {
            return Err(DecisionError::Journal);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecisionDelivery {
    pub replayed: bool,
    pub receipt: DecisionReceipt,
}

pub struct DecisionService {
    adapter: Arc<dyn DecisionAdapter>,
    transport: Arc<dyn DecisionTransport>,
    journal: Arc<dyn DecisionJournal>,
    clock: Arc<dyn Clock>,
    capacity: Semaphore,
    max_request_bytes: usize,
    timeout: Duration,
}
impl DecisionService {
    pub fn new(
        adapter: Arc<dyn DecisionAdapter>,
        transport: Arc<dyn DecisionTransport>,
        journal: Arc<dyn DecisionJournal>,
        clock: Arc<dyn Clock>,
        concurrency: usize,
        max_request_bytes: usize,
        timeout: Duration,
    ) -> Result<Self, DecisionError> {
        if concurrency == 0 || concurrency > 1024 || max_request_bytes == 0 || timeout.is_zero() {
            return Err(DecisionError::InvalidRequest);
        }
        Ok(Self {
            adapter,
            transport,
            journal,
            clock,
            capacity: Semaphore::new(concurrency),
            max_request_bytes,
            timeout,
        })
    }

    pub async fn execute(
        &self,
        request: DecisionRequest,
    ) -> Result<DecisionDelivery, DecisionError> {
        let _permit = self
            .capacity
            .try_acquire()
            .map_err(|_| DecisionError::Busy)?;
        let started = tokio::time::Instant::now();
        let body = self.adapter.encode(&request)?;
        if serde_json::to_vec(&body)
            .map_err(|_| DecisionError::InvalidRequest)?
            .len()
            > self.max_request_bytes
        {
            return Err(DecisionError::TooLarge);
        }
        let fp = fingerprint(&json!({
            "request":request, "wire":body, "adapter":self.adapter.identity(),
            "destination":self.transport.identity()
        }))?;
        let journal = self.journal.clone();
        let claim_request = request.clone();
        let claim_fp = fp.clone();
        let now = self.clock.now_ms();
        let claim =
            tokio::task::spawn_blocking(move || journal.claim(&claim_request, &claim_fp, now))
                .await
                .map_err(|_| DecisionError::Journal)??;
        if let Claim::Replay(receipt) = claim {
            if receipt.fingerprint != fp {
                return Err(DecisionError::Journal);
            }
            receipt
                .validate_for(&request)
                .map_err(|_| DecisionError::Journal)?;
            return Ok(DecisionDelivery {
                replayed: true,
                receipt,
            });
        }
        // Recheck after journal I/O. All failures after reservation are persisted.
        let call_started = self.clock.now_ms();
        let mut outcome = match request.admit(call_started) {
            Err(e) => Err(e),
            Ok(()) => {
                let budget = self
                    .timeout
                    .min(Duration::from_millis(request.deadline_ms - call_started));
                match tokio::time::timeout(budget, self.transport.send(body)).await {
                    Err(_) => Err(DecisionError::Expired),
                    Ok(Err(e)) => Err(e),
                    Ok(Ok(value)) => self.adapter.decode(&request, value).and_then(|response| {
                        response.validate_for(&request)?;
                        Ok(response)
                    }),
                }
            }
        };
        let observed_at = self.clock.now_ms();
        if observed_at < call_started || observed_at >= request.deadline_ms {
            outcome = Err(DecisionError::Expired);
        }
        let receipt = DecisionReceipt {
            schema_version: SCHEMA_VERSION,
            request_id: request.request_id,
            fingerprint: fp,
            effective_at_ms: request.effective_at_ms,
            available_at_ms: request.available_at_ms,
            received_at_ms: observed_at.max(call_started).max(request.available_at_ms),
            latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            outcome,
        };
        let journal = self.journal.clone();
        let to_store = receipt.clone();
        tokio::task::spawn_blocking(move || journal.finish(&to_store))
            .await
            .map_err(|_| DecisionError::Journal)??;
        Ok(DecisionDelivery {
            replayed: false,
            receipt,
        })
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn http_fixture(
        reply: Vec<u8>,
        max_bytes: usize,
    ) -> (HttpTransport, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut byte = [0; 1];
                socket.read_exact(&mut byte).await.unwrap();
                bytes.push(byte[0]);
                if bytes.ends_with(b"\r\n\r\n") {
                    break;
                }
                assert!(bytes.len() < 8192);
            }
            let headers = String::from_utf8(bytes).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = vec![0; length];
            socket.read_exact(&mut body).await.unwrap();
            socket.write_all(&reply).await.unwrap();
            headers
        });
        // Loopback endpoints are constructible only inside this private test module.
        (
            HttpTransport {
                client: Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                endpoint: Url::parse(&format!("http://{address}/decisions")).unwrap(),
                key: Zeroizing::new("fixture-key".into()),
                protocol: Protocol::SystemOne,
                max_bytes,
            },
            task,
        )
    }

    #[tokio::test]
    async fn transport_sends_auth_and_image_opt_in_and_decodes_json() {
        let payload = r#"{"model":"fixture","answers":{}}"#;
        let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}", payload.len());
        let (transport, server) = http_fixture(reply.into_bytes(), 1024).await;
        let result = transport
            .send(json!({"assets":{"image":{}}}))
            .await
            .unwrap();
        assert_eq!(result["model"], "fixture");
        let headers = server.await.unwrap().to_ascii_lowercase();
        assert!(headers.contains("authorization: bearer fixture-key"));
        assert!(headers.contains("typesafe-danger-zone-images: true"));
    }

    #[tokio::test]
    async fn redirects_and_auth_errors_do_not_expose_response_body() {
        for (status, expected) in [
            (302, DecisionError::Rejected),
            (401, DecisionError::Authentication),
            (429, DecisionError::RateLimited),
            (529, DecisionError::RateLimited),
        ] {
            let reply = format!("HTTP/1.1 {status} Error\r\nLocation: http://127.0.0.1:1/leak\r\nContent-Length: 11\r\n\r\nfixture-key");
            let (transport, server) = http_fixture(reply.into_bytes(), 1024).await;
            let result = transport.send(json!({})).await.unwrap_err();
            assert_eq!(result, expected);
            assert!(!result.to_string().contains("fixture-key"));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn streamed_body_limit_does_not_depend_on_content_length() {
        let body = "x".repeat(200);
        let reply = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nc8\r\n{body}\r\n0\r\n\r\n"
        );
        let (transport, server) = http_fixture(reply.into_bytes(), 100).await;
        assert_eq!(
            transport.send(json!({})).await.unwrap_err(),
            DecisionError::TooLarge
        );
        server.await.unwrap();
    }

    #[test]
    fn cloudflare_destination_cannot_be_supplied_as_an_arbitrary_url() {
        let adapter = ProviderAdapter {
            protocol: Protocol::Cloudflare,
            model: "clef".into(),
        };
        assert!(matches!(
            HttpTransport::new(&adapter, "fixture-key".into(), Some("../outside"), 1000),
            Err(DecisionError::InvalidRequest)
        ));
    }
}
