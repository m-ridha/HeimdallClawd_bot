//! Narrow registry installer; no model-controlled commands, URLs or filesystem paths.
use serde_json::{Value, json};
use std::io::{self, Read};
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
            {"name":"install","description":"Install an audited skill with script files permitted by name from the official ZeroClaw registry into the agent's persistent managed bundle. 80-second local deadline, 32 KiB per output stream. Does not install OS dependencies or grant tools; run list after a timeout before retrying.","inputSchema":{"type":"object","properties":{"name":{"type":"string","maxLength":64}},"required":["name"],"additionalProperties":false}},
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
fn capture(mut reader: impl Read) -> Result<Vec<u8>, &'static str> {
    let mut saved = Vec::new();
    let mut block = [0; 4096];
    let mut overflowed = false;
    loop {
        let size = reader
            .read(&mut block)
            .map_err(|_| "Skill output read failed")?;
        if size == 0 {
            break;
        }
        let keep = size.min(LIMIT.saturating_sub(saved.len()));
        overflowed |= keep < size;
        saved.extend_from_slice(&block[..keep]);
    }
    if overflowed {
        return Err("skill_output_limit: output exceeded 32 KiB; run list to inspect state");
    }
    Ok(saved)
}
fn failure(diagnostics: &[u8]) -> String {
    let text = String::from_utf8_lossy(diagnostics).to_ascii_lowercase();
    let (category, hint) = if [
        "could not resolve",
        "network is unreachable",
        "connection refused",
        "failed to connect",
        "connection timed out",
    ]
    .iter()
    .any(|p| text.contains(p))
    {
        (
            "network",
            "Check registry reachability and DNS; run list before retrying installation",
        )
    } else if text.contains("audit") || text.contains("unsafe") {
        (
            "audit_or_policy",
            "Review registry audit requirements; do not bypass them",
        )
    } else if text.contains("not found") || text.contains("404") {
        ("not_found", "Check the bare official-registry skill name")
    } else {
        (
            "unknown",
            "Check skill compatibility and registry availability; run list to inspect state",
        )
    };
    json!({"code":"skill_command_failed","category":category,"classification":"heuristic","hint":hint}).to_string()
}
fn configure_command(command: &mut Command) {
    command
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("ZEROCLAW_CONFIG_DIR", STATE)
        .env(
            "ZEROCLAW_skills__registry_url",
            "https://github.com/zeroclaw-labs/zeroclaw-skills",
        )
        .env("ZEROCLAW_skills__allow_scripts", "true")
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
const INSTALL_SCRIPT: &str = r#"set -eu
if [ ! -d "$2/.git" ]; then
    git clone --depth 1 "$1" "$2"
fi
exec "$3" skills install "$4" --bundle managed"#;

fn execute(tool: &str, name: Option<&str>) -> Result<String, String> {
    use std::os::unix::process::CommandExt;
    let mut cmd = if tool == "install" {
        // The cache must exist before Landlock opens it. Upstream skips cloning
        // existing directories, so bootstrap an empty cache inside the same
        // cleared-environment, bounded process group before native installation.
        let mut cmd = Command::new("/bin/sh");
        configure_command(&mut cmd);
        cmd.args([
            "-c",
            INSTALL_SCRIPT,
            "heimdall-skills",
            "https://github.com/zeroclaw-labs/zeroclaw-skills",
            "/data/zeroclaw/data/skills-registry",
            "/usr/local/bin/zeroclaw",
            name.unwrap(),
        ]);
        cmd
    } else {
        let mut cmd = command();
        cmd.args(["skills", "list", "--agent", "primary"]);
        cmd
    };
    let session = crate::child_policy::Session::new()
        .map_err(|_| "Cannot prepare private child state".to_string())?;
    session
        .configure(&mut cmd, crate::child_policy::Scope::Skills)
        .map_err(|_| "Child filesystem isolation is unavailable".to_string())?;
    // SAFETY: pre_exec only calls async-signal-safe setsid before exec;
    // a separate process group lets cleanup terminate Git descendants.
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
                // SAFETY: group is the child session ID established by setsid.
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
                let _ = child.wait();
                break None;
            }
        }
    };
    // SAFETY: the negative child session ID targets only its process group.
    // Reap descendants even if the direct CLI child exited first.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
    let output = stdout
        .join()
        .map_err(|_| "Skill output capture failed".to_string())?
        .map_err(str::to_owned)?;
    let diagnostics = stderr
        .join()
        .map_err(|_| "Skill diagnostic capture failed".to_string())?
        .map_err(str::to_owned)?;
    match status {
        Some(status) if status.success() => Ok(String::from_utf8_lossy(&output).into_owned()),
        Some(_) => Err(failure(&diagnostics)),
        None => Err(
            "skill_timeout: completion is unknown; run list before retrying installation".into(),
        ),
    }
}
pub fn serve() -> io::Result<()> {
    crate::mcp_stdio::serve(
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        |request| handle(request, &mut execute),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_clones_empty_cache_and_reuses_existing_checkout() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let cache = root.path().join("cache with spaces");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&cache).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", root.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .current_dir(&source)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "fixture Git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        std::fs::write(source.join("fixture"), b"registry content").unwrap();
        git(&["add", "fixture"]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ]);
        let runtime = root.path().join("runtime stub");
        std::fs::write(&runtime, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\n").unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let run = |cache: &std::path::Path| {
            Command::new("/bin/sh")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", root.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args(["-c", INSTALL_SCRIPT, "fixture"])
                .arg(&source)
                .arg(cache)
                .arg(&runtime)
                .arg("fixture-skill")
                .output()
                .unwrap()
        };
        assert!(run(&cache).status.success());
        assert!(cache.join(".git").is_dir());
        assert_eq!(
            std::fs::read(cache.join("fixture")).unwrap(),
            b"registry content"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("runtime stub.args")).unwrap(),
            "skills\ninstall\nfixture-skill\n--bundle\nmanaged\n"
        );
        std::fs::remove_dir_all(&source).unwrap();
        assert!(
            run(&cache).status.success(),
            "existing checkout must not be cloned again"
        );
        std::fs::remove_file(root.path().join("runtime stub.args")).unwrap();
        let missing = root.path().join("missing cache");
        std::fs::create_dir(&missing).unwrap();
        assert!(!run(&missing).status.success());
        assert!(
            !root.path().join("runtime stub.args").exists(),
            "failed clone must not invoke installation"
        );
    }

    #[test]
    fn skill_failures_classify_without_echoing_registry_text() {
        for (text, expected) in [
            ("could not resolve", "network"),
            ("audit rejected", "audit_or_policy"),
            ("not found", "not_found"),
            ("unrecognized", "unknown"),
        ] {
            let result = failure(
                format!("{text} TEST_secret user-private-content https://private.invalid")
                    .as_bytes(),
            );
            let value: Value = serde_json::from_str(&result).unwrap();
            assert_eq!(value["category"], expected);
            assert!(!result.contains("TEST_secret"));
            assert!(!result.contains("user-private-content"));
            assert!(!result.contains("private.invalid"));
        }
    }
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
        assert!(capture(&vec![b'x'; LIMIT + 100][..]).is_err());
    }
}
