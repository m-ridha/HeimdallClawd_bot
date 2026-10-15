//! Restricted GitHub Codespaces control. The model supplies only validated codespace names,
//! allowlisted sandbox repositories and one remote command. Tokens, machine types, timeouts,
//! CLI flags and local shells are fixed here and never model-controlled.
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
const GH: &str = "/usr/local/bin/gh";
const HOME: &str = "/data/zeroclaw/codespace";
const LIMIT: usize = 32768;
const DEADLINE: Duration = Duration::from_secs(80);
const MACHINE: &str = "basicLinux32gb";
const IDLE_TIMEOUT: &str = "30m";
const RETENTION: &str = "24h";
const MAX_CODESPACES: usize = 2;
const MAX_COMMAND: usize = 8192;
fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 100
        && bytes[0] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}
fn valid_repo(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    let owner_ok = !owner.is_empty()
        && owner.len() <= 39
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let name_ok = !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b));
    owner_ok && name_ok
}
/// Parses `HEIMDALL_CODESPACE_REPOS`: comma-separated `owner/name` entries, lower-cased.
pub fn parse_repos(value: &str) -> Result<Vec<String>, &'static str> {
    let mut repos = Vec::new();
    for entry in value.split(',') {
        let repo = entry.trim().to_ascii_lowercase();
        if !valid_repo(&repo) {
            return Err("Invalid HEIMDALL_CODESPACE_REPOS; use comma-separated owner/name entries");
        }
        if !repos.contains(&repo) {
            repos.push(repo);
        }
    }
    Ok(repos)
}
fn repo_of(listing: &str, name: &str) -> Option<String> {
    let value: Value = serde_json::from_str(listing).ok()?;
    value
        .as_array()?
        .iter()
        .find(|codespace| codespace["name"] == name)?["repository"]
        .as_str()
        .map(str::to_ascii_lowercase)
}
fn count_of(listing: &str) -> Option<usize> {
    serde_json::from_str::<Value>(listing)
        .ok()?
        .as_array()
        .map(Vec::len)
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn tools() -> Value {
    json!([
        {"name":"list","description":"List the GitHub account's codespaces with their repository and state.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"create","description":"Create a small codespace for an allowed sandbox repository (owner/name). Machine size, idle timeout and retention are fixed; at most two codespaces may exist.","inputSchema":{"type":"object","properties":{"repo":{"type":"string","maxLength":140}},"required":["repo"],"additionalProperties":false}},
        {"name":"stop","description":"Stop a codespace in an allowed sandbox repository.","inputSchema":{"type":"object","properties":{"name":{"type":"string","maxLength":100}},"required":["name"],"additionalProperties":false}},
        {"name":"delete","description":"Permanently delete a codespace in an allowed sandbox repository.","inputSchema":{"type":"object","properties":{"name":{"type":"string","maxLength":100}},"required":["name"],"additionalProperties":false}},
        {"name":"exec","description":"Run one shell command inside a codespace in an allowed sandbox repository. Each call is a fresh remote shell; a stopped codespace is started automatically. Output is bounded.","inputSchema":{"type":"object","properties":{"name":{"type":"string","maxLength":100},"command":{"type":"string","minLength":1,"maxLength":8192}},"required":["name","command"],"additionalProperties":false}}
    ])
}
fn shaped(args: &Value, keys: &[&str]) -> bool {
    args.as_object().is_some_and(|object| {
        object.len() == keys.len()
            && keys
                .iter()
                .all(|key| object.get(*key).is_some_and(Value::is_string))
    })
}
fn handle(
    request: Value,
    run: &mut impl FnMut(&str, &Value) -> Result<String, String>,
) -> Option<Value> {
    // Notifications never execute a tool.
    let id = request.get("id")?.clone();
    if request["jsonrpc"] != "2.0" || !(id.is_string() || id.is_i64() || id.is_u64()) {
        return Some(error(id, -32600, "Invalid request"));
    }
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"heimdall-codespace","version":"0.1.0"}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":tools()}),
        "tools/call" => {
            let tool = request["params"]["name"].as_str().unwrap_or("");
            let args = &request["params"]["arguments"];
            let keys: &[&str] = match tool {
                "list" => &[],
                "create" => &["repo"],
                "stop" | "delete" => &["name"],
                "exec" => &["name", "command"],
                _ => return Some(error(id, -32602, "Unknown codespace tool")),
            };
            if !shaped(args, keys) {
                return Some(error(id, -32602, "Invalid codespace arguments"));
            }
            let (text, failed) = match run(tool, args) {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            json!({"content":[{"type":"text","text":text}],"isError":failed})
        }
        _ => return Some(error(id, -32601, "Unknown method")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
fn capture(mut reader: impl Read) -> Result<Vec<u8>, &'static str> {
    let mut saved = Vec::new();
    let mut block = [0; 4096];
    let mut overflowed = false;
    loop {
        let size = reader
            .read(&mut block)
            .map_err(|_| "GitHub output read failed")?;
        if size == 0 {
            break;
        }
        let keep = size.min(LIMIT.saturating_sub(saved.len()));
        overflowed |= keep < size;
        saved.extend_from_slice(&block[..keep]);
    }
    if overflowed {
        return Err("GitHub output exceeded 32 KiB; run a narrower command or list to check state");
    }
    Ok(saved)
}
/// The GitHub CLI receives only its own token; Telegram, executor and provider credentials are never inherited.
fn configure_command(command: &mut Command, token: &str) {
    command
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", HOME)
        .env("GH_TOKEN", token)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_SPINNER_DISABLED", "1")
        .env("NO_COLOR", "1");
}
struct Codespaces {
    token: String,
    repos: Vec<String>,
}
impl Codespaces {
    fn from_env() -> Result<Self, String> {
        let token =
            std::env::var("GITHUB_TOKEN").map_err(|_| "GITHUB_TOKEN is required".to_string())?;
        crate::config::github_authorization(&token).map_err(str::to_owned)?;
        let repos = std::env::var("HEIMDALL_CODESPACE_REPOS")
            .map_err(|_| "HEIMDALL_CODESPACE_REPOS is required".to_string())?;
        Ok(Self {
            token,
            repos: parse_repos(&repos).map_err(str::to_owned)?,
        })
    }
    fn gh(&self, args: &[&str], deadline: Instant) -> Result<String, String> {
        std::fs::create_dir_all(HOME).map_err(|_| "Cannot prepare codespace state".to_string())?;
        let mut command = Command::new(GH);
        configure_command(&mut command, &self.token);
        command.args(args);
        // Separate process group lets timeout cleanup terminate ssh descendants.
        // SAFETY: setsid is async-signal-safe; the child invokes no Rust allocation
        // or synchronization before exec and reports OS failure directly.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "GitHub CLI failed to start".to_string())?;
        let group = child.id() as i32;
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        let stdout = std::thread::spawn(move || capture(out));
        let stderr = std::thread::spawn(move || capture(err));
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                _ => {
                    // SAFETY: group is the PID of our child session; a negative PID
                    // addresses only that child's process group.
                    unsafe {
                        libc::kill(-group, libc::SIGKILL);
                    }
                    let _ = child.wait();
                    break None;
                }
            }
        };
        // Reap descendants even if the direct CLI child exited first.
        // SAFETY: group identifies the session created for this child.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        // CLI diagnostics may contain remote user content or credentials. Never return
        // them on failure; successful output redacts the local GitHub credential.
        let _diagnostics = stderr
            .join()
            .map_err(|_| "GitHub diagnostic capture failed".to_string())?
            .map_err(str::to_owned)?;
        let output = stdout
            .join()
            .map_err(|_| "GitHub output capture failed".to_string())?
            .map_err(str::to_owned)?;
        match status {
            Some(status) if status.success() => {
                Ok(String::from_utf8_lossy(&output).replace(&self.token, "[redacted]"))
            }
            Some(_) => {
                Err("GitHub command failed; check token permissions and codespace state".into())
            }
            None => Err("GitHub command timed out; run list to check the current state".into()),
        }
    }

    /// Every mutation and command is checked against the account's real listing, not model claims.
    fn owned(&self, name: &str, deadline: Instant) -> Result<(), String> {
        let listing = self.gh(
            &[
                "codespace",
                "list",
                "--json",
                "name,repository",
                "--limit",
                "100",
            ],
            deadline,
        )?;
        match repo_of(&listing, name) {
            Some(repo) if self.repos.contains(&repo) => Ok(()),
            _ => Err("Codespace is not in an allowed sandbox repository".into()),
        }
    }
    fn call(&self, tool: &str, args: &Value) -> Result<String, String> {
        let deadline = Instant::now() + DEADLINE;
        match tool {
            "list" => self.gh(
                &[
                    "codespace",
                    "list",
                    "--json",
                    "name,repository,state,lastUsedAt",
                    "--limit",
                    "30",
                ],
                deadline,
            ),
            "create" => {
                let repo = args["repo"].as_str().unwrap_or("").to_ascii_lowercase();
                if !valid_repo(&repo) || !self.repos.contains(&repo) {
                    return Err("Repository is not in the allowed sandbox list".into());
                }
                let listing = self.gh(
                    &["codespace", "list", "--json", "name", "--limit", "100"],
                    deadline,
                )?;
                if count_of(&listing).is_none_or(|count| count >= MAX_CODESPACES) {
                    return Err(format!(
                        "Codespace limit of {MAX_CODESPACES} reached; delete one first"
                    ));
                }
                self.gh(
                    &[
                        "codespace",
                        "create",
                        "--repo",
                        repo.as_str(),
                        "--machine",
                        MACHINE,
                        "--idle-timeout",
                        IDLE_TIMEOUT,
                        "--retention-period",
                        RETENTION,
                        "--default-permissions",
                    ],
                    deadline,
                )
            }
            "stop" | "delete" | "exec" => {
                let name = args["name"].as_str().unwrap_or("");
                if !valid_name(name) {
                    return Err("Invalid codespace name".into());
                }
                self.owned(name, deadline)?;
                match tool {
                    "stop" => self.gh(&["codespace", "stop", "--codespace", name], deadline),
                    "delete" => self.gh(
                        &["codespace", "delete", "--codespace", name, "--force"],
                        deadline,
                    ),
                    _ => {
                        let command = args["command"].as_str().unwrap_or("");
                        if command.is_empty()
                            || command.len() > MAX_COMMAND
                            || command.contains('\0')
                        {
                            return Err("Command must contain 1-8192 bytes".into());
                        }
                        self.gh(
                            &["codespace", "ssh", "--codespace", name, "--", command],
                            deadline,
                        )
                    }
                }
            }
            _ => Err("Unknown codespace tool".into()),
        }
    }
}
pub fn serve() -> io::Result<()> {
    let mut service: Option<Codespaces> = None;
    let mut run = |tool: &str, args: &Value| -> Result<String, String> {
        if service.is_none() {
            service = Some(Codespaces::from_env()?);
        }
        service.as_ref().unwrap().call(tool, args)
    };
    let input = io::stdin();
    let mut input = input.lock();
    let output = io::stdout();
    let mut output = output.lock();
    loop {
        let mut line = String::new();
        let size = (&mut input).take(131073).read_line(&mut line)?;
        if size == 0 {
            return Ok(());
        }
        if size > 131072 {
            return Err(io::Error::other("MCP request too large"));
        }
        let response = match serde_json::from_str(&line) {
            Ok(request) => handle(request, &mut run),
            Err(_) => Some(error(Value::Null, -32700, "Invalid JSON")),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(tool: &str, args: Value) -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}})
    }
    #[test]
    fn names_and_repositories_are_strict() {
        assert!(valid_name("octo-literate-space-parakeet-mld1"));
        let long = "a".repeat(101);
        for bad in ["", "-x", "Name", "a/b", "a b", "a;b", "../x", long.as_str()] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert_eq!(
            parse_repos(" Octo/Sandbox,octo/sandbox,octo/other.repo ").unwrap(),
            ["octo/sandbox", "octo/other.repo"]
        );
        for bad in [
            "", ",", "octo", "octo/", "/x", "a/b/c", "a/..", "a b/c", "octo/x,",
        ] {
            assert!(parse_repos(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn repository_lookup_uses_the_account_listing() {
        let listing = r#"[{"name":"a-1","repository":"Octo/Sandbox"},{"name":"b-2","repository":"octo/other"}]"#;
        assert_eq!(repo_of(listing, "a-1").as_deref(), Some("octo/sandbox"));
        assert_eq!(repo_of(listing, "missing"), None);
        assert_eq!(repo_of("not json", "a-1"), None);
        assert_eq!(count_of(listing), Some(2));
        assert_eq!(count_of("{}"), None);
    }
    #[test]
    fn exposes_exactly_five_tools_and_rejects_bad_shapes_without_execution() {
        let list = handle(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let names: Vec<_> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["list", "create", "stop", "delete", "exec"]);
        let mut called = false;
        for (tool, args) in [
            ("list", json!({"extra": "x"})),
            ("create", json!({})),
            ("create", json!({"repo": "a/b", "machine": "x"})),
            ("stop", json!({"name": 1})),
            ("exec", json!({"name": "a"})),
            ("ssh", json!({})),
        ] {
            let response = handle(request(tool, args), &mut |_, _| {
                called = true;
                Ok(String::new())
            })
            .unwrap();
            assert!(response["error"].is_object(), "{tool}");
        }
        assert!(!called);
    }
    #[test]
    fn notifications_never_execute_and_valid_calls_reach_the_runner() {
        assert!(
            handle(
                json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"list","arguments":{}}}),
                &mut |_, _| unreachable!()
            )
            .is_none()
        );
        let ok = handle(
            request(
                "exec",
                json!({"name": "octo-box-1", "command": "git status"}),
            ),
            &mut |tool, args| {
                assert_eq!(tool, "exec");
                assert_eq!(args["command"], "git status");
                Ok("clean".into())
            },
        )
        .unwrap();
        assert_eq!(ok["result"]["isError"], false);
        let failed = handle(request("list", json!({})), &mut |_, _| Err("boom".into())).unwrap();
        assert_eq!(failed["result"]["isError"], true);
    }
    #[test]
    fn subprocess_receives_only_the_github_token() {
        let mut command = Command::new("/usr/bin/env");
        command
            .env("TELEGRAM_BOT_TOKEN", "dummy-private-fixture")
            .env("HEIMDALL_EXECUTOR_TOKEN", "dummy-executor-fixture");
        configure_command(&mut command, "ghp_fixtureTokenValue0123456789");
        let output = command.output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(!text.contains("dummy-private-fixture"));
        assert!(!text.contains("dummy-executor-fixture"));
        assert!(text.contains("GH_TOKEN=ghp_fixtureTokenValue0123456789"));
        assert_eq!(capture(&vec![b'x'; LIMIT][..]).unwrap().len(), LIMIT);
        assert!(capture(&vec![b'x'; LIMIT + 100][..]).is_err());
    }
}
