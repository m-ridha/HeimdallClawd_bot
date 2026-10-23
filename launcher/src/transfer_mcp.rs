//! Bounded Telegram file transfer; local workspace access and explicit recipient authorization.
use reqwest::blocking::{Client, Response, multipart};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path};
use std::time::Duration;
const LIMIT: usize = 49152;
const WORKSPACE: &str = "/data/zeroclaw/agents/primary/workspace";
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\0')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
fn read_attachment(source: &str) -> Result<Vec<u8>, &'static str> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::fs::OpenOptionsExt;
    const ROOT: &str = "/data/zeroclaw/agents/primary/workspace/telegram_files";
    let relative = Path::new(source)
        .strip_prefix(ROOT)
        .ok()
        .and_then(|p| p.to_str())
        .filter(|p| valid_path(p))
        .ok_or("Use a Telegram attachment path inside telegram_files")?;
    let root = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(ROOT)
        .map_err(|_| "Telegram attachment directory unavailable")?;
    let mut current: OwnedFd = root.into();
    let components: Vec<_> = Path::new(relative).components().collect();
    for (i, part) in components.iter().enumerate() {
        let name = std::ffi::CString::new(part.as_os_str().as_encoded_bytes())
            .map_err(|_| "Invalid attachment path")?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if i + 1 < components.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: owned live descriptor and NUL-terminated component; symlinks are rejected.
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err("Cannot open Telegram attachment");
        }
        // SAFETY: successful openat returned a new owned descriptor.
        current = unsafe { OwnedFd::from_raw_fd(fd) };
    }
    let file = std::fs::File::from(current);
    if !file
        .metadata()
        .map_err(|_| "Cannot inspect attachment")?
        .is_file()
    {
        return Err("Attachment must be a regular file");
    }
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read attachment")?;
    if bytes.len() > LIMIT {
        return Err("Transfer exceeds 48 KiB");
    }
    Ok(bytes)
}
fn status_error(status: u16) -> Option<&'static str> {
    match status {
        200..=299 => None,
        401 => Some("transfer_authentication: check the service credential"),
        403 => {
            Some("transfer_permission_or_policy: check recipient/service access and network policy")
        }
        404 => Some("transfer_not_found: check the file or service resource"),
        409 => Some("transfer_duplicate_request: inspect the original operation before retrying"),
        423 => Some("transfer_busy: wait before retrying this unstarted transfer request"),
        429 => Some("transfer_rate_limit: wait and check delivery state before resending"),
        500..=599 => Some(
            "transfer_service_unavailable: check service health and operation state before retrying",
        ),
        _ => Some("transfer_rejected: check request parameters and service configuration"),
    }
}
fn transport_error(error: reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "transfer_timeout: completion is unknown; check delivery or file state before retrying"
    } else if error.is_connect() {
        "transfer_network: check DNS, egress and service reachability"
    } else {
        "transfer_transport: request failed; check service health and operation state"
    }
}
fn value(response: Response) -> Result<Value, &'static str> {
    if let Some(error) = status_error(response.status().as_u16()) {
        return Err(error);
    }
    let mut bytes = Vec::new();
    response
        .take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| "Transfer response failed")?;
    if bytes.len() > 131072 {
        return Err("Transfer response exceeded the limit");
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid transfer service response")?;
    if value["ok"] == false {
        let code = value["error_code"]
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(0);
        return Err(
            status_error(code).unwrap_or("transfer_rejected: service rejected the operation")
        );
    }
    Ok(value)
}
struct Bridge {
    client: Client,
    token: String,
    peers: Vec<String>,
}
impl Bridge {
    fn new() -> Result<Self, &'static str> {
        let owner = std::env::var("TELEGRAM_OWNER_ID").map_err(|_| "Telegram owner is required")?;
        let allowed = std::env::var("TELEGRAM_ALLOWED_USER_IDS").ok();
        let peers = crate::config::parse_allowed_ids(&owner, allowed.as_deref())?;
        Ok(Self {
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "Cannot initialize transfer client")?,
            token: std::env::var("TELEGRAM_BOT_TOKEN").map_err(|_| "Telegram token is required")?,
            peers,
        })
    }
    fn workspace(&self) -> Result<crate::workspace::Workspace, &'static str> {
        crate::workspace::Workspace::open(Path::new(WORKSPACE)).map_err(|_| "Workspace unavailable")
    }
    fn api(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.token)
    }
    fn call(&self, tool: &str, args: &Value) -> Result<String, &'static str> {
        let path = args["path"]
            .as_str()
            .filter(|p| valid_path(p))
            .ok_or("Use a relative workspace path without traversal")?;
        match tool {
            "send" => {
                let recipient = args["recipient"]
                    .as_str()
                    .ok_or("An explicit Telegram recipient is required")?;
                if !self.peers.iter().any(|p| p == recipient) {
                    return Err("Recipient is outside the Telegram allowlist");
                }
                let bytes = self
                    .workspace()?
                    .read_binary(path)
                    .map_err(|_| "Cannot read workspace file; check path and 48 KiB limit")?;
                if bytes.len() > LIMIT {
                    return Err("Transfer exceeds 48 KiB");
                }
                let filename = Path::new(path)
                    .file_name()
                    .and_then(|v| v.to_str())
                    .ok_or("Invalid filename")?;
                let file = multipart::Part::bytes(bytes).file_name(filename.to_owned());
                let form = multipart::Form::new()
                    .text("chat_id", recipient.to_owned())
                    .part("document", file);
                let result = value(
                    self.client
                        .post(self.api("sendDocument"))
                        .multipart(form)
                        .send()
                        .map_err(transport_error)?,
                )?;
                if result["ok"] != true {
                    return Err("Telegram rejected the upload");
                }
                Ok("File delivered to the allowed Telegram recipient".into())
            }
            "import" => {
                let source = args["source"]
                    .as_str()
                    .ok_or("A Telegram attachment source path is required")?;
                let bytes = read_attachment(source)?;
                self.workspace()?
                    .write_binary(path, &bytes)
                    .map_err(|_| "Cannot write workspace file; check path and 48 KiB limit")?;
                Ok("Telegram attachment copied into the agent workspace".into())
            }
            "receive" => {
                let file_id = args["file_id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 512)
                    .ok_or("A Telegram file_id is required")?;
                let result = value(
                    self.client
                        .post(self.api("getFile"))
                        .json(&json!({"file_id":file_id}))
                        .send()
                        .map_err(transport_error)?,
                )?;
                if result["ok"] != true {
                    return Err("Telegram rejected the file lookup");
                }
                if result["result"]["file_size"]
                    .as_u64()
                    .is_some_and(|n| n > LIMIT as u64)
                {
                    return Err("Transfer exceeds 48 KiB");
                }
                let remote = result["result"]["file_path"]
                    .as_str()
                    .filter(|p| valid_path(p) && !p.contains(['?', '#']))
                    .ok_or("Invalid Telegram file path")?;
                let response = self
                    .client
                    .get(format!(
                        "https://api.telegram.org/file/bot{}/{}",
                        self.token, remote
                    ))
                    .send()
                    .map_err(transport_error)?;
                if let Some(error) = status_error(response.status().as_u16()) {
                    return Err(error);
                }
                let mut bytes = Vec::new();
                response
                    .take((LIMIT + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "transfer_read_failed: response body could not be read")?;
                if bytes.len() > LIMIT {
                    return Err("Transfer exceeds 48 KiB");
                }
                self.workspace()?
                    .write_binary(path, &bytes)
                    .map_err(|_| "Cannot write workspace file; check path and 48 KiB limit")?;
                Ok("File saved in the agent workspace".into())
            }
            _ => Err("Unknown transfer tool"),
        }
    }
}
fn handle(
    request: Value,
    run: &mut impl FnMut(&str, &Value) -> Result<String, &'static str>,
) -> Option<Value> {
    let id = request.get("id")?.clone();
    if request["jsonrpc"] != "2.0" || !(id.is_string() || id.is_i64() || id.is_u64()) {
        return Some(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"Invalid request"}}),
        );
    }
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"heimdall-transfer","version":"0.1.0"}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":[
            {"name":"send","description":"Send an agent workspace file (up to 48 KiB) as a Telegram document to an allowed user. An explicit allowed recipient is required; use the requesting private chat identity. Paths are relative to /data/zeroclaw/agents/primary/workspace.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"recipient":{"type":"string"}},"required":["path","recipient"],"additionalProperties":false}},
            {"name":"import","description":"Copy a Telegram attachment already downloaded under the agent workspace telegram_files directory into the agent workspace (up to 48 KiB). Use the source path shown in the incoming Telegram message.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"source":{"type":"string"}},"required":["path","source"],"additionalProperties":false}},
            {"name":"receive","description":"Download a Telegram file_id into the agent workspace (up to 48 KiB).","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"file_id":{"type":"string"}},"required":["path","file_id"],"additionalProperties":false}}
        ]}),
        "tools/call" => {
            let tool = request["params"]["name"].as_str().unwrap_or("");
            let args = &request["params"]["arguments"];
            let allowed = match tool {
                "send" => &["path", "recipient"][..],
                "receive" => &["path", "file_id"][..],
                "import" => &["path", "source"][..],
                _ => &[],
            };
            let valid = !allowed.is_empty()
                && args.as_object().is_some_and(|a| {
                    a.len() == allowed.len()
                        && allowed
                            .iter()
                            .all(|key| a.get(*key).is_some_and(Value::is_string))
                });
            let (text, failed) = match if valid {
                run(tool, args)
            } else {
                Err(match tool {
                    "send" => {
                        "transfer_invalid_arguments: send requires string path and explicit recipient, with no extra fields"
                    }
                    "receive" => {
                        "transfer_invalid_arguments: receive requires string path and file_id, with no extra fields"
                    }
                    "import" => {
                        "transfer_invalid_arguments: import requires string path and source, with no extra fields"
                    }
                    _ => "transfer_unknown_tool: use send, receive or import",
                })
            } {
                Ok(text) => (text, false),
                Err(error) => (error.to_owned(), true),
            };
            json!({"content":[{"type":"text","text":text}],"isError":failed})
        }
        _ => {
            return Some(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unknown method"}}),
            );
        }
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
pub fn serve() -> io::Result<()> {
    let mut bridge = None;
    let mut run = |tool: &str, args: &Value| {
        if bridge.is_none() {
            bridge = Some(Bridge::new()?);
        }
        bridge.as_ref().unwrap().call(tool, args)
    };
    let mut reader = io::stdin().lock();
    let mut out = io::stdout().lock();
    serve_stream(&mut reader, &mut out, &mut run)
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
}
fn serve_stream(
    reader: &mut impl BufRead,
    out: &mut impl Write,
    run: &mut impl FnMut(&str, &Value) -> Result<String, &'static str>,
) -> io::Result<()> {
    const REQUEST_LIMIT: usize = 8192;
    loop {
        let mut line = Vec::new();
        if reader
            .by_ref()
            .take((REQUEST_LIMIT + 1) as u64)
            .read_until(b'\n', &mut line)?
            == 0
        {
            return Ok(());
        }
        let response = if line.len() > REQUEST_LIMIT {
            if !line.ends_with(b"\n") {
                reader.skip_until(b'\n')?;
            }
            Some(error(Value::Null, -32700, "Transfer request exceeds 8 KiB"))
        } else {
            match serde_json::from_slice(&line) {
                Ok(request) => handle(request, run),
                Err(_) => Some(error(Value::Null, -32700, "Invalid JSON")),
            }
        };
        if let Some(response) = response {
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejected_http_response_never_exposes_body_or_url() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 4096];
            let size = stream.read(&mut request).unwrap();
            assert!(size > 0);
            stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 29\r\nConnection: close\r\n\r\nTEST_secret user-private-data").unwrap();
        });
        let response = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
            .get(format!("http://{address}/private-fixture"))
            .send()
            .unwrap();
        let error = value(response).unwrap_err();
        server.join().unwrap();
        assert!(error.starts_with("transfer_permission_or_policy:"));
        for private in [
            "TEST_secret",
            "user-private-data",
            "private-fixture",
            "127.0.0.1",
        ] {
            assert!(!error.contains(private));
        }
    }
    #[test]
    fn transfer_statuses_have_distinct_safe_recovery_messages() {
        assert_eq!(status_error(200), None);
        assert!(
            status_error(401)
                .unwrap()
                .starts_with("transfer_authentication:")
        );
        assert!(
            status_error(403)
                .unwrap()
                .starts_with("transfer_permission_or_policy:")
        );
        assert!(status_error(423).unwrap().starts_with("transfer_busy:"));
        assert!(
            status_error(429)
                .unwrap()
                .starts_with("transfer_rate_limit:")
        );
        assert!(
            status_error(503)
                .unwrap()
                .starts_with("transfer_service_unavailable:")
        );
    }
    #[test]
    fn missing_recipient_never_reaches_the_transfer_runner() {
        let result = handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"send","arguments":{"path":"x"}}}), &mut |_, _| panic!("must reject before side effects")).unwrap();
        assert_eq!(result["result"]["isError"], true);
    }
    #[test]
    fn transfer_paths_and_notifications_are_restricted() {
        for path in [
            "../secret",
            "/data/zeroclaw/auth-profiles.json",
            "a/../../b",
            "",
        ] {
            assert!(!valid_path(path));
        }
        assert!(valid_path("exports/report.pdf"));
        let mut called = false;
        let mut run = |_: &str, _: &Value| {
            called = true;
            Ok("ok".into())
        };
        assert!(
            handle(
                json!({"method":"tools/call","params":{"name":"send","arguments":{"path":"x"}}}),
                &mut run
            )
            .is_none()
        );
        let result = handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"send","arguments":{"path":"x","url":"https://example.com"}}}), &mut run).unwrap();
        assert_eq!(result["result"]["isError"], true);
        assert!(!called);
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn malformed_and_oversized_lines_do_not_drop_the_server() {
        let mut input = b"not-json\n".to_vec();
        input.extend(vec![b'x'; 9000]);
        input.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"initialize\"}\n");
        let mut output = Vec::new();
        serve_stream(&mut io::Cursor::new(input), &mut output, &mut |_, _| {
            panic!("must not execute")
        })
        .unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["error"]["code"], -32700);
        assert_eq!(replies[1]["error"]["code"], -32700);
        assert_eq!(replies[2]["id"], 3);
        assert!(replies[2].get("result").is_some());
    }
}
