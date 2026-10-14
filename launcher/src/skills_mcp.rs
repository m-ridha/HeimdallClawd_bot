//! Narrow registry installer; no model-controlled commands, URLs or filesystem paths.
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
const STATE: &str = "/data/zeroclaw";
const LIMIT: usize = 32768;
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn handle(
    request: Value,
    run: &mut impl FnMut(&str, Option<&str>) -> Result<String, String>,
) -> Option<Value> {
    // Notifications never execute a tool.
    let id = request.get("id")?.clone();
    let method = request["method"].as_str().unwrap_or("");
    if request["jsonrpc"] != "2.0" || !(id.is_string() || id.is_i64() || id.is_u64()) {
        return Some(error(id, -32600, "Invalid request"));
    }
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"heimdall-skills","version":"0.1.0"}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":[
            {"name":"install","description":"Install an audited skill with shell-script files disabled by name from the official ZeroClaw registry into the agent's persistent managed bundle. Does not install OS dependencies or grant tools.","inputSchema":{"type":"object","properties":{"name":{"type":"string","maxLength":64}},"required":["name"],"additionalProperties":false}},
            {"name":"list","description":"List skills available to the primary agent.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}
        ]}),
        "tools/call" => {
            let tool = request["params"]["name"].as_str().unwrap_or("");
            let args = &request["params"]["arguments"];
            let name = args["name"].as_str();
            let valid = match tool {
                "install" => {
                    args.as_object().is_some_and(|a| a.len() == 1) && name.is_some_and(valid_name)
                }
                "list" => args.as_object().is_some_and(|a| a.is_empty()),
                _ => false,
            };
            if !valid {
                return Some(error(
                    id,
                    -32602,
                    "Use a bare registry skill name; URLs, paths and extra arguments are not accepted",
                ));
            }
            let (text, failed) = match run(tool, name) {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            json!({"content":[{"type":"text","text":text}],"isError":failed})
        }
        _ => return Some(error(id, -32601, "Unknown method")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
fn capture(mut reader: impl Read) -> Vec<u8> {
    let mut saved = Vec::new();
    let mut block = [0; 4096];
    while let Ok(size) = reader.read(&mut block) {
        if size == 0 {
            break;
        }
        let keep = size.min(LIMIT.saturating_sub(saved.len()));
        saved.extend_from_slice(&block[..keep]);
    }
    saved
}
fn configure_command(command: &mut Command) {
    command
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", STATE)
        .env("ZEROCLAW_CONFIG_DIR", STATE)
        .env(
            "ZEROCLAW_skills__registry_url",
            "https://github.com/zeroclaw-labs/zeroclaw-skills",
        )
        .env("ZEROCLAW_skills__allow_scripts", "false")
        .env("ZEROCLAW_skills__open_skills_enabled", "false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_COUNT", "3")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "never")
        .env("GIT_CONFIG_KEY_1", "protocol.ext.allow")
        .env("GIT_CONFIG_VALUE_1", "never")
        .env("GIT_CONFIG_KEY_2", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_2", "/dev/null");
}
fn command() -> Command {
    let mut command = Command::new("/usr/local/bin/zeroclaw");
    configure_command(&mut command);
    command
}
fn execute(tool: &str, name: Option<&str>) -> Result<String, String> {
    use std::os::unix::process::CommandExt;
    let mut cmd = command();
    cmd.arg("skills");
    if tool == "install" {
        cmd.args(["install", name.unwrap(), "--bundle", "managed"]);
    } else {
        cmd.args(["list", "--agent", "primary"]);
    }
    // Separate process group lets timeout cleanup terminate Git descendants.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Skill command failed to start".to_string())?;
    let group = child.id() as i32;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let stdout = std::thread::spawn(move || capture(out));
    let stderr = std::thread::spawn(move || capture(err));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < Duration::from_secs(80) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => {
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
                let _ = child.wait();
                break None;
            }
        }
    };
    // Reap descendants even if the direct CLI child exited first.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
    let output = stdout.join().unwrap_or_default();
    let diagnostics = stderr.join().unwrap_or_default();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output),
        String::from_utf8_lossy(&diagnostics)
    );
    match status {
        Some(status) if status.success() => Ok(text),
        Some(_) => Err(format!("Skill installation/listing failed: {text}")),
        None => Err(
            "Skill operation timed out; retry list to check whether installation completed".into(),
        ),
    }
}
pub fn serve() -> io::Result<()> {
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
            Ok(request) => handle(request, &mut execute),
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
    #[test]
    fn rejects_urls_paths_flags_and_extra_arguments_without_execution() {
        let mut called = false;
        for name in [
            "../bad",
            "https://github.com/other/repo",
            "--help",
            "a/b",
            "a;pwd",
            "",
        ] {
            let response=handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"install","arguments":{"name":name}}}),&mut |_,_| {called=true;Ok(String::new())}).unwrap();
            assert!(response["error"].is_object());
        }
        assert!(!called);
    }
    #[test]
    fn exposes_only_install_and_list_and_ignores_tool_notifications() {
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
        assert_eq!(names, ["install", "list"]);
        assert!(handle(json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"install","arguments":{"name":"valid-skill"}}}),&mut |_,_| unreachable!()).is_none());
        let response=handle(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"install","arguments":{"name":"valid-skill"}}}),&mut |tool,name| {assert_eq!(tool,"install");assert_eq!(name,Some("valid-skill"));Ok("Installed".into())}).unwrap();
        assert_eq!(response["result"]["isError"], false);
    }
    #[test]
    fn subprocess_inherits_no_model_or_telegram_credentials() {
        let mut command = Command::new("/usr/bin/env");
        command.env("TELEGRAM_BOT_TOKEN", "dummy-private-fixture");
        configure_command(&mut command);
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("dummy-private-fixture"));
        let env: Vec<_> = command
            .get_envs()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        assert!(!env.iter().any(|key| key.contains("TOKEN")
            || key.contains("api_key")
            || key.contains("API_KEY")));
        assert!(env.contains(&"ZEROCLAW_skills__allow_scripts".to_string()));
        assert_eq!(capture(&vec![b'x'; LIMIT + 100][..]).len(), LIMIT);
    }
}
