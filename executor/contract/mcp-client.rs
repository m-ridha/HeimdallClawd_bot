#![cfg(target_os = "linux")]
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use zeroclaw_config::schema::McpServerConfig;
use zeroclaw_tools::mcp_client::McpServer;
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
    assert_eq!(pwd["content"][0]["text"], "/data/workspace\n");
    let protected = server
        .call_tool("bash", json!({"command":"cat /run/heimdall-control/probe"}))
        .await
        .unwrap_err();
    assert!(protected.to_string().contains("Permission denied"));
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
    assert_eq!(cleanup["content"][0]["text"], "cleanup-ok");
    child.kill().await.unwrap();
    let _ = child.wait().await;
}
