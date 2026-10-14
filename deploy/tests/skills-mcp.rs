#[tokio::test]
async fn heimdall_native_client_connects_to_skill_installer_without_host_execution() {
    let command = std::env::var("HEIMDALL_TEST_SKILLS_MCP").unwrap();
    let config: zeroclaw_config::schema::McpServerConfig = serde_json::from_value(serde_json::json!({
        "name":"skills","transport":"stdio","command":command,"args":["--skills-mcp"],"tool_timeout_secs":5
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
        ["install", "list"]
    );
    let result = server
        .call_tool("install", serde_json::json!({"name":"../outside"}))
        .await;
    assert!(result.is_err());
}
