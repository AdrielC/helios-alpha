use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use clap::{Parser, ValueEnum};
use helio_decision::*;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Clone, Copy, ValueEnum)]
enum Provider {
    Openai,
    Jev,
    Clef,
    ClefFlash,
}

#[derive(Parser)]
#[command(
    about = "Local journaled decision inference. Emits research evidence; cannot submit orders."
)]
struct Args {
    #[arg(long, value_enum, default_value = "openai")]
    provider: Provider,
    #[arg(long)]
    model: Option<String>,
    /// JSONL requests; omit to read stdin. Lines are processed with bounded concurrency.
    #[arg(long, conflicts_with = "smoke")]
    input: Option<PathBuf>,
    /// One small synthetic predicate to validate credentials and provider access.
    #[arg(long)]
    smoke: bool,
    #[arg(long, default_value = "data/decisions/journal.sqlite3")]
    journal: PathBuf,
    #[arg(long, default_value_t = 4)]
    concurrency: usize,
    #[arg(long, default_value_t = 10_000)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 2_097_152)]
    max_bytes: usize,
    #[arg(long, default_value_t = 100_000)]
    max_records: usize,
    /// Use an explicit recorded provider response, without credentials or network I/O.
    #[arg(long)]
    fixture_response: Option<PathBuf>,
    /// Defaults to ~/.config/helios-alpha/decision-credentials.json.
    #[arg(long)]
    credentials: Option<PathBuf>,
}

struct FixtureTransport {
    response: Value,
}
impl DecisionTransport for FixtureTransport {
    fn identity(&self) -> Value {
        json!({"fixture_response": self.response})
    }
    fn send(&self, _: Value) -> DecisionFuture<'_, Result<Value, DecisionError>> {
        Box::pin(async { Ok(self.response.clone()) })
    }
}

fn credentials(path: Option<PathBuf>) -> Result<Value, DecisionError> {
    let path = path.or_else(|| {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".config/helios-alpha/decision-credentials.json"))
    });
    let Some(path) = path else {
        return Ok(Value::Null);
    };
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(_) => return Err(DecisionError::Authentication),
        Ok(meta) => {
            if !meta.is_file() || meta.len() > 16_384 {
                return Err(DecisionError::Authentication);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o077 != 0 {
                    return Err(DecisionError::Authentication);
                }
            }
        }
    }
    let bytes =
        zeroize::Zeroizing::new(std::fs::read(path).map_err(|_| DecisionError::Authentication)?);
    serde_json::from_slice(&bytes).map_err(|_| DecisionError::Authentication)
}

fn setting(key: &str, file: &Value) -> Option<String> {
    std::env::var(key)
        .ok()
        .or_else(|| file[key].as_str().map(str::to_owned))
}

async fn emit(result: Result<DecisionDelivery, DecisionError>) -> Result<bool, DecisionError> {
    let failed = match &result {
        Err(_) => true,
        Ok(d) => d.receipt.outcome.is_err(),
    };
    let mut bytes = serde_json::to_vec(&result).map_err(|_| DecisionError::InvalidResponse)?;
    bytes.push(b'\n');
    tokio::io::stdout()
        .write_all(&bytes)
        .await
        .map_err(|_| DecisionError::Unavailable)?;
    Ok(failed)
}

async fn run(args: Args) -> Result<bool, DecisionError> {
    if args.max_bytes == 0 || args.max_bytes > 32 * 1024 * 1024 {
        return Err(DecisionError::InvalidRequest);
    }
    let (protocol, default_model, key_name) = match args.provider {
        Provider::Openai => (Protocol::OpenAi, "gpt-6-luna", "OPENAI_API_KEY"),
        Provider::Jev => (Protocol::SystemOne, "jev-latest", "TYPESAFE_API_KEY"),
        Provider::Clef => (Protocol::Cloudflare, "clef", "CLOUDFLARE_API_TOKEN"),
        Provider::ClefFlash => (Protocol::Cloudflare, "clef-flash", "CLOUDFLARE_API_TOKEN"),
    };
    let adapter = Arc::new(ProviderAdapter {
        protocol,
        model: args.model.unwrap_or(default_model.into()),
    });
    let transport: Arc<dyn DecisionTransport> = if let Some(path) = args.fixture_response {
        let bytes = std::fs::read(path).map_err(|_| DecisionError::InvalidRequest)?;
        if bytes.len() > args.max_bytes {
            return Err(DecisionError::TooLarge);
        }
        Arc::new(FixtureTransport {
            response: serde_json::from_slice(&bytes).map_err(|_| DecisionError::InvalidResponse)?,
        })
    } else {
        let mut file = credentials(args.credentials)?;
        let key = setting(key_name, &file).ok_or(DecisionError::Authentication)?;
        let account = setting("CLOUDFLARE_ACCOUNT_ID", &file);
        // Avoid retaining a second copy of credentials for the runner's lifetime.
        if let Some(fields) = file.as_object_mut() {
            use zeroize::Zeroize;
            for v in fields.values_mut() {
                if let Value::String(s) = v {
                    s.zeroize();
                }
            }
        }
        Arc::new(HttpTransport::new(
            &adapter,
            key,
            account.as_deref(),
            args.max_bytes,
        )?)
    };
    if let Some(parent) = args.journal.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|_| DecisionError::Journal)?;
    }
    let service = Arc::new(DecisionService::new(
        adapter,
        transport,
        Arc::new(SqliteJournal::open(args.journal, args.max_records)?),
        Arc::new(SystemClock),
        args.concurrency,
        args.max_bytes,
        Duration::from_millis(args.timeout_ms),
    )?);
    if args.smoke {
        let now = SystemClock.now_ms();
        let request = DecisionRequest {
            schema_version: SCHEMA_VERSION,
            request_id: format!("smoke-{now}"),
            effective_at_ms: now,
            available_at_ms: now,
            deadline_ms: now.saturating_add(args.timeout_ms),
            input: DecisionInput::State {
                value: json!({"event":"synthetic connectivity test","source_verified":true}),
            },
            questions: BTreeMap::from([(
                "verified".into(),
                Question::Predicate {
                    instructions: json!(
                        "Does the supplied synthetic record set source_verified to true?"
                    ),
                    criteria: None,
                },
            )]),
            native_options: None,
        };
        return emit(service.execute(request).await).await;
    }
    let input: Box<dyn tokio::io::AsyncRead + Unpin + Send> = match args.input {
        Some(path) => Box::new(
            tokio::fs::File::open(path)
                .await
                .map_err(|_| DecisionError::InvalidRequest)?,
        ),
        None => Box::new(tokio::io::stdin()),
    };
    let mut reader = BufReader::new(input);
    let mut tasks = tokio::task::JoinSet::new();
    let mut failed = false;
    let mut line = Vec::new();
    loop {
        // Keep the partial buffer if select cancels read_until. Completed decisions
        // must be delivered even when the input stream remains open and idle.
        let remaining = args.max_bytes + 1 - line.len();
        let mut bounded = (&mut reader).take(remaining as u64);
        tokio::select! {
            result = tasks.join_next(), if !tasks.is_empty() => {
                failed |= emit(result.ok_or(DecisionError::Unavailable)?
                    .map_err(|_| DecisionError::Unavailable)?).await?;
            }
            result = bounded.read_until(b'\n', &mut line), if tasks.len() < args.concurrency => {
                let length = result.map_err(|_| DecisionError::Unavailable)?;
                if length == 0 && line.is_empty() { break; }
                if line.len() > args.max_bytes { return Err(DecisionError::TooLarge); }
                if !line.iter().all(u8::is_ascii_whitespace) {
                    match serde_json::from_slice::<DecisionRequest>(&line) {
                        Ok(request) => {
                            let worker = service.clone();
                            tasks.spawn(async move { worker.execute(request).await });
                        }
                        Err(_) => { failed |= emit(Err(DecisionError::InvalidRequest)).await?; }
                    }
                }
                line.clear();
            }
        }
    }
    while let Some(result) = tasks.join_next().await {
        failed |= emit(result.map_err(|_| DecisionError::Unavailable)?).await?;
    }
    Ok(failed)
}

#[tokio::main]
async fn main() {
    match run(Args::parse()).await {
        Ok(false) => {}
        Ok(true) => std::process::exit(1),
        Err(e) => {
            eprintln!("{e}");
            if e == DecisionError::Authentication {
                eprintln!("Run python3 scripts/setup-decision-credentials.py locally, or set the provider's API-key environment variable.");
            }
            std::process::exit(1);
        }
    }
}
