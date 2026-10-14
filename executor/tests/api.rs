use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use heimdall_executor::server::{App, router};
use tower::ServiceExt;
#[tokio::test]
async fn authentication_and_failed_readiness_are_enforced() {
    let app = App::new("x".repeat(32), false);
    let body = r#"{"jsonrpc":"2.0","id":"t","method":"initialize","params":{}}"#;
    let response = router(app.clone())
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router(app)
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", "x".repeat(32)))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
#[tokio::test]
async fn oversized_requests_are_rejected() {
    let app = App::new("x".repeat(32), false);
    for (body, expected) in [(" ".repeat(131073), StatusCode::PAYLOAD_TOO_LARGE)] {
        let response = router(app.clone())
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", "x".repeat(32)))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn private_listener_serves_ipv4_and_ipv6_and_rejects_unauthenticated_requests() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener =
        tokio::net::TcpListener::bind(heimdall_executor::server::listen_address("0").unwrap())
            .await
            .unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        axum::serve(listener, router(App::new("x".repeat(32), true)))
            .await
            .unwrap();
    });
    for host in ["127.0.0.1", "::1"] {
        let mut stream = tokio::net::TcpStream::connect((host, port)).await.unwrap();
        stream.write_all(b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            stream.read_to_string(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    }
    task.abort();
}

#[tokio::test]
async fn only_mcp_is_available_for_tool_requests() {
    let response = router(App::new("x".repeat(32), true))
        .oneshot(Request::post("/execute")
            .header("authorization", format!("Bearer {}", "x".repeat(32)))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"run","id":"removed","tool":"bash","arguments":{"command":"true"}}"#)).unwrap())
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
