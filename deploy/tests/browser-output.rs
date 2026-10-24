//! Native regression coverage for the patched text-browser process runner.
#![cfg(target_os = "linux")]
use std::path::Path;
use std::time::Duration;
use zeroclaw_tools::text_browser::heimdall_browser_dump;

async fn wait_for_pid(path: &Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(path).await
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("browser fixture must start")
}

async fn assert_reaped(pid: u32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("browser child must terminate and be reaped");
}

fn args(script: &str, pid: &Path) -> Vec<String> {
    vec![
        "-c".into(),
        script.into(),
        "fixture".into(),
        pid.to_str().unwrap().into(),
    ]
}

#[tokio::test]
async fn heimdall_browser_accepts_exact_stdout_limit_and_drains_stderr() {
    let output = heimdall_browser_dump(
        "/bin/sh",
        &[
            "-c".into(),
            "head -c 16000 /dev/zero >&2; head -c 500000 /dev/zero".into(),
        ],
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 500_000);
    assert_eq!(output.stderr.len(), 16_000);
}

#[tokio::test]
async fn heimdall_browser_overflow_does_not_wait_for_open_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pid");
    let command = args(
        "echo $$ > \"$1\"; head -c 16000 /dev/zero >&2; head -c 500001 /dev/zero; exec sleep 30",
        &path,
    );
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        heimdall_browser_dump("/bin/sh", &command),
    )
    .await
    .expect("overflow must stop before the sleeping child's stderr closes");
    assert_eq!(
        result.unwrap_err().to_string(),
        "Text browser output limit exceeded"
    );
    assert_reaped(wait_for_pid(&path).await).await;
}

#[tokio::test]
async fn heimdall_browser_timeout_terminates_and_reaps_child() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pid");
    let command = args("echo $$ > \"$1\"; exec sleep 30", &path);
    let task = tokio::spawn(async move {
        tokio::time::timeout(
            Duration::from_secs(2),
            heimdall_browser_dump("/bin/sh", &command),
        )
        .await
    });
    let pid = wait_for_pid(&path).await;
    assert!(
        task.await.unwrap().is_err(),
        "outer timeout must cancel the runner"
    );
    assert_reaped(pid).await;
}

#[tokio::test]
async fn heimdall_browser_cancellation_terminates_and_reaps_child() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pid");
    let command = args("echo $$ > \"$1\"; exec sleep 30", &path);
    let task = tokio::spawn(async move { heimdall_browser_dump("/bin/sh", &command).await });
    let pid = wait_for_pid(&path).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_reaped(pid).await;
}
