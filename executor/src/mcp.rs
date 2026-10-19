//! Authenticated MCP adapter with client-scoped execution and cancellation.
use crate::{
    protocol::{COMMAND_LIMIT, Job, LIMIT},
    server::{App, execute_formatted},
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
fn error(id: Value, code: i32, message: &str) -> Response {
    Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})).into_response()
}
fn job_id(id: &Value) -> Option<String> {
    if !(id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()) {
        return None;
    }
    let key = format!("mcp:{}", id);
    (key.len() <= 128).then_some(key)
}
const SESSION_HEADER: &str = "mcp-session-id";
fn session(headers: &HeaderMap) -> Result<Option<uuid::Uuid>, ()> {
    headers
        .get(SESSION_HEADER)
        .map(|value| {
            let text = value.to_str().map_err(|_| ())?;
            let id = uuid::Uuid::parse_str(text).map_err(|_| ())?;
            if id.to_string() != text {
                return Err(());
            }
            Ok(id)
        })
        .transpose()
}
fn scoped_key(session: uuid::Uuid, id: &Value) -> Option<String> {
    job_id(id).map(|key| format!("{session}:{key}"))
}
fn tools() -> Value {
    json!([
    {"name":"bash","description":"Run a command in the writable isolated /data/workspace executor: up to 3 seconds admission wait, then 30-second execution timeout, 64 KiB combined output, two concurrent jobs. Networking and control credentials unavailable. Use relative paths; include cd in the same command because each call is a fresh process. Bash result text is JSON with exit_code (null for signals), termination_reason (completed, timeout, cancelled, output_limit), truncated, stdout and stderr. Only completed with exit_code 0 is success; output shares one raw-byte limit. Launch failures remain generic errors.","inputSchema":{"type":"object","properties":{"command":{"type":"string","minLength":1,"maxLength":COMMAND_LIMIT}},"required":["command"],"additionalProperties":false}},
    {"name":"file_read","description":"Read a UTF-8 file relative to the isolated workspace, at most 64 KiB.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}},
    {"name":"file_read_base64","description":"Read a binary file as base64, at most 48 KiB.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}},
    {"name":"file_write_base64","description":"Write a base64-encoded binary file, at most 48 KiB decoded.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string","maxLength":LIMIT}},"required":["path","content"],"additionalProperties":false}},
    {"name":"file_write","description":"Write a file relative to the isolated workspace, at most 64 KiB. This remote workspace is writable.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string","maxLength":LIMIT}},"required":["path","content"],"additionalProperties":false}}
    ])
}
pub(crate) async fn handle(
    State(app): State<App>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if !app.is_ready() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !request.is_object() || request["jsonrpc"] != "2.0" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(session) = session(&headers) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let method = request["method"].as_str().unwrap_or("");
    let id = request.get("id").cloned();
    if id.is_none() {
        return match method {
            "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
            "notifications/cancelled" => {
                if let Some(key) =
                    session.and_then(|session| scoped_key(session, &request["params"]["requestId"]))
                {
                    app.cancel(&key);
                }
                StatusCode::ACCEPTED.into_response()
            }
            _ => StatusCode::BAD_REQUEST.into_response(),
        };
    }
    let id = id.unwrap();
    let Some(_) = job_id(&id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match method {
  "initialize"=> ([(SESSION_HEADER, uuid::Uuid::new_v4().to_string())], Json(json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"heimdall-workspace","version":env!("CARGO_PKG_VERSION")}}}))).into_response(),
  "ping"=>Json(json!({"jsonrpc":"2.0","id":id,"result":{}})).into_response(),
  "tools/list"=>Json(json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}})).into_response(),
  "tools/call"=>{
   let params=&request["params"];
   // Sessionless legacy clients get independent calls; bare-ID cancellation
   // is intentionally ignored. Response disconnect still cancels the job.
   let key = session.and_then(|session| scoped_key(session, &id))
       .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
   let job=Job{id:uuid::Uuid::new_v4().to_string(),tool:params["name"].as_str().unwrap_or("").into(),arguments:params["arguments"].clone()};
   if let Err(message)=job.validate() {return error(id,-32602,message);}
   let allowed=match job.tool.as_str(){"bash"=>&["command"][..],"file_read"|"file_read_base64"=>&["path"][..],_=>&["path","content"][..]};
   if job.arguments.as_object().unwrap().keys().any(|k|!allowed.contains(&k.as_str())) {return error(id,-32602,"Unknown tool argument");}
   let response=execute_formatted(app,job,id.clone(),key).await;
   if !response.status().is_success(){
    let (code,message)=match response.status(){
     StatusCode::LOCKED=>(-32001,"executor_busy: two jobs are active; wait before retrying this unstarted request"),
     StatusCode::CONFLICT=>(-32002,"duplicate_request: this request ID is active in this client session; use a distinct ID or await completion"),
     StatusCode::SERVICE_UNAVAILABLE=>(-32003,"executor_unavailable: isolation readiness is not established; ask the operator to check executor health"),
     _=>(-32000,"executor_failed: request did not start; check executor health")
    };
    return error(id,code,message);
   }
   response
  },
  _=>error(id,-32601,"Unknown method")
 }
}
