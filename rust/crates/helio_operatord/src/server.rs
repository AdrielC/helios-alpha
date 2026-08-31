use crate::auth::{AuthError, CommandAuth};
use crate::store::{now, CommandExecutor, OperatorStore, StoreError};
use crate::time_series::{TimeSeriesError, TimeSeriesPort};
use crate::types::{
    CommandReceipt, CommandRequest, EntityHistoryEvent, InvestigationCitation,
    InvestigationRequest, InvestigationResult, SavedWorkspace,
};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, ETAG, IF_MATCH};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

const MAX_COMMAND_BYTES: usize = 64 * 1024;
const MAX_QUERY_BYTES: usize = 512 * 1024;
const IDEMPOTENCY_HEADER: &str = "idempotency-key";

#[derive(Clone, Debug)]
struct RecordedCommand {
    request_digest: [u8; 32],
    receipt: CommandReceipt,
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<OperatorStore>,
    pub auth: CommandAuth,
    pub command_executor: Arc<dyn CommandExecutor>,
    pub time_series: Arc<dyn TimeSeriesPort>,
    command_gate: Arc<Mutex<HashMap<String, RecordedCommand>>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppState")
            .field("store", &self.store)
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl AppState {
    pub fn new(
        store: Arc<OperatorStore>,
        auth: CommandAuth,
        command_executor: Arc<dyn CommandExecutor>,
        time_series: Arc<dyn TimeSeriesPort>,
    ) -> Self {
        Self {
            store,
            auth,
            command_executor,
            time_series,
            command_gate: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub fn router(state: AppState, static_dir: Option<PathBuf>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/operations/snapshot", get(snapshot))
        .route("/operations/stream", get(snapshot_stream))
        .route("/series/catalog", get(series_catalog))
        .route("/forecasts", get(forecast_bundles))
        .route("/series/query", post(series_query))
        .route("/entities/{kind}/{id}/history", get(entity_history))
        .route("/workspaces/{id}", get(get_workspace).put(save_workspace))
        .route("/investigations", post(investigate))
        .route("/command/session", get(command_session))
        .route(
            "/commands",
            post(command).layer(DefaultBodyLimit::max(MAX_COMMAND_BYTES)),
        )
        .layer(DefaultBodyLimit::max(MAX_QUERY_BYTES));

    let mut application = Router::new()
        .route("/runtime-config.js", get(runtime_config))
        .nest("/api/v1", api);
    if let Some(directory) = static_dir {
        let index = directory.join("index.html");
        application = application
            .fallback_service(ServeDir::new(directory).not_found_service(ServeFile::new(index)));
    }

    let request_id = HeaderName::from_static("x-request-id");
    application
        .with_state(state)
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .layer(SetSensitiveRequestHeadersLayer::new([
            COOKIE,
            HeaderName::from_static("authorization"),
            HeaderName::from_static("x-helios-csrf"),
        ]))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static("default-src 'self'; base-uri 'none'; connect-src 'self'; font-src 'self'; form-action 'none'; frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; worker-src 'self' blob:"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(CompressionLayer::new())
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
}

async fn runtime_config() -> Response {
    let body = r#"window.__HELIOS_OPERATIONS__ = Object.freeze({
  snapshotUrl: "/api/v1/operations/snapshot",
  streamUrl: "/api/v1/operations/stream",
  timeSeriesCatalogUrl: "/api/v1/series/catalog",
  forecastBundlesUrl: "/api/v1/forecasts",
  timeSeriesQueryUrl: "/api/v1/series/query",
  investigationUrl: "/api/v1/investigations",
  commandSessionUrl: "/api/v1/command/session",
  commandUrl: "/api/v1/commands"
});
"#;
    let mut response = body.into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    schema_version: u8,
    status: &'static str,
    snapshot_sequence: u64,
    command_channel: &'static str,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    let snapshot = state.store.snapshot().await;
    Json(Health {
        schema_version: 1,
        status: "ok",
        snapshot_sequence: snapshot.sequence,
        command_channel: if state.auth.is_enabled() {
            "authentication-required"
        } else {
            "disabled"
        },
    })
}

async fn snapshot(State(state): State<AppState>) -> Response {
    let snapshot = state.store.snapshot().await;
    let sequence = snapshot.sequence;
    let mut response = Json(snapshot).into_response();
    response.headers_mut().insert(
        ETAG,
        HeaderValue::from_str(&format!("\"{sequence}\"")).expect("sequence ETag is valid"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn snapshot_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let initial = state.store.snapshot().await;
    let receiver = state.store.subscribe();
    let store = state.store.clone();
    let requested_cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let initial_stream =
        stream::once(async move { Ok(snapshot_event(&initial, requested_cursor)) });
    let updates = stream::unfold((receiver, store), |(mut receiver, store)| async move {
        match receiver.recv().await {
            Ok(snapshot) => Some((Ok(snapshot_event(&snapshot, None)), (receiver, store))),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let snapshot = store.snapshot().await;
                Some((Ok(snapshot_event(&snapshot, None)), (receiver, store)))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    });
    Sse::new(initial_stream.chain(updates)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("projection-alive"),
    )
}

fn snapshot_event(snapshot: &crate::types::OperationsSnapshot, _requested: Option<u64>) -> Event {
    Event::default()
        .event("snapshot")
        .id(snapshot.sequence.to_string())
        .json_data(snapshot)
        .unwrap_or_else(|_| {
            Event::default()
                .event("projection-error")
                .data("serialization failed")
        })
}

async fn series_catalog(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    json_value(state.time_series.catalog()?)
}

async fn forecast_bundles(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    json_value(state.time_series.forecast_bundles()?)
}

async fn series_query(
    State(state): State<AppState>,
    Json(request): Json<crate::types::TimeSeriesRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    json_value(state.time_series.query(&request)?)
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_history_limit")]
    limit: usize,
}

fn default_history_limit() -> usize {
    100
}

async fn entity_history(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
    Query(query): Query<HistoryQuery>,
) -> Json<crate::types::HistoryPage> {
    Json(
        state
            .store
            .history(&kind, &id, query.after, query.limit)
            .await,
    )
}

async fn get_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SavedWorkspace>, ApiError> {
    state
        .store
        .workspace(&id)
        .await
        .map(Json)
        .ok_or_else(|| ApiError::not_found("workspace_not_found", "Workspace was not found"))
}

async fn save_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(mut workspace): Json<SavedWorkspace>,
) -> Result<Json<SavedWorkspace>, ApiError> {
    let actor = state.auth.authenticate(&headers)?.to_owned();
    state.auth.verify_csrf(&headers)?;
    if id != workspace.workspace_id {
        return Err(ApiError::bad_request(
            "workspace_identity_mismatch",
            "Path and body workspace identities differ",
        ));
    }
    let expected = optional_etag(&headers)?;
    workspace.schema_version = 1;
    Ok(Json(
        state
            .store
            .save_workspace(&actor, workspace, expected)
            .await?,
    ))
}

async fn investigate(
    State(state): State<AppState>,
    Json(request): Json<InvestigationRequest>,
) -> Result<Json<InvestigationResult>, ApiError> {
    let from = OffsetDateTime::parse(&request.from, &Rfc3339);
    let to = OffsetDateTime::parse(&request.to, &Rfc3339);
    let cursor = OffsetDateTime::parse(&request.cursor, &Rfc3339);
    if request.schema_version != 1
        || !matches!((&from, &to), (Ok(from), Ok(to)) if from < to)
        || !matches!((&from, &to, &cursor), (Ok(from), Ok(to), Ok(cursor)) if cursor >= from && cursor <= to)
    {
        return Err(ApiError::bad_request(
            "invalid_investigation",
            "Investigation schema and interval must be valid",
        ));
    }
    let snapshot = state.store.snapshot().await;
    if request.context.account_id != snapshot.context.account_id {
        return Err(ApiError::forbidden(
            "account_scope_mismatch",
            "Investigation account does not match the current projection",
        ));
    }
    if request.snapshot_sequence > snapshot.sequence {
        return Err(ApiError::conflict(
            "future_snapshot",
            "Investigation snapshot is newer than the current projection",
        ));
    }
    let catalog = state.time_series.catalog()?;
    let suggestions = catalog
        .iter()
        .filter(|descriptor| !request.series_ids.contains(&descriptor.id))
        .take(3)
        .map(|descriptor| descriptor.id.clone())
        .collect();
    let citations = request
        .series_ids
        .iter()
        .take(12)
        .map(|id| InvestigationCitation {
            id: format!("series:{id}:{}", request.snapshot_sequence),
            source_id: id.clone(),
            label: catalog
                .iter()
                .find(|descriptor| descriptor.id == *id)
                .map_or_else(|| id.clone(), |descriptor| descriptor.label.clone()),
            timestamp: request.cursor.clone(),
        })
        .collect();
    Ok(Json(InvestigationResult {
        schema_version: 1,
        investigation_id: Uuid::new_v4().to_string(),
        generated_at: now()?,
        model_id: "helios-evidence-index-v1".into(),
        summary: "The selected interval has been indexed against the exact snapshot and registered observation lineage. Open the cited records before assigning causality.".into(),
        limitation: "Read-only deterministic evidence index. It does not establish causality or grant execution authority.".into(),
        suggested_series_ids: suggestions,
        citations,
    }))
}

async fn command_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<crate::types::CommandSession>, ApiError> {
    Ok(Json(state.auth.session(&headers)?))
}

async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> Result<Json<CommandReceipt>, ApiError> {
    if headers
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|size| size > MAX_COMMAND_BYTES)
    {
        return Err(ApiError::payload_too_large());
    }
    let actor = state.auth.authenticate(&headers)?.to_owned();
    state.auth.verify_csrf(&headers)?;
    if headers
        .get("x-helios-command")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Err(ApiError::forbidden(
            "command_intent_header_missing",
            "X-Helios-Command must explicitly identify a command request",
        ));
    }
    request
        .validate()
        .map_err(|message| ApiError::bad_request("invalid_command", &message))?;
    let key = idempotency_key(&headers)?;
    let request_bytes = serde_json::to_vec(&request).map_err(ApiError::serialization)?;
    let digest: [u8; 32] = Sha256::digest(request_bytes).into();

    let mut commands = state.command_gate.lock().await;
    if let Some(existing) = commands.get(&key) {
        if existing.request_digest == digest {
            return Ok(Json(existing.receipt.clone()));
        }
        return Err(ApiError::conflict(
            "idempotency_conflict",
            "Idempotency key was already used for a different command",
        ));
    }

    let snapshot = state.store.snapshot().await;
    let header_sequence = required_etag(&headers)?;
    if header_sequence != request.expected_sequence
        || snapshot.sequence != request.expected_sequence
    {
        return Err(ApiError::precondition_failed(
            "snapshot_changed",
            "Current snapshot no longer matches the reviewed command",
        ));
    }

    let outcome = state
        .command_executor
        .execute(&actor, &request, &state.store)
        .await
        .map_err(|error| {
            ApiError::service_unavailable("command_execution_failed", &error.to_string())
        })?;
    let submitted_at = now()?;
    let receipt = CommandReceipt {
        schema_version: 1,
        command_id: Uuid::new_v4().to_string(),
        idempotency_key: key.clone(),
        action: request.action.clone(),
        target_id: request.target_id.clone(),
        status: outcome.status,
        submitted_at: submitted_at.clone(),
        message: outcome.message,
        expected_sequence: request.expected_sequence,
    };
    let history = EntityHistoryEvent {
        schema_version: 1,
        cursor: 0,
        event_id: receipt.command_id.clone(),
        entity_kind: "command".into(),
        entity_id: request.target_id.clone(),
        occurred_at: submitted_at.clone(),
        observed_at: submitted_at,
        actor,
        event_type: request.action.as_str().to_owned(),
        payload: serde_json::json!({
            "status": receipt.status,
            "reason": request.reason,
            "snapshotSequence": request.expected_sequence,
            "idempotencyKey": key,
        }),
    };
    state.store.append_history(history).await?;
    commands.insert(
        receipt.idempotency_key.clone(),
        RecordedCommand {
            request_digest: digest,
            receipt: receipt.clone(),
        },
    );
    Ok(Json(receipt))
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get(IDEMPOTENCY_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::bad_request(
                "idempotency_key_missing",
                "Idempotency-Key is required for commands",
            )
        })?;
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ApiError::bad_request(
            "idempotency_key_invalid",
            "Idempotency-Key must contain 1 to 128 visible ASCII characters",
        ));
    }
    Ok(key.to_owned())
}

fn required_etag(headers: &HeaderMap) -> Result<u64, ApiError> {
    optional_etag(headers)?.ok_or_else(|| {
        ApiError::precondition_required(
            "if_match_required",
            "If-Match snapshot sequence is required",
        )
    })
}

fn optional_etag(headers: &HeaderMap) -> Result<Option<u64>, ApiError> {
    let Some(value) = headers.get(IF_MATCH) else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| ApiError::bad_request("if_match_invalid", "If-Match is invalid"))?;
    raw.trim_matches('"')
        .parse::<u64>()
        .map(Some)
        .map_err(|_| ApiError::bad_request("if_match_invalid", "If-Match is invalid"))
}

fn json_value<T: Serialize>(value: T) -> Result<Json<serde_json::Value>, ApiError> {
    serde_json::to_value(value)
        .map(Json)
        .map_err(ApiError::serialization)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    schema_version: u8,
    code: &'static str,
    message: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn bad_request(code: &'static str, message: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    fn forbidden(code: &'static str, message: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    fn not_found(code: &'static str, message: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }

    fn conflict(code: &'static str, message: &str) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    fn precondition_failed(code: &'static str, message: &str) -> Self {
        Self::new(StatusCode::PRECONDITION_FAILED, code, message)
    }

    fn precondition_required(code: &'static str, message: &str) -> Self {
        Self::new(StatusCode::PRECONDITION_REQUIRED, code, message)
    }

    fn service_unavailable(code: &'static str, message: &str) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "command_too_large",
            "Command payload exceeds the configured limit",
        )
    }

    fn serialization(error: serde_json::Error) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "serialization_failed",
            error.to_string(),
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ErrorEnvelope {
                schema_version: 1,
                code: self.code,
                message: self.message,
            }),
        )
            .into_response();
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        response
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::Disabled => {
                Self::service_unavailable("command_auth_disabled", &error.to_string())
            }
            AuthError::Unauthorized => Self::new(
                StatusCode::UNAUTHORIZED,
                "operator_unauthorized",
                error.to_string(),
            ),
            AuthError::InvalidCsrf => Self::forbidden("csrf_rejected", &error.to_string()),
            AuthError::InvalidExpiry => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_clock_failed",
                error.to_string(),
            ),
        }
    }
}

impl From<TimeSeriesError> for ApiError {
    fn from(error: TimeSeriesError) -> Self {
        match error {
            TimeSeriesError::ContextMismatch => {
                Self::forbidden("account_scope_mismatch", &error.to_string())
            }
            TimeSeriesError::Poisoned => {
                Self::service_unavailable("series_repository_unavailable", &error.to_string())
            }
            _ => Self::bad_request("invalid_time_series_query", &error.to_string()),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::WorkspaceConflict | StoreError::SequenceRegression => {
                Self::conflict("state_conflict", &error.to_string())
            }
            StoreError::WorkspaceOwnerMismatch | StoreError::AccountIdentityChanged => {
                Self::forbidden("state_scope_mismatch", &error.to_string())
            }
            StoreError::InvalidSnapshot(_) => {
                Self::bad_request("invalid_snapshot", &error.to_string())
            }
            StoreError::Clock => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "clock_failed",
                error.to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::CommandAuth;
    use crate::fixtures::{default_catalog, default_forecast_bundles, empty_snapshot};
    use crate::store::{CommandOutcome, ReadOnlyCommandExecutor};
    use crate::time_series::InMemoryTimeSeriesPort;
    use crate::types::{CommandAction, CommandStatus};
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use axum::body::Body;
    use axum::http::Request;
    use http::header::COOKIE;
    use time::Duration as TimeDuration;
    use tower::ServiceExt;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";
    const SECRET: &[u8] = b"abcdef0123456789abcdef0123456789";

    #[derive(Debug)]
    struct CompletingExecutor;

    #[async_trait]
    impl CommandExecutor for CompletingExecutor {
        async fn execute(
            &self,
            _actor: &str,
            command: &CommandRequest,
            store: &OperatorStore,
        ) -> Result<CommandOutcome, crate::store::CommandExecutionError> {
            if matches!(command.action, CommandAction::ActivateKillSwitch) {
                store
                    .mutate_snapshot(|snapshot| {
                        snapshot.risk.kill_switch_active = true;
                        Ok(())
                    })
                    .await
                    .map_err(|error| {
                        crate::store::CommandExecutionError::Infrastructure(error.to_string())
                    })?;
            }
            Ok(CommandOutcome {
                status: CommandStatus::Completed,
                message: "Applied".into(),
            })
        }
    }

    fn state(executor: Arc<dyn CommandExecutor>) -> AppState {
        let snapshot = empty_snapshot();
        let account_id = snapshot.context.account_id.clone();
        AppState::new(
            OperatorStore::new(snapshot).unwrap(),
            CommandAuth::enabled("operator", TOKEN, SECRET, TimeDuration::minutes(15)).unwrap(),
            executor,
            Arc::new(InMemoryTimeSeriesPort::new(
                account_id,
                default_catalog(),
                default_forecast_bundles(),
            )),
        )
    }

    async fn json_body(response: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
    }

    fn cookie() -> HeaderValue {
        HeaderValue::from_str(&format!("helios_operator_session={TOKEN}")).unwrap()
    }

    async fn csrf(application: &Router) -> String {
        let response = application
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/command/session")
                    .header(COOKIE, cookie())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await["csrfToken"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn command_request(csrf: &str, key: &str, sequence: u64, reason: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/v1/commands")
            .header(COOKIE, cookie())
            .header("x-helios-csrf", csrf)
            .header("x-helios-command", "1")
            .header(IDEMPOTENCY_HEADER, key)
            .header(IF_MATCH, format!("\"{sequence}\""))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "schemaVersion": 1,
                    "action": "activate_kill_switch",
                    "targetId": "account",
                    "reason": reason,
                    "confirmation": "ACTIVATE",
                    "expectedSequence": sequence
                }))
                .unwrap(),
            ))
            .unwrap()
    }

    #[tokio::test]
    async fn snapshot_has_no_store_and_monotonic_etag() {
        let application = router(state(Arc::new(ReadOnlyCommandExecutor)), None);
        let response = application
            .oneshot(
                Request::builder()
                    .uri("/api/v1/operations/snapshot")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[ETAG], "\"1\"");
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(json_body(response).await["schemaVersion"], 2);
    }

    #[tokio::test]
    async fn commands_require_cookie_csrf_intent_idempotency_and_sequence() {
        let application = router(state(Arc::new(CompletingExecutor)), None);
        let csrf = csrf(&application).await;

        let response = application
            .clone()
            .oneshot(command_request(
                &csrf,
                "kill-1",
                1,
                "Operational drill activation",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let receipt = json_body(response).await;
        assert_eq!(receipt["status"], "completed");
        assert_eq!(receipt["idempotencyKey"], "kill-1");

        let replay = application
            .clone()
            .oneshot(command_request(
                &csrf,
                "kill-1",
                1,
                "Operational drill activation",
            ))
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::OK);
        assert_eq!(json_body(replay).await["commandId"], receipt["commandId"]);

        let conflict = application
            .oneshot(command_request(
                &csrf,
                "kill-1",
                1,
                "Different operational reason",
            ))
            .await
            .unwrap();
        assert_eq!(conflict.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn stale_sequence_never_reaches_executor() {
        let application = router(state(Arc::new(CompletingExecutor)), None);
        let csrf = csrf(&application).await;
        let response = application
            .oneshot(command_request(
                &csrf,
                "stale",
                99,
                "Operational drill activation",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn command_session_never_works_without_the_session_cookie() {
        let application = router(state(Arc::new(ReadOnlyCommandExecutor)), None);
        let response = application
            .oneshot(
                Request::builder()
                    .uri("/api/v1/command/session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
