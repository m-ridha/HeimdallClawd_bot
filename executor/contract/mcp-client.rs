#![cfg(target_os = "linux")]
use serde_json::{Value, json};

use tokio::io::{AsyncBufReadExt, BufReader};
use zeroclaw_config::schema::McpServerConfig;
use zeroclaw_tools::mcp_client::McpServer;

fn command_outcome(reply: Value) -> Value {
    assert_eq!(reply["isError"], false);
    serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn heimdall_native_mcp_client_contract() {
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "contract requires root Linux builder"
    );
    let mut child = tokio::process::Command::new("/bin/sh")
        .arg(std::env::var("HEIMDALL_TEST_ENTRYPOINT").unwrap())
        .env("EXECUTOR_TOKEN", "x".repeat(32))
        .env("PORT", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let port = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(addr) = line.strip_prefix("Executor listening on ") {
                return addr.parse::<std::net::SocketAddr>().unwrap().port();
            }
        }
        panic!("executor did not become ready");
    })
    .await
    .unwrap();
    let cfg = |token: &str| -> McpServerConfig {
        serde_json::from_value(json!({"name":"workspace","transport":"http","url":format!("http://[::1]:{port}/mcp"),"headers":{"Authorization":format!("Bearer {token}")},"tool_timeout_secs":35,"max_response_bytes":524288})).unwrap()
    };
    assert!(McpServer::connect(cfg("incorrect")).await.is_err());
    let server = McpServer::connect(cfg(&"x".repeat(32))).await.unwrap();
    assert_eq!(
        server
            .tools()
            .await
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        [
            "bash",
            "file_read",
            "file_read_base64",
            "file_write_base64",
            "file_write"
        ]
    );
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // Fresh real clients both start tools/call at ID 3. Each command waits
    // for the other to start, proving overlap rather than sequential success.
    let first = McpServer::connect(cfg(&"x".repeat(32))).await.unwrap();
    let second = McpServer::connect(cfg(&"x".repeat(32))).await.unwrap();
    let a = format!(".client-concurrent-a-{suffix}");
    let b = format!(".client-concurrent-b-{suffix}");
    let (left, right) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(
            first.call_tool("bash", json!({"command":format!("touch {a}; while ! test -f {b}; do sleep 0.05; done; printf first")})),
            second.call_tool("bash", json!({"command":format!("touch {b}; while ! test -f {a}; do sleep 0.05; done; printf second")}))
        )
    }).await.unwrap();
    assert_eq!(command_outcome(left.unwrap())["stdout"], "first");
    assert_eq!(command_outcome(right.unwrap())["stdout"], "second");
    first
        .call_tool("bash", json!({"command":format!("rm -- {a} {b}")}))
        .await
        .unwrap();
    let path = format!(".client-contract-{suffix}");
    let write = server
        .call_tool(
            "file_write",
            json!({"path":path,"content":"native-client-ok"}),
        )
        .await
        .unwrap();
    assert_eq!(write["isError"], false);
    let read = server
        .call_tool("file_read", json!({"path":path}))
        .await
        .unwrap();
    assert_eq!(read["content"][0]["text"], "native-client-ok");
    let pwd = server
        .call_tool("bash", json!({"command":"pwd"}))
        .await
        .unwrap();
    assert_eq!(command_outcome(pwd)["stdout"], "/data/workspace\n");
    let protected = server
        .call_tool("bash", json!({"command":"cat /run/heimdall-control/probe"}))
        .await
        .unwrap_err();
    assert!(protected.to_string().contains("Permission denied"));
    let split = server
        .call_tool("bash", json!({"command":"printf out; printf err >&2"}))
        .await
        .unwrap();
    assert_eq!(command_outcome(split), json!({
        "exit_code":0, "termination_reason":"completed", "truncated":false,
        "stdout":"out", "stderr":"err",
    }));
    let silent = server
        .call_tool("bash", json!({"command":"exit 7"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(silent.contains("\"exit_code\":7"), "{silent}");
    assert!(silent.contains("\"termination_reason\":\"completed\""), "{silent}");
    // Status must survive the pinned client's shortening of long errors.
    let noisy = server
        .call_tool("bash", json!({"command":"head -c 8192 /dev/zero; exit 9"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(noisy.contains("\"exit_code\":9"), "{noisy}");
    assert!(noisy.contains("\"truncated\":false"), "{noisy}");
    let pidfile = format!(".client-pid-{suffix}");
    let interrupted = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        server.call_tool(
            "bash",
            json!({"command":format!("sleep 120 & echo $! > {pidfile}; wait")}),
        ),
    )
    .await;
    assert!(interrupted.is_err());
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let pid: i32 = std::fs::read_to_string(format!("/data/workspace/{pidfile}"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "canceled child survived");
    let fresh = McpServer::connect(cfg(&"x".repeat(32))).await.unwrap();
    let cleanup = fresh
        .call_tool(
            "bash",
            json!({"command":format!("rm -- {path} {pidfile}; printf cleanup-ok")}),
        )
        .await
        .unwrap();
    assert_eq!(command_outcome(cleanup)["stdout"], "cleanup-ok");
    child.kill().await.unwrap();
    let _ = child.wait().await;
}

#[tokio::test]
async fn heimdall_client_echoes_session_header_on_calls() {
    use wiremock::matchers::{body_partial_json, header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let endpoint = MockServer::start().await;
    let session = "00000000-0000-4000-8000-000000000001";
    Mock::given(method("POST"))
        .and(body_partial_json(json!({"method":"initialize"})))
        .respond_with(ResponseTemplate::new(200).insert_header("Mcp-Session-Id", session)
            .set_body_json(json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}}}})))
        .expect(1).mount(&endpoint).await;
    Mock::given(body_partial_json(
        json!({"method":"notifications/initialized"}),
    ))
    .and(header("Mcp-Session-Id", session))
    .respond_with(ResponseTemplate::new(202))
    .expect(1)
    .mount(&endpoint)
    .await;
    Mock::given(body_partial_json(json!({"method":"tools/list"})))
        .and(header("Mcp-Session-Id", session))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"bash","description":"fixture","inputSchema":{"type":"object"}}]}})))
        .expect(1).mount(&endpoint).await;
    Mock::given(body_partial_json(json!({"method":"tools/call","id":3})))
        .and(header("Mcp-Session-Id", session))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"session-ok"}],"isError":false}})))
        .expect(1).mount(&endpoint).await;
    let config: McpServerConfig = serde_json::from_value(
        json!({"name":"fixture","transport":"http","url":endpoint.uri(),"tool_timeout_secs":5}),
    )
    .unwrap();
    let client = McpServer::connect(config).await.unwrap();
    let reply = client
        .call_tool("bash", json!({"command":"true"}))
        .await
        .unwrap();
    assert_eq!(reply["content"][0]["text"], "session-ok");
    endpoint.verify().await;
}
