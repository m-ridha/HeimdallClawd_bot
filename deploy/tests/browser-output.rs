//! Native regression coverage for the patched text-browser process runner.
#![cfg(target_os = "linux")]
use std::path::Path;
use std::time::Duration;
use zeroclaw_tools::text_browser::heimdall_browser_dump;

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use zeroclaw_api::tool::{Tool, ToolResult};
use zeroclaw_config::autonomy::AutonomyLevel;
use zeroclaw_config::policy::SecurityPolicy;
use zeroclaw_tools::text_browser::{TextBrowserTool, heimdall_browser_client};

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
        b"",
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
        heimdall_browser_dump("/bin/sh", &command, b""),
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
            heimdall_browser_dump("/bin/sh", &command, b""),
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
    let task = tokio::spawn(async move { heimdall_browser_dump("/bin/sh", &command, b"").await });
    let pid = wait_for_pid(&path).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_reaped(pid).await;
}

fn tool(allowed_private_hosts: &[&str]) -> TextBrowserTool {
    TextBrowserTool::new_with_private_hosts(
        Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            ..SecurityPolicy::default()
        }),
        Some("lynx".into()),
        5,
        allowed_private_hosts
            .iter()
            .map(|host| (*host).into())
            .collect(),
        vec![],
    )
    .unwrap()
}

async fn accept(listener: &TcpListener) -> (TcpStream, std::net::SocketAddr) {
    tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .expect("fixture must receive a connection")
        .unwrap()
}

async fn request(stream: &mut TcpStream) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            let byte = stream.read_u8().await.unwrap();
            bytes.push(byte);
            assert!(bytes.len() < 16_384);
        }
        String::from_utf8(bytes).unwrap()
    })
    .await
    .expect("fixture must receive request headers")
}

async fn execute(url: &str, allowed_private_hosts: &[&str]) -> ToolResult {
    tool(allowed_private_hosts)
        .execute(serde_json::json!({"url": url}))
        .await
        .unwrap()
}

async fn redirect_result(location: &str, allowed_private_hosts: &[&str]) -> ToolResult {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let server = tokio::spawn(async move {
        let (mut stream, _) = accept(&listener).await;
        request(&mut stream).await;
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    let result = execute(&url, allowed_private_hosts).await;
    server.await.unwrap();
    result
}

#[tokio::test]
async fn redirect_targets_reapply_private_metadata_and_scheme_checks() {
    // Only the fixture's initial host is opted in. The redirect must not inherit
    // its permission. No request may reach the separate private destination.
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = format!(
        "http://localhost:{}/private",
        target.local_addr().unwrap().port()
    );
    let result = redirect_result(&destination, &["127.0.0.1"]).await;
    assert!(!result.success);
    assert!(result.error.unwrap().contains("private"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err()
    );

    for destination in [
        "http://10.0.0.1/",
        "http://[::1]/",
        "http://169.254.169.254/latest/meta-data/",
        "http://user:password@example.invalid/",
        "file:///etc/passwd",
    ] {
        let result = redirect_result(destination, &["127.0.0.1"]).await;
        assert!(!result.success, "{destination}");
    }
    let result = redirect_result("http://169.254.169.254/", &["*"]).await;
    assert!(!result.success);
    assert!(result.error.unwrap().contains("metadata"));
}

#[tokio::test]
async fn relative_redirect_renders_offline_with_final_base_and_no_subrequests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = accept(&listener).await;
        assert!(request(&mut stream).await.starts_with("GET /start "));
        stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /final/page\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        drop(stream);
        let (mut stream, _) = accept(&listener).await;
        assert!(request(&mut stream).await.starts_with("GET /final/page "));
        let body = format!(
            "<html><head><meta http-equiv='refresh' content='0;url=http://{address}/private'></head><body><p>Safe rendered page</p><a href='next'>Next page</a><img src='http://{address}/image'><iframe src='http://{address}/frame'></iframe><object data='http://{address}/object'></object></body></html>"
        );
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        listener
    });
    let result = execute(&format!("http://{address}/start"), &["127.0.0.1"]).await;
    let listener = server.await.unwrap();
    assert!(result.success, "{:?}", result.error);
    assert!(result.output.contains("Safe rendered page"));
    assert!(
        result
            .output
            .contains(&format!("http://{address}/final/next"))
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn pinned_transport_preserves_host_and_does_not_resolve_again_or_follow_redirects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = accept(&listener).await;
        let headers = request(&mut stream).await.to_ascii_lowercase();
        assert!(headers.contains(&format!("host: pinned-browser.invalid:{}", address.port())));
        stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
    });
    // This hostname cannot resolve through DNS. Only the supplied address works.
    let client = heimdall_browser_client("pinned-browser.invalid", &[address]).unwrap();
    let response = client
        .get(format!("http://pinned-browser.invalid:{}/", address.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    server.await.unwrap();
}

#[tokio::test]
async fn redirect_loops_stop_at_ten_hops() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..11 {
            let (mut stream, _) = accept(&listener).await;
            request(&mut stream).await;
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
        }
        listener
    });
    let result = execute(&format!("http://{address}/"), &["127.0.0.1"]).await;
    let listener = server.await.unwrap();
    assert!(!result.success);
    assert!(result.error.unwrap().contains("Too many redirects"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn fetched_input_is_bounded_before_rendering() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = accept(&listener).await;
        request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 500001\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        stream.write_all(&vec![b'x'; 500_001]).await.unwrap();
    });
    let result = execute(&format!("http://{address}/"), &["127.0.0.1"]).await;
    server.await.unwrap();
    assert!(!result.success);
    assert!(
        result
            .error
            .unwrap()
            .contains("Text browser input limit exceeded")
    );
}

#[tokio::test]
async fn unguarded_browser_backends_are_rejected() {
    for browser in ["links", "w3m"] {
        let result = tool(&[])
            .execute(serde_json::json!({
                "url": "http://example.invalid/", "browser": browser,
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unsupported text browser"));
    }
}

#[tokio::test]
async fn stdin_is_written_while_output_is_drained() {
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        heimdall_browser_dump(
            "/bin/sh",
            &[
                "-c".into(),
                "head -c 500000 /dev/zero; cat >/dev/null".into(),
            ],
            &vec![b'x'; 500_000],
        ),
    )
    .await
    .expect("stdin and stdout must not deadlock")
    .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout.len(), 500_000);
}

#[tokio::test]
async fn public_style_cross_host_redirects_work_and_plain_text_stays_literal() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = format!(
        "http://localhost:{}/text",
        target.local_addr().unwrap().port()
    );
    let server = tokio::spawn(async move {
        let (mut stream, _) = accept(&target).await;
        request(&mut stream).await;
        let body = "Readable café <b>literal tags</b>";
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    });
    let result = redirect_result(&destination, &["127.0.0.1", "localhost"]).await;
    server.await.unwrap();
    assert!(result.success, "{:?}", result.error);
    assert!(result.output.contains("Readable café <b>literal tags</b>"));
}
