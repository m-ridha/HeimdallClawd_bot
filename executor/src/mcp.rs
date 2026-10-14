//! Stateless, authenticated MCP adapter over the existing execution supervisor.
use crate::{
    protocol::Job,
    server::{App, execute_formatted},
};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
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
fn tools() -> Value {
    json!([
    {"name":"bash","description":"Run a bounded command in the separate isolated workspace. Networking and control credentials are unavailable.","inputSchema":{"type":"object","properties":{"command":{"type":"string","minLength":1,"maxLength":32768}},"required":["command"],"additionalProperties":false}},
    {"name":"file_read","description":"Read a UTF-8 file relative to the isolated workspace, at most 64 KiB.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}},
    {"name":"file_write","description":"Write a file relative to the isolated workspace, at most 64 KiB. This remote workspace is writable.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string","maxLength":65536}},"required":["path","content"],"additionalProperties":false}}
    ])
}
pub(crate) async fn handle(State(app): State<App>, Json(request): Json<Value>) -> Response {
    if !app.is_ready() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if !request.is_object() || request["jsonrpc"] != "2.0" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let method = request["method"].as_str().unwrap_or("");
    let id = request.get("id").cloned();
    if id.is_none() {
        return match method {
            "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
            "notifications/cancelled" => {
                if let Some(key) = job_id(&request["params"]["requestId"]) {
                    app.cancel(&key);
                }
                StatusCode::ACCEPTED.into_response()
            }
            _ => StatusCode::BAD_REQUEST.into_response(),
        };
    }
    let id = id.unwrap();
    let Some(key) = job_id(&id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match method {
  "initialize"=>Json(json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"heimdall-workspace","version":env!("CARGO_PKG_VERSION")}}})).into_response(),
  "ping"=>Json(json!({"jsonrpc":"2.0","id":id,"result":{}})).into_response(),
  "tools/list"=>Json(json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}})).into_response(),
  "tools/call"=>{
   let params=&request["params"];
   let job=Job{id:key,tool:params["name"].as_str().unwrap_or("").into(),arguments:params["arguments"].clone()};
   if let Err(message)=job.validate() {return error(id,-32602,message);}
   let allowed=match job.tool.as_str(){"bash"=>&["command"][..],"file_read"=>&["path"][..],_=>&["path","content"][..]};
   if job.arguments.as_object().unwrap().keys().any(|k|!allowed.contains(&k.as_str())) {return error(id,-32602,"Unknown tool argument");}
   let response=execute_formatted(app,job,id.clone()).await;
   if !response.status().is_success(){return error(id,-32000,"Workspace executor is busy or unavailable");}
   response
  },
  _=>error(id,-32601,"Unknown method")
 }
}
