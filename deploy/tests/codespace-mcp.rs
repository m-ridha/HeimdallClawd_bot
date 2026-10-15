#[tokio::test]
async fn heimdall_native_client_connects_to_codespace_server_without_network() {
    // The launcher binary path is shared with the skills test.
    let command = std::env::var("HEIMDALL_TEST_SKILLS_MCP").unwrap();
    let config: zeroclaw_config::schema::McpServerConfig = serde_json::from_value(serde_json::json!({
        "name":"codespace","transport":"stdio","command":command,"args":["--codespace-mcp"],"tool_timeout_secs":5
    })).unwrap();
    let server = zeroclaw_tools::mcp_client::McpServer::connect(config)
        .await
        .unwrap();
    let tools = server.tools().await;
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["list", "create", "stop", "delete", "exec"]
    );
    // Missing command argument: rejected by the server before any process is spawned.
    let result = server
        .call_tool("exec", serde_json::json!({"name":"octo-box-1"}))
        .await;
    assert!(result.is_err());
}
