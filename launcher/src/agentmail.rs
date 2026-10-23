//! Signed, deterministic email admission in front of the existing gateway.
//! No email contents or credentials are persisted by this adapter.
use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use hmac::{Hmac, KeyInit, Mac};
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::{Request, Response, StatusCode, body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    convert::Infallible,
    fs,
    io::{self, BufRead, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};

const ROUTE: &str = "/agentmail/events";
const BACKEND_HOST: &str = "127.0.0.1";
const BACKEND_PORT: u16 = 3887;
const STATE: &str = "/data/zeroclaw/agentmail-state.json";
const BODY_LIMIT: usize = 131_072;
const EVENT_LIMIT: usize = 10_000;
const RECENT_EVENT_LIMIT: usize = 10;
// Dispatch acknowledges admission; it does not wait for agent completion.
const DISPATCH_TIMEOUT: Duration = Duration::from_secs(35);
type Body = UnsyncBoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;
fn failure() -> io::Error {
    io::Error::other("AgentMail adapter configuration or state is invalid")
}

#[derive(Clone)]
pub struct Filter {
    pub name: String,
    sender: String,
    contains: String,
    sop: Option<String>,
}
impl Filter {
    fn sender_matches(&self, message: &Value) -> bool {
        // Match the entire mailbox, never a display-name substring.
        let Some(from) = message["from"].as_str() else {
            return false;
        };
        if from.contains(['\r', '\n']) {
            return false;
        }
        let from = from.trim();
        let mailbox = if let Some((prefix, rest)) = from.split_once('<') {
            // Delimiters inside a quoted display name are text, not additional
            // mailboxes. Quoted-pair escapes must not close that name early.
            let mut quoted = false;
            let mut escaped = false;
            for byte in prefix.bytes() {
                if escaped {
                    escaped = false;
                } else if quoted && byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    quoted = !quoted;
                } else if !quoted && b",;<>".contains(&byte) {
                    return false;
                }
            }
            if quoted || escaped {
                return false;
            }
            rest.strip_suffix('>').unwrap_or("")
        } else {
            from
        };
        !mailbox.contains([',', ';']) && mailbox.trim().eq_ignore_ascii_case(&self.sender)
    }
    fn matches(&self, message: &Value) -> bool {
        self.sender_matches(message)
            && [
                message["subject"].as_str(),
                message["extracted_text"]
                    .as_str()
                    .or_else(|| message["text"].as_str()),
            ]
            .into_iter()
            .flatten()
            .any(|s| s.to_lowercase().contains(&self.contains))
    }
}

pub struct Settings {
    secret: Vec<u8>,
    token: String,
    filters: Vec<Filter>,
    state: PathBuf,
    paired_tokens: String,
    authorization: Option<String>,
    gateway: String,
    telegram: Option<(String, String)>,
    gateway_secret: String,
}
impl Settings {
    pub fn from_env() -> io::Result<Option<Self>> {
        if !template_enabled(include_str!("../../deploy/bot/config.template.toml"))? {
            return Ok(None);
        }
        let secret = std::env::var("HEIMDALL_AGENTMAIL_WEBHOOK_SECRET").map_err(|_| failure())?;
        let secret = STANDARD
            .decode(secret.strip_prefix("whsec_").ok_or_else(failure)?)
            .map_err(|_| failure())?;
        let token = std::env::var("HEIMDALL_AGENTMAIL_GATEWAY_TOKEN").map_err(|_| failure())?;
        if secret.len() < 16
            || token.len() < 32
            || token.len() > 1024
            || !token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(failure());
        }
        let filters = template_filters(include_str!("../../deploy/bot/config.template.toml"))?;
        // Preserve explicitly configured native gateway clients and add a
        // digest for this adapter. The raw bearer never reaches saved TOML.
        let existing = match std::env::var("ZEROCLAW_gateway__paired_tokens") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            _ => return Err(failure()),
        };
        let paired_tokens = paired_tokens(existing.as_deref(), &token)?;
        let gateway_secret = match std::env::var("ZEROCLAW_gateway__webhook_secret") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => String::new(),
            _ => return Err(failure()),
        };
        if gateway_secret.len() > 1024 || gateway_secret.bytes().any(|b| b.is_ascii_control()) {
            return Err(failure());
        }
        let owner = std::env::var("TELEGRAM_OWNER_ID").map_err(|_| failure())?;
        crate::config::parse_allowed_ids(&owner, None).map_err(|_| failure())?;
        let bot = std::env::var("TELEGRAM_BOT_TOKEN").map_err(|_| failure())?;
        crate::config::secret_overrides(&bot, &"0".repeat(32)).map_err(|_| failure())?;
        let authorization = match std::env::var("AGENTMAIL_AUTHORIZATION") {
            Ok(value)
                if value.starts_with("Bearer ")
                    && value.len() <= 1024
                    && value.bytes().all(|b| b.is_ascii_graphic() || b == b' ') =>
            {
                Some(value)
            }
            Err(std::env::VarError::NotPresent) => None,
            _ => return Err(failure()),
        };
        Ok(Some(Self {
            secret,
            token,
            filters,
            state: PathBuf::from(STATE),
            paired_tokens,
            authorization,
            gateway: format!("http://{BACKEND_HOST}:{BACKEND_PORT}"),
            telegram: Some((owner, bot)),
            gateway_secret,
        }))
    }
}

// Encode digest bytes explicitly: sha2 0.11's output array has no LowerHex impl.
fn sha256_hex(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn paired_tokens(existing: Option<&str>, token: &str) -> io::Result<String> {
    let mut tokens: Vec<String> = match existing {
        Some(raw) => serde_json::from_str(raw).map_err(|_| failure())?,
        None => Vec::new(),
    };
    if tokens.len() > 64 || tokens.iter().any(|s| s.len() > 1024) {
        return Err(failure());
    }
    let digest = sha256_hex(token);
    if !tokens.contains(&digest) {
        tokens.push(digest);
    }
    serde_json::to_string(&tokens).map_err(|_| failure())
}

fn template_enabled(template: &str) -> io::Result<bool> {
    let config: toml::Value = toml::from_str(template).map_err(|_| failure())?;
    config
        .get("heimdall")
        .and_then(|v| v.get("agentmail"))
        .and_then(|v| v.get("enabled"))
        .and_then(toml::Value::as_bool)
        .ok_or_else(failure)
}

// The template owns non-secret rules; all rule validation is shared with fixtures.
fn template_filters(template: &str) -> io::Result<Vec<Filter>> {
    let config: toml::Value = toml::from_str(template).map_err(|_| failure())?;
    let filters = config
        .get("heimdall")
        .and_then(|v| v.get("agentmail"))
        .and_then(|v| v.get("filters"))
        .ok_or_else(failure)?;
    let value = serde_json::to_value(filters).map_err(|_| failure())?;
    parse_filters(&value.to_string())
}

fn parse_filters(raw: &str) -> io::Result<Vec<Filter>> {
    if raw.len() > 16_384 {
        return Err(failure());
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| failure())?;
    let entries = value
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .ok_or_else(failure)?;
    let mut filters = Vec::new();
    for v in entries {
        let name = v["name"].as_str().ok_or_else(failure)?;
        let sender = v["sender"].as_str().ok_or_else(failure)?;
        let contains = v["contains"].as_str().ok_or_else(failure)?;
        let sop = match v.get("sop") {
            None => None,
            Some(Value::String(name))
                if !name.is_empty()
                    && name.len() <= 128
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)) =>
            {
                Some(name.clone())
            }
            _ => return Err(failure()),
        };
        if v.as_object().is_none_or(|o| {
            o.keys()
                .any(|k| !["name", "sender", "contains", "sop"].contains(&k.as_str()))
        }) || !valid_id(name)
            || filters.iter().any(|f: &Filter| f.name == name)
            || sender.len() > 254
            || !sender.contains('@')
            || sender
                .bytes()
                .any(|b| !b.is_ascii_graphic() || b"<>,;".contains(&b))
            || contains.trim().is_empty()
            || contains.len() > 256
        {
            return Err(failure());
        }
        filters.push(Filter {
            name: name.into(),
            sender: sender.into(),
            contains: contains.to_lowercase(),
            sop,
        });
    }
    Ok(filters)
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.@<>".contains(&b))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn verify(secret: &[u8], headers: &hyper::HeaderMap, body: &[u8], now: u64) -> bool {
    let get = |s| headers.get(s).and_then(|h| h.to_str().ok());
    let (Some(id), Some(timestamp), Some(signatures)) =
        (get("svix-id"), get("svix-timestamp"), get("svix-signature"))
    else {
        return false;
    };
    let Ok(seconds) = timestamp.parse::<u64>() else {
        return false;
    };
    if !valid_id(id) || now.abs_diff(seconds) > 300 {
        return false;
    }
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    signatures.split_ascii_whitespace().take(8).any(|s| {
        s.strip_prefix("v1,")
            .and_then(|v| STANDARD.decode(v).ok())
            .is_some_and(|sig| mac.clone().verify_slice(&sig).is_ok())
    })
}

// A short exclusive file lock coordinates the supervisor and stdio MCP processes.
// Event reservations deliberately remain after ambiguous dispatch failures: retries
// must never start a second credentialed agent task automatically.
fn state<T>(path: &Path, update: impl FnOnce(&mut Value) -> io::Result<T>) -> io::Result<T> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || meta.permissions().mode() & 0o077 != 0 {
        return Err(failure());
    }
    // SAFETY: file owns a valid descriptor; flock only changes this open file's lock.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(failure());
    }
    let mut raw = String::new();
    (&mut file).take(2_000_001).read_to_string(&mut raw)?;
    if raw.len() > 2_000_000 {
        return Err(failure());
    }
    let mut data = if raw.is_empty() {
        json!({"enabled":{},"events":{}})
    } else {
        serde_json::from_str(&raw).map_err(|_| failure())?
    };
    if data["enabled"]
        .as_object()
        .is_none_or(|v| v.values().any(|b| !b.is_boolean()))
        || data["events"].as_object().is_none_or(|events| {
            events.len() > EVENT_LIMIT
                || events
                    .values()
                    .any(|e| !e.is_object() || !e["status"].is_string() || !e["time"].is_u64())
        })
    {
        return Err(failure());
    }
    let original = serde_json::to_vec(&data).map_err(|_| failure())?;
    let result = update(&mut data)?;
    let bytes = serde_json::to_vec(&data).map_err(|_| failure())?;
    if !raw.is_empty() && bytes == original {
        return Ok(result);
    }
    // Keep the lock on the same inode across all participants. A partial write
    // fails closed on the next read, rather than dropping deduplication state.
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.set_len(bytes.len() as u64)?;
    file.sync_all()?;
    Ok(result)
}

/// Volume locks and fsync must never occupy a gateway worker thread.
async fn async_state<T: Send + 'static>(
    settings: Arc<Settings>,
    update: impl FnOnce(&mut Value) -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(move || state(&settings.state, update))
        .await
        .map_err(|_| failure())?
}

fn needs_body(filters: &[Filter], message: &Value) -> bool {
    message["body_url"].is_string()
        && !message["extracted_text"].is_string()
        && !message["text"].is_string()
        && filters
            .iter()
            .any(|f| f.sender_matches(message) && !f.matches(message))
}

fn response(status: StatusCode, text: &'static str) -> Response<Body> {
    let mut r = Response::new(
        Full::new(Bytes::from_static(text.as_bytes()))
            .map_err(|never: Infallible| match never {})
            .boxed_unsync(),
    );
    *r.status_mut() = status;
    r
}

async fn receive(
    req: Request<Incoming>,
    settings: Arc<Settings>,
    jobs: Arc<Semaphore>,
) -> Response<Body> {
    if req.method() != hyper::Method::POST {
        return response(StatusCode::METHOD_NOT_ALLOWED, "POST required");
    }
    let (parts, mut incoming) = req.into_parts();
    let read = async {
        let mut body = Vec::new();
        while let Some(frame) = incoming.frame().await {
            let frame = frame.map_err(|_| ())?;
            if let Some(bytes) = frame.data_ref() {
                if body.len() + bytes.len() > BODY_LIMIT {
                    return Err(());
                }
                body.extend_from_slice(bytes);
            }
        }
        Ok(body)
    };
    let Ok(Ok(body)) = tokio::time::timeout(Duration::from_secs(10), read).await else {
        return response(StatusCode::PAYLOAD_TOO_LARGE, "Invalid or oversized event");
    };
    if !verify(&settings.secret, &parts.headers, &body, now()) {
        return response(StatusCode::UNAUTHORIZED, "Invalid signature");
    }
    let Ok(event) = serde_json::from_slice::<Value>(&body) else {
        return response(StatusCode::BAD_REQUEST, "Invalid event");
    };
    if event["event_type"] != "message.received" {
        return response(StatusCode::OK, "Ignored");
    }
    let Some(id) = event["event_id"].as_str().filter(|s| valid_id(s)) else {
        return response(StatusCode::BAD_REQUEST, "Missing event ID");
    };
    let event_id = id.to_owned();
    let filters = settings.filters.clone();
    let active = async_state(settings.clone(), move |data| {
        if data["events"].get(&event_id).is_some() {
            return Ok(Vec::new());
        }
        Ok(filters
            .iter()
            .filter(|f| data["enabled"][&f.name].as_bool().unwrap_or(false))
            .cloned()
            .collect::<Vec<_>>())
    })
    .await;
    let active = match active {
        Ok(active) => active,
        Err(_) => {
            return response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Admission state unavailable",
            );
        }
    };
    if active.is_empty() {
        return response(StatusCode::OK, "Ignored or duplicate");
    }
    let mut message = event["message"].clone();
    let Ok(permit) = jobs.try_acquire_owned() else {
        return response(StatusCode::SERVICE_UNAVAILABLE, "Busy; retry later");
    };
    if needs_body(&active, &message) {
        match fetch_message(&settings, &message).await {
            Ok(fetched) => message = fetched,
            Err(_) => return response(StatusCode::SERVICE_UNAVAILABLE, "Message body unavailable"),
        }
    }
    let matched: Vec<_> = active
        .iter()
        .filter(|f| f.matches(&message))
        .map(|f| f.name.clone())
        .collect();
    if matched.is_empty() {
        return response(StatusCode::OK, "Ignored");
    }
    let event_id = id.to_owned();
    let reserve = async_state(settings.clone(), move |data| {
        let selected: Vec<_> = matched
            .iter()
            .filter(|name| data["enabled"][name.as_str()].as_bool().unwrap_or(false))
            .cloned()
            .collect();
        if selected.is_empty() || data["events"].get(&event_id).is_some() {
            return Ok(Vec::new());
        }
        let events = data["events"].as_object_mut().ok_or_else(failure)?;
        // Never silently evict reservations. The operator can archive the ledger.
        if events.len() >= EVENT_LIMIT {
            return Err(failure());
        }
        events.insert(event_id, json!({"time":now(),"status":"reserved"}));
        Ok(selected)
    })
    .await;
    let selected = match reserve {
        Ok(v) if v.is_empty() => return response(StatusCode::OK, "Ignored or duplicate"),
        Ok(v) => v,
        Err(_) => {
            return response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Admission state unavailable",
            );
        }
    };
    let event_id = id.to_string();
    // Email metadata stays untrusted structured data, never chat instructions.
    let summary = json!({"rules":selected,"inbox_id":message["inbox_id"].as_str().unwrap_or("").chars().take(254).collect::<String>(),"message_id":message["message_id"].as_str().unwrap_or("").chars().take(512).collect::<String>()});
    let mut sops: Vec<String> = settings
        .filters
        .iter()
        .filter(|f| selected.contains(&f.name))
        .filter_map(|f| f.sop.clone())
        .collect();
    sops.sort();
    sops.dedup();
    tokio::spawn(async move {
        let _permit = permit;
        let result = dispatch(&settings, &event_id, &summary, &sops).await;
        let diagnostic = result.as_ref().err().map(DispatchError::diagnostic);
        // Hash identifiers for correlation; never print bodies, credentials or URLs.
        let event_ref = sha256_hex(&event_id);
        if let Some(ref diagnostic) = diagnostic {
            eprintln!(
                "AgentMail dispatch uncertain; event_ref={event_ref}; diagnostic={diagnostic}; automatic retry suppressed"
            );
        }
        if async_state(settings.clone(), move |data| {
            data["events"][&event_id]["status"] = json!(if result.is_ok() {
                "accepted"
            } else {
                "uncertain"
            });
            data["events"][&event_id]["diagnostic"] = diagnostic.unwrap_or_else(
                || json!({"stage":"complete","gateway_completed":!sops.is_empty(),"execution_completed":false}),
            );
            Ok(())
        })
        .await
        .is_err()
        {
            eprintln!("AgentMail completion state unavailable; reservation retained");
        }
    });
    response(StatusCode::ACCEPTED, "Accepted")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseBodyError {
    Timeout,
    Transport,
    TooLarge,
    InvalidJson,
}
impl ResponseBodyError {
    fn kind(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::TooLarge => "response_too_large",
            Self::InvalidJson => "invalid_response",
        }
    }
}

async fn bounded_json(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Value, ResponseBodyError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            ResponseBodyError::Timeout
        } else {
            ResponseBodyError::Transport
        }
    })? {
        if bytes.len() + chunk.len() > limit {
            return Err(ResponseBodyError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| ResponseBodyError::InvalidJson)
}

#[derive(Debug)]
enum DispatchError {
    Client,
    State,
    GatewayTransport(bool),
    GatewayHttp(u16),
    GatewayBody(u16, ResponseBodyError),
    GatewayResponse(u16),
    TelegramTransport(bool),
    TelegramHttp(u16),
    TelegramBody(u16, ResponseBodyError),
    TelegramResponse(u16),
}
impl DispatchError {
    fn diagnostic(&self) -> Value {
        let (stage, phase, kind, status, completed) = match self {
            Self::State => ("state", "admission", "persistence", None, false),
            Self::Client => ("gateway", "request", "client_configuration", None, false),
            Self::GatewayTransport(timeout) => (
                "gateway",
                "request",
                if *timeout { "timeout" } else { "transport" },
                None,
                false,
            ),
            Self::GatewayHttp(status) => {
                ("gateway", "response_status", "http", Some(*status), false)
            }
            Self::GatewayBody(status, error) => (
                "gateway",
                "response_body",
                error.kind(),
                Some(*status),
                false,
            ),
            Self::GatewayResponse(status) => (
                "gateway",
                "response_body",
                "invalid_response",
                Some(*status),
                false,
            ),
            Self::TelegramTransport(timeout) => (
                "telegram",
                "request",
                if *timeout { "timeout" } else { "transport" },
                None,
                true,
            ),
            Self::TelegramHttp(status) => {
                ("telegram", "response_status", "http", Some(*status), true)
            }
            Self::TelegramBody(status, error) => (
                "telegram",
                "response_body",
                error.kind(),
                Some(*status),
                true,
            ),
            Self::TelegramResponse(status) => (
                "telegram",
                "response_body",
                "invalid_response",
                Some(*status),
                true,
            ),
        };
        json!({"stage":stage,"phase":phase,"kind":kind,"http_status":status,"gateway_completed":completed})
    }
}

/// Start configured webhook-triggered SOPs. Run admission is not completion.
async fn dispatch(
    settings: &Arc<Settings>,
    event_id: &str,
    metadata: &Value,
    sops: &[String],
) -> Result<(), DispatchError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(DISPATCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| DispatchError::Client)?;
    for sop in sops {
        // SOP names have already passed the strict startup ASCII validation.
        let path = format!("/sop/webhook/{sop}");
        let response = client
            .post(format!("{}{path}", settings.gateway))
            .bearer_auth(&settings.token)
            .header("X-Idempotency-Key", event_id)
            .header("X-Webhook-Secret", &settings.gateway_secret)
            .json(metadata)
            .send()
            .await
            .map_err(|e| DispatchError::GatewayTransport(e.is_timeout()))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(DispatchError::GatewayHttp(status));
        }
        let run = read_sop_dispatch(response, sop, &path).await?;
        // Persist each successful admission before another dispatch or delivery.
        // A later failure must not erase references to work already admitted.
        let id = event_id.to_owned();
        async_state(settings.clone(), move |data| {
            let entry = &mut data["events"][&id];
            if entry.get("runs").is_none() {
                entry["runs"] = json!([]);
            }
            entry["runs"].as_array_mut().ok_or_else(failure)?.push(run);
            Ok(())
        })
        .await
        .map_err(|_| DispatchError::State)?;
    }
    let text = if sops.is_empty() {
        "An enabled email filter matched. No SOP is assigned; no agent task was started."
    } else {
        "Email SOP dispatch admitted. This is not a completion notice; inspect SOP runs for progress and results."
    };
    deliver_notification(&client, text, settings.telegram.as_ref()).await
}

async fn read_sop_dispatch(
    response: reqwest::Response,
    sop: &str,
    path: &str,
) -> Result<Value, DispatchError> {
    let status = response.status().as_u16();
    let value = bounded_json(response, 65_536)
        .await
        .map_err(|error| DispatchError::GatewayBody(status, error))?;
    let runs = value["results"]
        .as_array()
        .filter(|runs| runs.len() == 1)
        .ok_or(DispatchError::GatewayResponse(status))?;
    let run = &runs[0];
    let run_id = run["run_id"]
        .as_str()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or(DispatchError::GatewayResponse(status))?;
    if value["status"] != "accepted"
        || value["source"] != "webhook"
        || value["path"] != path
        || run["sop"] != sop
        || !matches!(run["status"].as_str(), Some("started" | "coalesced"))
    {
        return Err(DispatchError::GatewayResponse(status));
    }
    Ok(json!({"sop":sop,"run_id":run_id,"status":run["status"]}))
}

async fn deliver_notification(
    client: &reqwest::Client,
    text: &str,
    telegram: Option<&(String, String)>,
) -> Result<(), DispatchError> {
    let Some((owner, bot)) = telegram else {
        return Ok(());
    };
    let response = client
        .post(format!("https://api.telegram.org/bot{bot}/sendMessage"))
        .timeout(Duration::from_secs(20))
        .json(&json!({"chat_id":owner,"text":text}))
        .send()
        .await
        .map_err(|e| DispatchError::TelegramTransport(e.is_timeout()))?;
    if !response.status().is_success() {
        return Err(DispatchError::TelegramHttp(response.status().as_u16()));
    }
    validate_telegram_response(response).await
}

async fn validate_telegram_response(response: reqwest::Response) -> Result<(), DispatchError> {
    let status = response.status().as_u16();
    if bounded_json(response, 8192)
        .await
        .map_err(|error| DispatchError::TelegramBody(status, error))?["ok"]
        != true
    {
        return Err(DispatchError::TelegramResponse(status));
    }
    Ok(())
}

async fn fetch_message(settings: &Settings, message: &Value) -> io::Result<Value> {
    let inbox = message["inbox_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 254)
        .ok_or_else(failure)?;
    let id = message["message_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or_else(failure)?;
    let authorization = settings.authorization.as_deref().ok_or_else(failure)?;
    let mut url =
        reqwest::Url::parse("https://api.agentmail.to/v0/inboxes/").map_err(|_| failure())?;
    url.path_segments_mut()
        .map_err(|_| failure())?
        .pop_if_empty()
        .push(inbox)
        .push("messages")
        .push(id);
    // Deliberately ignore body_url. Only the fixed named-secret origin receives credentials.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| failure())?;
    let response = client
        .get(url)
        .header("Authorization", authorization)
        .send()
        .await
        .map_err(|_| failure())?;
    if !response.status().is_success() {
        return Err(failure());
    }
    let fetched = bounded_json(response, 1_048_576)
        .await
        .map_err(|_| failure())?;
    if fetched["inbox_id"] != message["inbox_id"] || fetched["message_id"] != message["message_id"]
    {
        return Err(failure());
    }
    Ok(fetched)
}

async fn proxy(
    mut req: Request<Incoming>,
    peer: std::net::SocketAddr,
    backend: &str,
    lease: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
) -> Result<Response<Body>, hyper::Error> {
    // Pairing-code secrets and shutdown remain host-only. Dashboard reload and
    // SOP routes use the patched native gateway's paired remote-admin policy.
    let path = req.uri().path();
    if ["/admin/shutdown", "/admin/paircode", "/admin/paircode/new"]
        .iter()
        .any(|route| path.ends_with(route))
    {
        return Ok(response(StatusCode::FORBIDDEN, "Local administration only"));
    }
    let upgrade = req.headers().get("upgrade").is_some();
    let client_upgrade = if upgrade {
        Some(hyper::upgrade::on(&mut req))
    } else {
        None
    };
    for h in [
        "forwarded",
        "x-forwarded-for",
        "x-real-ip",
        "x-gateway-admin-token",
        "x-zeroclaw-admin-token",
    ] {
        req.headers_mut().remove(h);
    }
    if let Ok(value) = peer.ip().to_string().parse() {
        req.headers_mut().insert("x-forwarded-for", value);
    }
    let Ok(socket) = TcpStream::connect(backend).await else {
        return Ok(response(StatusCode::BAD_GATEWAY, "Gateway unavailable"));
    };
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(socket)).await?;
    tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    });
    let mut result = sender.send_request(req).await?;
    if result.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client) = client_upgrade
    {
        let server = hyper::upgrade::on(&mut result);
        tokio::spawn(async move {
            let _lease = lease;
            if let (Ok(client), Ok(server)) = tokio::join!(client, server) {
                let _ = tokio::io::copy_bidirectional(
                    &mut TokioIo::new(client),
                    &mut TokioIo::new(server),
                )
                .await;
            }
        });
    }
    Ok(result.map(|body| body.map_err(|e| Box::new(e) as _).boxed_unsync()))
}

/// Own the one public listener and terminate the runtime when the supervisor exits.
pub fn supervise(mut command: Command, port: u16, settings: Settings) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    command
        .args([
            "daemon",
            "--host",
            BACKEND_HOST,
            "--port",
            &BACKEND_PORT.to_string(),
        ])
        .env("ZEROCLAW_gateway__host", BACKEND_HOST)
        .env("ZEROCLAW_gateway__port", BACKEND_PORT.to_string())
        .env("ZEROCLAW_gateway__trust_forwarded_headers", "true")
        .env("ZEROCLAW_gateway__paired_tokens", &settings.paired_tokens);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        if port == BACKEND_PORT { return Err(failure()) }
        let settings = Arc::new(settings);
        async_state(settings.clone(), |_| Ok(())).await?;
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;
        command.process_group(0);
        struct RuntimeChild(std::process::Child);
        impl Drop for RuntimeChild { fn drop(&mut self) {
            // SAFETY: child started its own process group before executing.
            unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL); }
            let _ = self.0.wait();
        }}
        let mut child = RuntimeChild(command.spawn()?);
        let connections = Arc::new(Semaphore::new(64));
        let jobs = Arc::new(Semaphore::new(2));
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop { tokio::select! {
            _ = term.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
            _ = tick.tick() => if child.0.try_wait()?.is_some() { return Err(io::Error::other("ZeroClaw exited")) },
            incoming = listener.accept() => {
                let (stream, peer) = incoming?;
                let Ok(permit) = connections.clone().try_acquire_owned() else { continue };
                let settings = settings.clone(); let jobs = jobs.clone();
                tokio::spawn(async move {
                    let permit = Arc::new(permit);
                    let service = service_fn(move |req: Request<Incoming>| {
                        let settings = settings.clone(); let jobs = jobs.clone();
                        let permit = permit.clone();
                        async move {
                            if req.uri().path() == ROUTE { Ok(receive(req, settings, jobs).await) }
                            else { proxy(req, peer, &format!("{BACKEND_HOST}:{BACKEND_PORT}"), Some(permit)).await }
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new().timer(hyper_util::rt::TokioTimer::new()).header_read_timeout(Duration::from_secs(10)).max_buf_size(32768).serve_connection(TokioIo::new(stream), service).with_upgrades().await;
                });
            }
        }}
        // Stop and reap before leaving the runtime; no detached daemon survives.
        drop(child); Ok(())
    })
}

/// Narrow management tool: durable enable/disable state, never arbitrary paths.
pub fn serve_mcp() -> io::Result<()> {
    let settings = Settings::from_env();
    let mut input = io::stdin().lock();
    loop {
        let mut line = Vec::new();
        if (&mut input).take(4097).read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if line.len() > 4096 {
            return Err(failure());
        }
        let req: Value = serde_json::from_slice(&line).map_err(|_| failure())?;
        let Some(id) = req.get("id") else { continue };
        if req["jsonrpc"] != "2.0" || !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Err(failure());
        }
        let result = match req["method"].as_str() {
            Some("initialize") => {
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"heimdall-mail","version":"0.1.0"}})
            }
            Some("ping") => json!({}),
            Some("tools/list") => json!({"tools":[
                {"name":"filters","description":"List configured email filters, SOP assignments, enabled state and the ten most recent sanitized event diagnostics. Event references are SHA-256 hashes of event IDs. No secrets or email content returned.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
                {"name":"set_enabled","description":"Enable or disable a configured email filter when the operator requests it. Does not configure Railway ingress or credentials.","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"enabled":{"type":"boolean"}},"required":["name","enabled"],"additionalProperties":false}}
            ]}),
            Some("tools/call") => {
                let args = &req["params"]["arguments"];
                let name = req["params"]["name"].as_str().unwrap_or("");
                let valid = match name {
                    "filters" => args.as_object().is_some_and(|a| a.is_empty()),
                    "set_enabled" => {
                        args.as_object().is_some_and(|a| a.len() == 2)
                            && args["name"].as_str().is_some_and(valid_id)
                            && args["enabled"].is_boolean()
                    }
                    _ => false,
                };
                if !valid {
                    writeln!(
                        io::stdout().lock(),
                        "{}",
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Use filters with no arguments, or set_enabled with name and enabled"}})
                    )?;
                    continue;
                }
                let output = match &settings {
                    Ok(Some(settings)) => manage(settings, name, args),
                    Ok(None) => Ok(json!({"configured":false,"hint":"Email ingress is disabled in the template"}).to_string()),
                    Err(_) => Err(io::Error::other("Email ingress configuration is invalid or credentials are missing")),
                };
                match output {
                    Ok(text) => json!({"content":[{"type":"text","text":text}]}),
                    Err(_) => {
                        json!({"isError":true,"content":[{"type":"text","text":"Invalid email configuration, missing credentials, or unavailable admission state"}]})
                    }
                }
            }
            _ => {
                writeln!(
                    io::stdout().lock(),
                    "{}",
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unknown method"}})
                )?;
                continue;
            }
        };
        writeln!(
            io::stdout().lock(),
            "{}",
            json!({"jsonrpc":"2.0","id":id,"result":result})
        )?;
    }
    Ok(())
}

fn manage(settings: &Settings, name: &str, args: &Value) -> io::Result<String> {
    state(&settings.state, |data| {
        match name {
            "filters" if args.as_object().is_some_and(|a| a.is_empty()) => {}
            "set_enabled" if args.as_object().is_some_and(|a| a.len() == 2) => {
                let rule = args["name"]
                    .as_str()
                    .filter(|n| settings.filters.iter().any(|f| f.name == *n))
                    .ok_or_else(failure)?;
                let enabled = args["enabled"].as_bool().ok_or_else(failure)?;
                data["enabled"][rule] = json!(enabled);
            }
            _ => return Err(failure()),
        }
        let events = data["events"].as_object().ok_or_else(failure)?;
        let mut recent: Vec<_> = events.iter().map(|(id, event)| json!({"event_ref":sha256_hex(id),"time":event["time"],"status":event["status"],"diagnostic":event.get("diagnostic").cloned().unwrap_or(Value::Null),"runs":event.get("runs").cloned().unwrap_or_else(|| json!([]))})).collect();
        recent.sort_by_key(|event| std::cmp::Reverse(event["time"].as_u64().unwrap_or(0)));
        recent.truncate(RECENT_EVENT_LIMIT);
        Ok(json!({"configured":true,"recent_events":recent,"events":{"total":events.len(),"uncertain":events.values().filter(|v| v["status"] == "uncertain").count(),"reserved":events.values().filter(|v| v["status"] == "reserved").count()},"filters":settings.filters.iter().map(|f| json!({"name":f.name,"sender":f.sender,"contains":f.contains,"sop":f.sop,"enabled":data["enabled"][&f.name].as_bool().unwrap_or(false)})).collect::<Vec<_>>()}).to_string())
    })
}

#[cfg(test)]
#[path = "agentmail/tests.rs"]
mod tests;
