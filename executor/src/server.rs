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
pub(crate) async fn execute_formatted(app: App, job: Job, rpc_id: serde_json::Value) -> Response {
    if let Err(error) = job.validate() {
        return (StatusCode::BAD_REQUEST, error).into_response();
    }
    if !app.ready {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let cancel = CancellationToken::new();
    let permit = {
        let mut active = app.active.lock().unwrap();
        if active.contains_key(&job.id) {
            return StatusCode::CONFLICT.into_response();
        }
        let Ok(permit) = app.gate.clone().try_acquire_owned() else {
            return StatusCode::LOCKED.into_response();
        };
        active.insert(job.id.clone(), cancel.clone());
        permit
    };
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    tokio::spawn(async move {
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
        app.active.lock().unwrap().remove(&id);
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
    #[test]
    fn private_listener_accepts_ipv6_addressing() {
        let address = listen_address("3889").unwrap();
        assert!(address.is_ipv6());
        assert!(address.ip().is_unspecified());
        assert_eq!(address.port(), 3889);
    }
    #[tokio::test]
    async fn active_duplicate_and_capacity_have_distinct_statuses() {
        let app = App::new("x".repeat(32), true);
        let _permit = app.gate.clone().acquire_many_owned(2).await.unwrap();
        app.active
            .lock()
            .unwrap()
            .insert("active".into(), CancellationToken::new());
        for (id, expected) in [
            ("active", StatusCode::CONFLICT),
            ("other", StatusCode::LOCKED),
        ] {
            let reply = execute_formatted(
                app.clone(),
                Job {
                    id: id.into(),
                    tool: "bash".into(),
                    arguments: serde_json::json!({"command":"true"}),
                },
                serde_json::json!(id),
            )
            .await;
            assert_eq!(reply.status(), expected);
        }
    }
}
