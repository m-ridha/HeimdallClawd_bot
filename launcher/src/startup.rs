//! Bounded authentication startup and an explicit, non-operational recovery listener.
use std::os::unix::process::CommandExt;
use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const AUTH_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_LIMIT: usize = 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

pub fn recovery_enabled(value: Option<&str>) -> io::Result<bool> {
    match value {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(io::Error::other("HEIMDALL_RECOVERY must be 0 or 1")),
    }
}

/// Keep terminal payload formatting off; structured diagnostics use their own policy.
pub fn configure_daemon(command: &mut Command, port: u16) -> &mut Command {
    command.args(["daemon", "--host", "0.0.0.0", "--port", &port.to_string()])
}

/// The native OAuth refresh needs its state files, not integration environment secrets.
pub fn configure_auth(command: &mut Command, state: &str) {
    command
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", state)
        .env("ZEROCLAW_CONFIG_DIR", state);
}

/// Discard OAuth diagnostics; bound the process and reap it on every exit path.
pub fn validate_auth(command: &mut Command, timeout: Duration) -> io::Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| io::Error::other("ChatGPT authentication check failed to start"))?;
    let deadline = Instant::now() + timeout;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(
                        "ChatGPT authentication validation failed; complete device sign-in in recovery mode",
                    ))
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                break Err(io::Error::other(
                    "ChatGPT authentication validation timed out; check connectivity or use recovery mode",
                ));
            }
            Err(_) => break Err(io::Error::other("ChatGPT authentication check failed")),
        }
    };
    // SAFETY: process_group(0) makes this child the leader of its own group;
    // the negative PID targets only the authentication subprocess descendants.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();
    result
}

/// Read-only capacity diagnostics. Never enumerate files or log file contents.
pub fn report_storage(path: &str) {
    let Ok(path_c) = std::ffi::CString::new(path) else {
        return;
    };
    let mut usage = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is a live NUL-terminated string and usage is writable storage.
    if unsafe { libc::statvfs(path_c.as_ptr(), usage.as_mut_ptr()) } != 0 {
        eprintln!("Storage capacity unavailable for {path}");
        return;
    }
    // SAFETY: statvfs initialized the structure on success.
    let usage = unsafe { usage.assume_init() };
    let total = (usage.f_blocks as u128) * (usage.f_frsize as u128);
    let available = (usage.f_bavail as u128) * (usage.f_frsize as u128);
    println!("Storage {path}: total_bytes={total} available_bytes={available}");
}

fn respond(mut stream: TcpStream) -> io::Result<()> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
    let mut line = Vec::new();
    while line.len() < REQUEST_LIMIT && Instant::now() < deadline {
        stream.set_read_timeout(Some(deadline.saturating_duration_since(Instant::now())))?;
        let mut byte = [0];
        if stream.read(&mut byte)? == 0 {
            break;
        }
        line.push(byte[0]);
        if line.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let healthy = line.ends_with(b"\r\n\r\n")
        && (line.starts_with(b"GET /health HTTP/1.1\r\n")
            || line.starts_with(b"GET /health HTTP/1.0\r\n"));
    let (status, body) = if healthy {
        ("200 OK", "recovery\n")
    } else {
        ("503 Service Unavailable", "bot offline: recovery\n")
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// No daemon, provider calls or persistent writes; inbound health checks only.
pub fn serve_recovery(port: u16) -> io::Result<()> {
    let listener = TcpListener::bind(("::", port))
        .map_err(|_| io::Error::other("Recovery listener failed to bind"))?;
    eprintln!("HEIMDALL RECOVERY: bot OFFLINE; health success indicates recovery access only");
    eprintln!(
        "Free storage first if needed; then run: gosu heimdall zeroclaw auth login --config-dir /data/zeroclaw --model-provider openai-codex --device-code"
    );
    eprintln!(
        "Remove HEIMDALL_RECOVERY and redeploy after repair; do not publish a release in recovery mode"
    );
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let _ = respond(stream);
            } // Untrusted request failures never enter logs.
            Err(_) => return Err(io::Error::other("Recovery listener failed")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daemon_uses_quiet_terminal_policy() {
        let mut command = Command::new("fixture");
        configure_daemon(&mut command, 3888);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["daemon", "--host", "0.0.0.0", "--port", "3888"]
        );
    }
    #[test]
    fn recovery_is_explicit_and_strict() {
        assert!(!recovery_enabled(None).unwrap());
        assert!(!recovery_enabled(Some("0")).unwrap());
        assert!(recovery_enabled(Some("1")).unwrap());
        assert!(
            recovery_enabled(Some("secret-invalid-value"))
                .unwrap_err()
                .to_string()
                .contains("0 or 1")
        );
    }
    #[test]
    fn auth_success_failure_and_timeout_have_safe_errors() {
        validate_auth(
            Command::new("/bin/sh").args(["-c", "exit 0"]),
            Duration::from_secs(2),
        )
        .unwrap();
        let error = validate_auth(
            Command::new("/bin/sh").args(["-c", "printf synthetic-private-value >&2; exit 1"]),
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(!error.to_string().contains("synthetic-private-value"));
        let start = Instant::now();
        let error = validate_auth(
            Command::new("/bin/sh").args(["-c", "sleep 30 & wait"]),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}

#[cfg(test)]
mod auth_environment_tests {
    #[test]
    fn refresh_does_not_inherit_integration_secrets() {
        let mut command = std::process::Command::new("/usr/bin/env");
        command.env("TEST_PROVIDER_SECRET", "TEST_private_value");
        super::configure_auth(&mut command, "/tmp/fixture-state");
        let output = command.output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("ZEROCLAW_CONFIG_DIR=/tmp/fixture-state"));
        assert!(!text.contains("TEST_PROVIDER_SECRET"));
        assert!(!text.contains("TEST_private_value"));
        assert_eq!(text.lines().count(), 3);
    }
}
