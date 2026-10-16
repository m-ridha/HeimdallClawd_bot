#![cfg(target_os = "linux")]
use heimdall_executor::{process::run_at, protocol::Job};
use serde_json::json;
use std::{path::Path, time::Duration};
use tokio_util::sync::CancellationToken;
async fn execute(
    tool: &str,
    args: serde_json::Value,
    timeout: Duration,
) -> heimdall_executor::protocol::Reply {
    run_at(
        Job {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.into(),
            arguments: args,
        },
        CancellationToken::new(),
        timeout,
        Path::new(env!("CARGO_BIN_EXE_heimdall-executor")),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn actual_linux_isolation_and_descendant_cleanup() {
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "Linux security tests must run in their root supervisor test container"
    );
    unsafe {
        assert_eq!(libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0), 0);
    }
    std::fs::create_dir_all("/data/workspace").unwrap();
    std::fs::create_dir_all("/run/heimdall-tmp").unwrap();
    for path in ["/data/workspace", "/run/heimdall-tmp"] {
        let path = std::ffi::CString::new(path).unwrap();
        unsafe {
            assert_eq!(libc::chown(path.as_ptr(), 64173, 64173), 0);
        }
    }
    std::fs::create_dir_all("/run/heimdall-control").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        "/run/heimdall-control",
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    std::fs::write("/run/heimdall-control/probe", "private").unwrap();
    // A persistent workspace symlink must never redirect root startup ownership.
    let malicious = Path::new("/data/workspace/.tmp");
    let _ = std::fs::remove_file(malicious);
    std::os::unix::fs::symlink("/run/heimdall-control", malicious).unwrap();
    assert!(include_str!("../entrypoint.sh").contains("exec /usr/local/bin/heimdall-executor"));
    let startup =
        include_str!("../entrypoint.sh").replace("exec /usr/local/bin/heimdall-executor", "exit 0");
    assert!(
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(startup)
            .status()
            .unwrap()
            .success()
    );
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata("/run/heimdall-control").unwrap().uid(), 0);
    std::fs::remove_file(malicious).unwrap();
    let t = Duration::from_secs(5);
    let probe = execute("__probe", json!({}), t).await;
    assert!(!probe.is_error, "{}", probe.content);
    assert!(probe.content.contains("isolation:passed"));
    let write = execute(
        "file_write",
        json!({"path":"note.txt","content":"hello"}),
        t,
    )
    .await;
    assert!(!write.is_error, "{}", write.content);
    let read = execute("file_read", json!({"path":"note.txt"}), t).await;
    assert_eq!(read.content, "hello");
    let command = execute("bash", json!({"command":"printf 'command-ok'"}), t).await;
    assert!(!command.is_error, "{}", command.content);
    assert_eq!(command.content, "command-ok");
    for (operation, command) in [
        (
            "timestamp-preserving copy",
            r#"printf compatibility > compat.txt && chmod 640 compat.txt && touch -t 202001020304 compat.txt && cp -p compat.txt compat-copy.txt && test "$(cat compat-copy.txt)" = compatibility && test "$(stat -c %a compat-copy.txt)" = 640 && test "$(stat -c %Y compat.txt)" = "$(stat -c %Y compat-copy.txt)""#,
        ),
        (
            "archive creation",
            "tar -cf compat.tar compat.txt && test -s compat.tar",
        ),
        (
            "archive extraction",
            r#"rm compat.txt && tar -xf compat.tar && test "$(cat compat.txt)" = compatibility && rm compat.txt compat-copy.txt compat.tar"#,
        ),
    ] {
        let compatibility = execute("bash", json!({"command":command}), t).await;
        assert!(
            !compatibility.is_error,
            "{operation}: {}",
            compatibility.content
        );
    }
    let git = execute(
        "bash",
        json!({"command":"mkdir -p git-fixture && cd git-fixture && git init -q && printf 'initial\\n' > example.txt && git add example.txt && git -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial && git checkout -qb feature && printf 'changed\\n' > example.txt && git diff --exit-code -- example.txt >/dev/null; result=$?; test \"$result\" -eq 1 && git add example.txt && git -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm change && test \"$(git log --format=%s -1)\" = change && git status --porcelain && cd .. && rm -rf git-fixture && printf git-offline-ready"}),
        t,
    ).await;
    assert!(!git.is_error, "{}", git.content);
    assert_eq!(git.content, "git-offline-ready");
    let scripting = execute(
        "bash",
        json!({"command": r#"python3 -B -c 'import base64,json,pathlib,subprocess,tempfile; p=pathlib.Path("python-fixture.json"); p.write_text(json.dumps({"value":base64.b64decode("cmVhZHk=").decode()})); assert subprocess.check_output(["jq","-r",".value",str(p)],text=True).strip()=="ready"; p.unlink(); f=tempfile.TemporaryFile(); f.write(b"ok"); f.close(); print("python-jq-ready")'"#}),
        t,
    ).await;
    assert!(!scripting.is_error, "{}", scripting.content);
    assert_eq!(scripting.content, "python-jq-ready\n");
    let allocation = execute(
        "bash",
        json!({"command":"tail /dev/zero"}),
        Duration::from_secs(10),
    )
    .await;
    assert!(allocation.is_error);
    assert!(
        !allocation.content.contains("Execution timed out"),
        "allocation cap did not stop job"
    );
    let temporary = execute(
        "bash",
        json!({"command":"printf '%s' \"$TMPDIR\"; touch \"$TMPDIR/probe\""}),
        t,
    )
    .await;
    assert!(!temporary.is_error, "{}", temporary.content);
    assert!(
        !Path::new(&temporary.content).exists(),
        "per-job temp directory survived"
    );
    let combined = execute(
        "bash",
        json!({"command":"rm -- note.txt; printf command-probe"}),
        t,
    )
    .await;
    assert!(!combined.is_error, "{}", combined.content);
    assert_eq!(combined.content, "command-probe");
    let escape = execute("bash", json!({"command":"setsid --wait /bin/true"}), t).await;
    assert!(escape.is_error, "setsid unexpectedly worked");
    let background = execute(
        "bash",
        json!({"command":"sleep 120 & printf '%s\\n' $!"}),
        t,
    )
    .await;
    let pid: i32 = background
        .content
        .trim()
        .parse()
        .expect(&background.content);
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "background child survived"
    );
    let timed = execute(
        "bash",
        json!({"command":"sleep 120"}),
        Duration::from_millis(150),
    )
    .await;
    assert!(timed.is_error);
    assert!(timed.content.contains("timed out"));
    let flood = execute("bash", json!({"command":"yes x"}), t).await;
    assert!(flood.truncated);
    assert!(flood.content.len() < 65650);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        token.cancel();
    });
    let canceled = run_at(
        Job {
            id: "cancel".into(),
            tool: "bash".into(),
            arguments: json!({"command":"sleep 120"}),
        },
        cancel,
        t,
        Path::new(env!("CARGO_BIN_EXE_heimdall-executor")),
    )
    .await
    .unwrap();
    assert!(canceled.is_error);
    assert!(canceled.content.contains("canceled"));
}
