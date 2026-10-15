#[tokio::test]
async fn heimdall_native_client_connects_to_codespace_server_without_network() {
    // The launcher binary path is shared with the skills test.
    let command = std::env::var("HEIMDALL_TEST_SKILLS_MCP").unwrap();
    let config: zeroclaw_config::schema::McpServerConfig = serde_json::from_value(serde_json::json!({
        "name":"codespace","transport":"stdio","command":"/usr/bin/env","args":["-u","GITHUB_TOKEN","-u","HEIMDALL_CODESPACE_REPOS",command,"--codespace-mcp"],"tool_timeout_secs":5
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
        ["status", "list", "create", "stop", "delete", "exec"]
    );
    // The native client can discover local configuration without credentials or
    // a GitHub request. env -u above prevents inherited operator tokens affecting this test.
    let result = server
        .call_tool("status", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(result["isError"], false);
    let status: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(status["token"], "missing");
    assert_eq!(status["repository_configuration"], "missing");
    assert_eq!(status["allowed_repositories"], serde_json::json!([]));
    assert_eq!(status["authentication_verified"], false);
    assert_eq!(status["network_verified"], false);
    assert_eq!(status["limits"]["deadline_seconds"], 80);
    // Missing command argument: rejected by the server before any process is spawned.
    let result = server
        .call_tool("exec", serde_json::json!({"name":"octo-box-1"}))
        .await;
    assert!(result.is_err());
}
