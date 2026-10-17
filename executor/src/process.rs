use crate::protocol::{CommandOutcome, Job, LIMIT, Reply, TerminationReason};
use std::{io, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

struct Group(i32);
impl Drop for Group {
    fn drop(&mut self) {
        if self.0 > 0 {
            unsafe {
                libc::kill(-self.0, libc::SIGKILL);
            }
        }
    }
}

/// Retain one shared byte budget, plus the legacy merged view used by file
/// tools and startup probes. Return true only if this read discarded bytes.
fn capture(merged: &mut Vec<u8>, stream: &mut Vec<u8>, bytes: &[u8]) -> bool {
    let retained = bytes.len().min(LIMIT - merged.len());
    merged.extend_from_slice(&bytes[..retained]);
    stream.extend_from_slice(&bytes[..retained]);
    retained < bytes.len()
}

pub async fn run(job: Job, cancel: CancellationToken, timeout: Duration) -> io::Result<Reply> {
    run_at(job, cancel, timeout, &std::env::current_exe()?).await
}

pub async fn run_at(
    job: Job,
    cancel: CancellationToken,
    timeout: Duration,
    binary: &std::path::Path,
) -> io::Result<Reply> {
    if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return Err(io::Error::other("An isolated Linux supervisor is required"));
    }
    let temp = tempfile::Builder::new()
        .prefix("job-")
        .tempdir_in("/run/heimdall-tmp")?;
    let path = std::ffi::CString::new(temp.path().as_os_str().as_encoded_bytes())
        .map_err(io::Error::other)?;
    // SAFETY: this is a newly created, root-owned directory under a root-owned parent.
    if unsafe { libc::chown(path.as_ptr(), 64173, 64173) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut command = Command::new(binary);
    command
        .arg("--helper")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", temp.path())
        .env("TMPDIR", temp.path())
        .current_dir("/data/workspace")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.process_group(0);
        // SAFETY: only async-signal-safe identity syscalls are used between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setgroups(0, std::ptr::null()) != 0
                    || libc::setgid(64173) != 0
                    || libc::setuid(64173) != 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("No helper PID"))? as i32;
    let mut group = Group(pid);
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("Missing helper stdin"))?;
    let payload = serde_json::to_vec(&job).map_err(io::Error::other)?;
    let writer = tokio::spawn(async move { stdin.write_all(&payload).await });
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Missing helper stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Missing helper stderr"))?;
    let mut output = Vec::new();
    let mut stdout_output = Vec::new();
    let mut stderr_output = Vec::new();
    let mut out_done = false;
    let mut err_done = false;
    let mut out_buf = [0u8; 4096];
    let mut err_buf = [0u8; 4096];
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut reason = TerminationReason::Completed;
    let mut truncated = false;
    let mut status = None;
    let mut failure = None;
    loop {
        if out_done && err_done && status.is_some() {
            break;
        }
        tokio::select! {
            _ = cancel.cancelled() => {
                reason = TerminationReason::Cancelled;
                break;
            }
            _ = &mut deadline => {
                reason = TerminationReason::Timeout;
                break;
            }
            read = stdout.read(&mut out_buf), if !out_done => {
                let n = match read {
                    Ok(n) => n,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                if n == 0 {
                    out_done = true;
                } else if capture(&mut output, &mut stdout_output, &out_buf[..n]) {
                    truncated = true;
                    reason = TerminationReason::OutputLimit;
                    break;
                }
            }
            read = stderr.read(&mut err_buf), if !err_done => {
                let n = match read {
                    Ok(n) => n,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                if n == 0 {
                    err_done = true;
                } else if capture(&mut output, &mut stderr_output, &err_buf[..n]) {
                    truncated = true;
                    reason = TerminationReason::OutputLimit;
                    break;
                }
            }
            result = child.wait(), if status.is_none() => {
                match result {
                    Ok(value) => status = Some(value),
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
                // SAFETY: pid is the process group created for this helper.
                // A completed shell must not leave background descendants running.
                unsafe { libc::kill(-pid, libc::SIGKILL); }
            }
        }
    }
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    if status.is_none() {
        match child.wait().await {
            Ok(value) => status = Some(value),
            Err(error) => failure = Some(error),
        }
    }
    writer.abort();
    // Subreaper ownership permits collecting orphan descendants after group kill.
    for _ in 0..100 {
        let result = unsafe { libc::waitpid(-pid, std::ptr::null_mut(), libc::WNOHANG) };
        if result < 0 {
            break;
        }
        if result == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    group.0 = 0;
    if let Some(error) = failure {
        return Err(error);
    }
    let mut content = String::from_utf8_lossy(&output).into_owned();
    // Preserve legacy annotations internally and for file tools, but do not
    // inject them into the command's actual stdout or stderr.
    match reason {
        TerminationReason::Timeout => content.push_str("\nExecution timed out"),
        TerminationReason::Cancelled => content.push_str("\nExecution canceled"),
        _ => {}
    }
    if truncated {
        content.push_str("\n[output truncated; execution stopped]");
    }
    let status = status.ok_or_else(|| io::Error::other("Missing helper exit status"))?;
    let command = (job.tool == "bash").then(|| CommandOutcome {
        exit_code: status.code(),
        termination_reason: reason,
        truncated,
        stdout: String::from_utf8_lossy(&stdout_output).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_output).into_owned(),
    });
    let is_error = if command.is_some() {
        reason != TerminationReason::Completed || !status.success()
    } else {
        // Keep the existing file-tool truncation and error behavior unchanged.
        matches!(
            reason,
            TerminationReason::Timeout | TerminationReason::Cancelled
        )
            || (!status.success() && !(truncated && job.tool == "file_read"))
    };
    Ok(Reply {
        id: job.id,
        content,
        is_error,
        truncated,
        command,
    })
}

/// These run in the actual deployed container before /health can become ready.
pub async fn verify_platform() -> io::Result<()> {
    use serde_json::json;
    let timeout = Duration::from_secs(5);
    let token = CancellationToken::new();
    let execute = |tool: &str, args: serde_json::Value| Job {
        id: uuid::Uuid::new_v4().to_string(),
        tool: tool.into(),
        arguments: args,
    };
    let probe = run(execute("__probe", json!({})), token.clone(), timeout).await?;
    if probe.is_error || !probe.content.contains("isolation:passed") {
        return Err(io::Error::other(format!(
            "kernel/identity probe failed: {}",
            probe.content
        )));
    }
    println!("Isolation check kernel_identity_credentials: PASS");
    let name = format!(".heimdall-probe-{}", uuid::Uuid::new_v4());
    let write = run(
        execute(
            "file_write",
            json!({"path":name,"content":"workspace-probe"}),
        ),
        token.clone(),
        timeout,
    )
    .await?;
    let read = run(
        execute("file_read", json!({"path":name})),
        token.clone(),
        timeout,
    )
    .await?;
    if write.is_error || read.is_error || read.content != "workspace-probe" {
        return Err(io::Error::other("workspace probe failed"));
    }
    let command = run(
        execute(
            "bash",
            json!({"command":format!("rm -- {name}; printf command-probe")}),
        ),
        token.clone(),
        timeout,
    )
    .await?;
    if command.is_error || command.content != "command-probe" {
        return Err(io::Error::other(format!(
            "shell probe failed: {}",
            command.content
        )));
    }
    println!("Isolation check workspace_and_commands: PASS");
    let background = run(
        execute("bash", json!({"command":"sleep 120 & printf '%s\\n' $!"})),
        token.clone(),
        timeout,
    )
    .await?;
    let pid: i32 = background
        .content
        .trim()
        .parse()
        .map_err(|_| io::Error::other("descendant probe returned invalid PID"))?;
    if unsafe { libc::kill(pid, 0) } == 0 {
        return Err(io::Error::other("background descendant survived"));
    }
    let timed = run(
        execute("bash", json!({"command":"sleep 120"})),
        token.clone(),
        Duration::from_millis(150),
    )
    .await?;
    if !timed.is_error
        || !timed.command.as_ref().is_some_and(|command| {
            command.termination_reason == TerminationReason::Timeout
        })
    {
        return Err(io::Error::other("timeout probe failed"));
    }
    let flood = run(
        execute("bash", json!({"command":"yes x"})),
        token.clone(),
        timeout,
    )
    .await?;
    if !flood.is_error
        || !flood.command.as_ref().is_some_and(|command| {
            command.truncated && command.termination_reason == TerminationReason::OutputLimit
        })
    {
        return Err(io::Error::other("output limit probe failed"));
    }
    let canceled = CancellationToken::new();
    let signal = canceled.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        signal.cancel();
    });
    let result = run(
        execute("bash", json!({"command":"sleep 120"})),
        canceled,
        timeout,
    )
    .await?;
    if !result.is_error
        || !result.command.as_ref().is_some_and(|command| {
            command.termination_reason == TerminationReason::Cancelled
        })
    {
        return Err(io::Error::other("cancellation probe failed"));
    }
    println!("Isolation check descendants_timeout_output_cancel: PASS");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_share_one_limit_and_exact_limit_is_not_truncated() {
        let mut merged = Vec::new();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(!capture(&mut merged, &mut stdout, &vec![b'o'; 32768]));
        assert!(!capture(&mut merged, &mut stderr, &vec![b'e'; 32768]));
        assert_eq!(merged.len(), 65536);
        assert_eq!(stdout.len(), 32768);
        assert_eq!(stderr.len(), 32768);
        assert!(capture(&mut merged, &mut stderr, b"overflow"));
        assert_eq!(merged.len(), stdout.len() + stderr.len());
        assert_eq!(merged.len(), 65536);
    }

    #[test]
    fn partial_reads_retain_only_available_bytes() {
        let mut merged = vec![b'x'; 65535];
        let mut stderr = Vec::new();
        assert!(capture(&mut merged, &mut stderr, b"abc"));
        assert_eq!(stderr, b"a");
        assert_eq!(merged.len(), 65536);
    }
}
