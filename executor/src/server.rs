use crate::protocol::{BODY_LIMIT, Job};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{Request, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::{Semaphore, mpsc};
const ADMISSION_WAIT: Duration = Duration::from_secs(3);
use tokio_util::sync::CancellationToken;
#[derive(Clone)]
pub struct App {
    token: Arc<String>,
    ready: bool,
    gate: Arc<Semaphore>,
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
}
impl App {
    pub(crate) fn is_ready(&self) -> bool {
        self.ready
    }
    pub(crate) fn cancel(&self, id: &str) -> bool {
        if let Some(token) = self.active.lock().unwrap().get(id) {
            token.cancel();
            true
        } else {
            false
        }
    }
    pub fn new(token: String, ready: bool) -> Self {
        Self {
            token: Arc::new(token),
            ready,
            gate: Arc::new(Semaphore::new(2)),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
async fn authenticate(State(app): State<App>, request: Request<Body>, next: Next) -> Response {
    let expected = format!("Bearer {}", app.token);
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if supplied.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() != 1 {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}
pub fn router(app: App) -> Router {
    Router::new()
        .route("/mcp", post(crate::mcp::handle))
        .route_layer(middleware::from_fn_with_state(app.clone(), authenticate))
        .route("/health", get(health))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(app)
}
async fn health(State(app): State<App>) -> StatusCode {
    if app.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
// Owned by the spawned task before its first poll, including abort/unwind paths.
struct ActiveJob {
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
    key: String,
    cancel: CancellationToken,
}
impl Drop for ActiveJob {
    fn drop(&mut self) {
        self.cancel.cancel();
        // Cleanup must not panic while unwinding if another task poisoned the map.
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.key);
    }
}
async fn acquire_slot(
    gate: Arc<Semaphore>,
    cancel: &CancellationToken,
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<tokio::sync::OwnedSemaphorePermit, (i32, &'static str)> {
    tokio::select! {
        biased;
        _ = tx.closed() => Err((-32800, "executor_cancelled: queued request did not start")),
        _ = cancel.cancelled() => Err((-32800, "executor_cancelled: queued request did not start")),
        result = tokio::time::timeout(ADMISSION_WAIT, gate.acquire_owned()) => {
            match result {
                Ok(Ok(permit)) => Ok(permit),
                _ => Err((-32001, "executor_busy: admission wait expired; request did not start")),
            }
        }
    }
}
pub(crate) async fn execute_formatted(
    app: App,
    job: Job,
    rpc_id: serde_json::Value,
    key: String,
) -> Response {
    if let Err(error) = job.validate() {
        return (StatusCode::BAD_REQUEST, error).into_response();
    }
    if !app.ready {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let cancel = CancellationToken::new();
    {
        let mut active = app.active.lock().unwrap();
        if active.contains_key(&key) {
            return StatusCode::CONFLICT.into_response();
        }
        active.insert(key.clone(), cancel.clone());
    }
    let guard = ActiveJob {
        active: app.active.clone(),
        key,
        cancel: cancel.clone(),
    };
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    tokio::spawn(async move {
        // Return the response stream before waiting so disconnects can cancel
        // queued work. No active-map mutex is held across an await.
        let admission = acquire_slot(app.gate.clone(), &cancel, &tx).await;
        let permit = match admission {
            Ok(permit) => permit,
            Err((code, message)) => {
                drop(guard);
                let bytes = serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":rpc_id,"error":{"code":code,"message":message}})).unwrap();
                let _ =
                    tokio::time::timeout(Duration::from_secs(2), tx.send(Ok(Bytes::from(bytes))))
                        .await;
                return;
            }
        };
        if tx.is_closed() || cancel.is_cancelled() {
            return;
        }
        let id = job.id.clone();
        let execution = crate::process::run(job, cancel.clone(), Duration::from_secs(30));
        tokio::pin!(execution);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(500));
        let mut disconnected = false;
        let reply = loop {
            tokio::select! {
                result=&mut execution=>break result,
                _=tx.closed(), if !disconnected=>{disconnected=true;cancel.cancel();},
                _=heartbeat.tick()=>{if tx.try_send(Ok(Bytes::from_static(b"\n"))).is_err() && tx.is_closed(){cancel.cancel();}}
            }
        };
        let reply = reply.unwrap_or_else(|_| crate::protocol::Reply {
            id: id.clone(),
            content: "Executor restrictions or process launch failed".into(),
            is_error: true,
            truncated: false,
        });
        let bytes = serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":rpc_id,"result":{"content":[{"type":"text","text":reply.content}],"isError":reply.is_error}})).unwrap();
        drop(guard);
        drop(permit);
        let _ = tokio::time::timeout(Duration::from_secs(2), tx.send(Ok(Bytes::from(bytes)))).await;
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
        .unwrap()
}

/// The private service must support IPv6 as well as IPv4 health checks.
pub fn listen_address(port: &str) -> std::io::Result<std::net::SocketAddr> {
    format!("[::]:{port}")
        .parse()
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn queued_admission_succeeds_when_a_slot_is_released() {
        let gate = Arc::new(Semaphore::new(2));
        let occupied = gate.clone().acquire_many_owned(2).await.unwrap();
        let cancel = CancellationToken::new();
        let (tx, _rx) = mpsc::channel(2);
        let (permit, ()) = tokio::join!(acquire_slot(gate.clone(), &cancel, &tx), async {
            drop(occupied);
        });
        let permit = permit.unwrap();
        assert_eq!(gate.available_permits(), 1);
        drop(permit);
        assert_eq!(gate.available_permits(), 2);
    }
    #[tokio::test]
    async fn queued_requests_cancel_on_disconnect_or_notification_without_starting() {
        use http_body_util::BodyExt;
        let app = App::new("x".repeat(32), true);
        let _occupied = app.gate.clone().acquire_many_owned(2).await.unwrap();
        for disconnect in [true, false] {
            let key = "queued".to_string();
            let response = execute_formatted(
                app.clone(),
                Job {
                    id: "job".into(),
                    tool: "bash".into(),
                    arguments: serde_json::json!({"command":"true"}),
                },
                serde_json::json!(3),
                key.clone(),
            )
            .await;
            assert!(app.active.lock().unwrap().contains_key(&key));
            if disconnect {
                drop(response);
            } else {
                assert!(app.cancel(&key));
                let body = response.into_body().collect().await.unwrap().to_bytes();
                let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(value["error"]["code"], -32800);
            }
            tokio::time::timeout(Duration::from_secs(2), async {
                while app.active.lock().unwrap().contains_key(&key) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(app.gate.available_permits(), 0);
        }
    }
    #[tokio::test]
    async fn active_guard_cleans_panic_and_abort_and_allows_id_reuse() {
        let app = App::new("x".repeat(32), true);
        for panic in [true, false] {
            let cancel = CancellationToken::new();
            assert!(
                app.active
                    .lock()
                    .unwrap()
                    .insert("reusable".into(), cancel.clone())
                    .is_none()
            );
            let guard = ActiveJob {
                active: app.active.clone(),
                key: "reusable".into(),
                cancel: cancel.clone(),
            };
            let permit = app.gate.clone().acquire_owned().await.unwrap();
            let task = tokio::spawn(async move {
                let (_guard, _permit) = (guard, permit);
                if panic {
                    panic!("synthetic task panic");
                }
                std::future::pending::<()>().await;
            });
            if !panic {
                task.abort();
            }
            let error = task.await.unwrap_err();
            assert_eq!(error.is_panic(), panic);
            assert!(cancel.is_cancelled());
            assert!(!app.active.lock().unwrap().contains_key("reusable"));
            assert_eq!(app.gate.available_permits(), 2);
        }
    }
    #[tokio::test]
    async fn cancellation_is_session_scoped_and_sessionless_notifications_are_ignored() {
        let app = App::new("x".repeat(32), true);
        let first = CancellationToken::new();
        let second = CancellationToken::new();
        app.active.lock().unwrap().insert(
            "00000000-0000-4000-8000-000000000001:mcp:3".into(),
            first.clone(),
        );
        app.active.lock().unwrap().insert(
            "00000000-0000-4000-8000-000000000002:mcp:3".into(),
            second.clone(),
        );
        let request = serde_json::json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}});
        let response = crate::mcp::handle(
            State(app.clone()),
            axum::http::HeaderMap::new(),
            axum::Json(request.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(!first.is_cancelled() && !second.is_cancelled());
        let headers = axum::http::HeaderMap::from_iter([(
            axum::http::HeaderName::from_static("mcp-session-id"),
            axum::http::HeaderValue::from_static("00000000-0000-4000-8000-000000000001"),
        )]);
        let response = crate::mcp::handle(State(app), headers, axum::Json(request)).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
    }
    #[test]
    fn private_listener_accepts_ipv6_addressing() {
        let address = listen_address("3889").unwrap();
        assert!(address.is_ipv6());
        assert!(address.ip().is_unspecified());
        assert_eq!(address.port(), 3889);
    }
    #[tokio::test]
    async fn mcp_distinguishes_busy_and_duplicate_without_starting_jobs() {
        use http_body_util::BodyExt;
        let app = App::new("x".repeat(32), true);
        let _permit = app.gate.clone().acquire_many_owned(2).await.unwrap();
        app.active.lock().unwrap().insert(
            "00000000-0000-4000-8000-000000000001:mcp:1".into(),
            CancellationToken::new(),
        );
        for (id, expected) in [(1, -32002), (2, -32001)] {
            let response = crate::mcp::handle(State(app.clone()), axum::http::HeaderMap::from_iter([(axum::http::HeaderName::from_static("mcp-session-id"), axum::http::HeaderValue::from_static("00000000-0000-4000-8000-000000000001"))]), axum::Json(serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"bash","arguments":{"command":"true"}}}))).await;
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["error"]["code"], expected);
            assert!(!value.to_string().contains("x".repeat(32).as_str()));
        }
    }
    #[tokio::test]
    async fn active_duplicate_and_capacity_have_distinct_statuses() {
        let app = App::new("x".repeat(32), true);
        let _permit = app.gate.clone().acquire_many_owned(2).await.unwrap();
        app.active
            .lock()
            .unwrap()
            .insert("active".into(), CancellationToken::new());
        for (id, expected) in [("active", StatusCode::CONFLICT), ("other", StatusCode::OK)] {
            let reply = execute_formatted(
                app.clone(),
                Job {
                    id: id.into(),
                    tool: "bash".into(),
                    arguments: serde_json::json!({"command":"true"}),
                },
                serde_json::json!(id),
                id.into(),
            )
            .await;
            assert_eq!(reply.status(), expected);
        }
    }
}
