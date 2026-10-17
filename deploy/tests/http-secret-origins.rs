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

#[test]
fn operational_headers_keep_values_without_exposing_other_values() {
    use reqwest::header::{HeaderMap, HeaderValue};
    use zeroclaw_tools::http_request::heimdall_response_headers;
    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
    headers.insert("retry-after", HeaderValue::from_static("60"));
    headers.insert(
        "set-cookie",
        HeaderValue::from_static("TEST_private_cookie"),
    );
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer TEST_private_token"),
    );
    headers.insert("x-custom", HeaderValue::from_static("TEST_private_content"));
    let output = heimdall_response_headers(&headers);
    assert!(output.contains("x-ratelimit-remaining: 0"));
    assert!(output.contains("retry-after: 60"));
    assert!(output.contains("set-cookie: ***REDACTED***"));
    assert!(!output.contains("TEST_private"));
}

#[tokio::test]
async fn malformed_request_bodies_fail_before_transport_without_echoing_content() {
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
    for body in [
        serde_json::json!({"secret":"TEST_private_body"}),
        serde_json::json!([]),
        serde_json::json!(null),
        serde_json::json!(42),
    ] {
        let result = tool
            .execute(serde_json::json!({
                "url":"https://not-a-real-host.invalid/", "method":"POST", "body":body
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("'body' must be a JSON-encoded string; serialize JSON objects before sending")
        );
    }
}
