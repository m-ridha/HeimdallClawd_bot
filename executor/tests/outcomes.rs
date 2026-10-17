#![cfg(target_os = "linux")]
use heimdall_executor::{
    process::run_at,
    protocol::{Job, Reply, TerminationReason},
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

async fn execute(tool: &str, arguments: Value, cancel: CancellationToken, timeout: Duration) -> Reply {
    run_at(
        Job {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.into(),
            arguments,
        },
        cancel,
        timeout,
        Path::new(env!("CARGO_BIN_EXE_heimdall-executor")),
    )
    .await
    .unwrap()
}

async fn command(text: &str, timeout: Duration) -> Reply {
    execute("bash", json!({"command":text}), CancellationToken::new(), timeout).await
}

fn wire_outcome(reply: &Reply) -> Value {
    let result = reply.mcp_result().unwrap();
    assert_eq!(result["isError"], reply.is_error);
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn assert_descendant_gone(reply: &Reply) {
    let pid: i32 = reply.command.as_ref().unwrap().stdout.trim().parse().unwrap();
    // SAFETY: signal zero only checks existence of the fixture PID.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "descendant survived");
}

async fn fixture_pid(path: &str) -> i32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(pid) = text.parse::<i32>() {
                    break pid;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn actual_command_outcomes_and_unchanged_file_results() {
    assert_eq!(unsafe { libc::geteuid() }, 0, "requires root Linux test container");
    // SAFETY: tests run in their dedicated root supervisor container.
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0), 0);
    }
    for path in ["/data/workspace", "/run/heimdall-tmp"] {
        std::fs::create_dir_all(path).unwrap();
        let path = std::ffi::CString::new(path).unwrap();
        // SAFETY: these are fixed fixture roots in the dedicated test container.
        assert_eq!(unsafe { libc::chown(path.as_ptr(), 64173, 64173) }, 0);
    }
    let timeout = Duration::from_secs(10);
    for (shell, code, stdout, stderr) in [
        ("true", 0, "", ""),
        ("exit 7", 7, "", ""),
        ("printf out; printf err >&2; exit 3", 3, "out", "err"),
        ("printf '\\303\\251'; printf '\\377' >&2", 0, "é", "�"),
    ] {
        let reply = command(shell, timeout).await;
        assert_eq!(reply.is_error, code != 0);
        assert_eq!(wire_outcome(&reply), json!({
            "exit_code":code, "termination_reason":"completed", "truncated":false,
            "stdout":stdout, "stderr":stderr,
        }));
    }

    let exact = command("head -c 65536 /dev/zero", timeout).await;
    let outcome = exact.command.as_ref().unwrap();
    assert!(!exact.is_error);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.termination_reason, TerminationReason::Completed);
    assert!(!outcome.truncated);
    assert_eq!(outcome.stdout.len(), 65536);
    assert!(outcome.stderr.is_empty());

    for shell in [
        "head -c 65537 /dev/zero",
        "head -c 65537 /dev/zero >&2",
        "head -c 32768 /dev/zero; head -c 32769 /dev/zero >&2",
    ] {
        let reply = command(shell, timeout).await;
        let outcome = reply.command.as_ref().unwrap();
        assert!(reply.is_error && reply.truncated && outcome.truncated);
        assert_eq!(outcome.termination_reason, TerminationReason::OutputLimit);
        assert_eq!(outcome.stdout.len() + outcome.stderr.len(), 65536);
        assert_eq!(wire_outcome(&reply)["termination_reason"], "output_limit");
    }

    let timed = command("sleep 120 & printf '%s\\n' $!; wait", Duration::from_secs(1)).await;
    let outcome = timed.command.as_ref().unwrap();
    assert!(timed.is_error);
    assert_eq!(outcome.termination_reason, TerminationReason::Timeout);
    assert_eq!(outcome.exit_code, None);
    assert!(!outcome.truncated);
    assert!(outcome.stderr.is_empty());
    assert_descendant_gone(&timed);

    let fixture = format!(".outcome-{}", uuid::Uuid::new_v4());
    let pidfile = format!("/data/workspace/{fixture}-pid");
    let cancel = CancellationToken::new();
    let execution = execute(
        "bash",
        json!({"command":format!("sleep 120 & printf '%s\\n' $!; printf '%s' $! > {fixture}-pid; wait")}),
        cancel.clone(),
        timeout,
    );
    // Synchronize cancellation with a running helper, rather than cancelling
    // before it has started. Both joined futures are asserted in the test body.
    let (canceled, descendant) = tokio::join!(execution, async {
        let pid = fixture_pid(&pidfile).await;
        cancel.cancel();
        pid
    });
    let outcome = canceled.command.as_ref().unwrap();
    assert!(canceled.is_error);
    assert_eq!(outcome.termination_reason, TerminationReason::Cancelled);
    assert_eq!(outcome.exit_code, None);
    assert!(!outcome.truncated);
    assert!(outcome.stderr.is_empty());
    // Cancellation may win before stdout is read; use the synchronized PID.
    assert_eq!(unsafe { libc::kill(descendant, 0) }, -1, "descendant survived");
    std::fs::remove_file(&pidfile).unwrap();

    let signal_command = format!("printf '%s' $$ > {fixture}-pid; exec sleep 120");
    let (signaled, ()) = tokio::join!(
        command(&signal_command, timeout),
        async {
            let pid = fixture_pid(&pidfile).await;
            // SAFETY: this is the PID written by this test's running helper.
            assert!(pid > 0);
            assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        }
    );
    assert!(signaled.is_error);
    assert_eq!(wire_outcome(&signaled), json!({
        "exit_code":null, "termination_reason":"completed", "truncated":false,
        "stdout":"", "stderr":"",
    }));
    std::fs::remove_file(&pidfile).unwrap();

    for (write, read, content, expected) in [
        ("file_write", "file_read", "{\"exit_code\":7}\n", "{\"exit_code\":7}\n"),
        ("file_write_base64", "file_read_base64", "AAECA/8=", "AAECA/8="),
    ] {
        let reply = execute(write, json!({"path":fixture,"content":content}), CancellationToken::new(), timeout).await;
        assert!(!reply.is_error);
        assert!(reply.command.is_none());
        assert_eq!(reply.mcp_result().unwrap()["content"][0]["text"], "file written\n");
        let reply = execute(read, json!({"path":fixture}), CancellationToken::new(), timeout).await;
        assert!(!reply.is_error);
        assert!(reply.command.is_none());
        assert_eq!(reply.mcp_result().unwrap()["content"][0]["text"], expected);
    }
    std::fs::remove_file(format!("/data/workspace/{fixture}")).unwrap();
}
