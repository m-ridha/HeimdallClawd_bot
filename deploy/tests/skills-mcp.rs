#[tokio::test]
async fn heimdall_native_client_connects_to_skill_installer_without_host_execution() {
    assert!(
        !std::fs::symlink_metadata("/usr/local/bin/zeroclaw")
            .unwrap()
            .file_type()
            .is_symlink(),
        "Native contract must install the runtime under the production system path, not symlink into the build tree"
    );
    let command = std::env::var("HEIMDALL_TEST_SKILLS_MCP").unwrap();
    let rendered = std::process::Command::new(&command)
        .arg("--render-config")
        .env_clear()
        .env("TELEGRAM_OWNER_ID", "1001")
        .output()
        .unwrap();
    assert!(rendered.status.success());
    // Config::load_or_init unconditionally ensures data/ exists before dispatching
    // `skills list`. Production sign-in creates it before polling. The confined
    // child must not be granted access to create or read instance data.
    std::fs::create_dir_all("/data/zeroclaw/data").unwrap();
    std::fs::write("/data/zeroclaw/data/private-fixture", "TEST_private_data").unwrap();
    std::fs::create_dir_all("/data/zeroclaw/shared/skills/managed").unwrap();
    std::fs::create_dir_all("/data/zeroclaw/agents/primary/workspace").unwrap();
    std::fs::write("/data/zeroclaw/config.toml", rendered.stdout).unwrap();
    std::fs::write("/data/zeroclaw/auth-profiles.json", "TEST_private_auth").unwrap();
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
    let listed = server.call_tool("list", serde_json::json!({})).await;
    assert!(listed.is_ok(), "Confined registry list failed: {listed:?}");
    let result = server
        .call_tool("install", serde_json::json!({"name":"../outside"}))
        .await;
    assert!(result.is_err());
    std::fs::remove_dir_all("/data/zeroclaw").unwrap();
}
