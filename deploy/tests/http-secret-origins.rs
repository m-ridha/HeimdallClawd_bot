use zeroclaw_tools::http_request::heimdall_auth_origin;
#[test]
fn named_secrets_require_exact_tls_origins() {
    for (secret, host) in [
        ("github", "api.github.com"),
        ("agentmail", "api.agentmail.to"),
    ] {
        assert!(heimdall_auth_origin(secret, &format!("https://{host}/v1/test")).is_ok());
        for url in [
            format!("http://{host}/"),
            format!("https://{host}:444/"),
            format!("https://{host}.example.org/"),
            format!("https://{host}@example.org/"),
            format!("https://user@{host}/"),
            "https://[fd12::1]/".into(),
            "https://127.0.0.1/".into(),
            "https://example.org/".into(),
        ] {
            assert!(heimdall_auth_origin(secret, &url).is_err(), "{url}");
        }
    }
    assert!(heimdall_auth_origin("unknown", "https://api.github.com/").is_err());
}

#[tokio::test]
async fn execute_rejects_secret_misdirection_and_private_addresses_before_transport() {
    use std::sync::Arc;
    use zeroclaw_api::tool::Tool;
    use zeroclaw_config::policy::SecurityPolicy;
    use zeroclaw_tools::http_request::HttpRequestTool;
    let tool = HttpRequestTool::new(
        Arc::new(SecurityPolicy::default()),
        vec!["*".into()],
        1024,
        1,
        false,
        vec![],
        vec![],
    )
    .unwrap();
    let result = tool
        .execute(
            serde_json::json!({"url":"https://not-a-real-host.invalid/", "auth_secret":"github"}),
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert_eq!(
        result.error.as_deref(),
        Some("Named secret is restricted to its approved HTTPS origin")
    );
    for url in ["http://[fd12::1]/", "http://[::1]/", "http://127.0.0.1/"] {
        let result = tool.execute(serde_json::json!({"url":url})).await.unwrap();
        assert!(!result.success, "{url}");
        assert!(result.error.unwrap().contains("private"), "{url}");
    }
}
