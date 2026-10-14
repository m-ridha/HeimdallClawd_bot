//! Bounded Telegram file transfer; executor credentials never reach workspace jobs.
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::blocking::{Client, Response, multipart};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
const LIMIT: usize = 49152;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
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
fn value(response: Response) -> Result<Value, &'static str> {
    if !response.status().is_success() {
        return Err("Transfer service rejected the request");
    }
    let mut bytes = Vec::new();
    response
        .take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| "Transfer response failed")?;
    if bytes.len() > 131072 {
        return Err("Transfer response exceeded the limit");
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid transfer service response")
}
struct Bridge {
    client: Client,
    token: String,
    executor: String,
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
            executor: std::env::var("HEIMDALL_EXECUTOR_TOKEN")
                .map_err(|_| "Executor token is required")?,
            peers,
        })
    }
    fn remote(&self, name: &str, args: Value) -> Result<String, &'static str> {
        let id = format!(
            "transfer-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let response = self.client.post("http://heimdall-executor.railway.internal:3889/mcp")
            .timeout(Duration::from_secs(35))
            .bearer_auth(&self.executor)
            .json(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}))
            .send().map_err(|_| "Private executor connection failed")?;
        let result = value(response)?;
        if result.get("error").is_some() || result["result"]["isError"] == true {
            return Err("Private executor transfer failed");
        }
        result["result"]["content"][0]["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or("Invalid executor response")
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
                let recipient = args["recipient"].as_str().unwrap_or(&self.peers[0]);
                if !self.peers.iter().any(|p| p == recipient) {
                    return Err("Recipient is outside the Telegram allowlist");
                }
                let encoded = self.remote("file_read_base64", json!({"path":path}))?;
                let bytes = STANDARD
                    .decode(encoded.trim())
                    .map_err(|_| "Invalid binary file encoding")?;
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
                        .map_err(|_| "Telegram upload failed")?,
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
                self.remote(
                    "file_write_base64",
                    json!({"path":path,"content":STANDARD.encode(bytes)}),
                )?;
                Ok("Telegram attachment copied into the isolated workspace".into())
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
                        .map_err(|_| "Telegram file lookup failed")?,
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
                    .map_err(|_| "Telegram download failed")?;
                if !response.status().is_success() {
                    return Err("Telegram rejected the download");
                }
                let mut bytes = Vec::new();
                response
                    .take((LIMIT + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Telegram download failed")?;
                if bytes.len() > LIMIT {
                    return Err("Transfer exceeds 48 KiB");
                }
                self.remote(
                    "file_write_base64",
                    json!({"path":path,"content":STANDARD.encode(bytes)}),
                )?;
                Ok("File saved in the isolated workspace".into())
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
            {"name":"send","description":"Send an isolated workspace file (up to 48 KiB) as a Telegram document to an allowed user. Defaults to the owner.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"recipient":{"type":"string"}},"required":["path"],"additionalProperties":false}},
            {"name":"import","description":"Copy a Telegram attachment already downloaded under the agent workspace telegram_files directory into the isolated workspace (up to 48 KiB). Use the source path shown in the incoming Telegram message.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"source":{"type":"string"}},"required":["path","source"],"additionalProperties":false}},
            {"name":"receive","description":"Download a Telegram file_id into the isolated workspace (up to 48 KiB).","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"file_id":{"type":"string"}},"required":["path","file_id"],"additionalProperties":false}}
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
                && args
                    .as_object()
                    .is_some_and(|a| a.keys().all(|k| allowed.contains(&k.as_str())));
            let (text, failed) = match if valid {
                run(tool, args)
            } else {
                Err("Invalid transfer arguments")
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
    loop {
        let mut line = String::new();
        if reader.by_ref().take(8193).read_line(&mut line)? == 0 {
            break;
        }
        if line.len() > 8192 {
            return Err(io::Error::other("Transfer request too large"));
        }
        let request = serde_json::from_str(&line).map_err(io::Error::other)?;
        if let Some(response) = handle(request, &mut run) {
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
