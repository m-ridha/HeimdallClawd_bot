use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use heimdall_executor::server::{App, router};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
async fn call(ready: bool, auth: bool, body: Value) -> (StatusCode, Value) {
    let mut request = Request::post("/mcp").header("content-type", "application/json");
    if auth {
        request = request.header("authorization", format!("Bearer {}", "x".repeat(32)));
    }
    let response = router(App::new("x".repeat(32), ready))
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn mcp_requires_authentication_and_readiness() {
    let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}});
    assert_eq!(
        call(true, false, request.clone()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(false, true, request).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
#[tokio::test]
async fn mcp_initializes_and_exposes_only_workspace_tools() {
    let (_,init)=call(true,true,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}})).await;
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    let (_, list) = call(
        true,
        true,
        json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}),
    )
    .await;
    let tools = list["result"]["tools"].as_array().expect("tools list");
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "bash",
            "file_read",
            "file_read_base64",
            "file_write_base64",
            "file_write"
        ]
    );
    assert!(
        tools
            .iter()
            .all(|t| t["inputSchema"]["additionalProperties"] == false)
    );
}
#[tokio::test]
async fn mcp_rejects_unknown_methods_bad_tools_and_paths_without_execution() {
    for (method, params, code) in [
        ("unknown", json!({}), -32601),
        ("tools/call", json!({"name":"shell","arguments":{}}), -32602),
        (
            "tools/call",
            json!({"name":"file_read","arguments":{"path":"../secret"}}),
            -32602,
        ),
    ] {
        let (_, reply) = call(
            true,
            true,
            json!({"jsonrpc":"2.0","id":2,"method":method,"params":params}),
        )
        .await;
        assert_eq!(reply["error"]["code"], code);
    }
    assert_eq!(
        call(
            true,
            true,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert_eq!(call(true,true,json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"bash","arguments":{"command":"touch should-not-exist"}}})).await.0,StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn mcp_rejects_unbounded_ids_and_extra_arguments() {
    for id in [
        Value::Null,
        json!(true),
        json!({"id":1}),
        json!("a".repeat(129)),
    ] {
        assert_eq!(
            call(
                true,
                true,
                json!({"jsonrpc":"2.0","id":id,"method":"tools/list"})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (_, reply) = call(true,true,json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"bash","arguments":{"command":"true","environment":{"SECRET":"value"}}}})).await;
    assert_eq!(reply["error"]["code"], -32602);
    assert_eq!(call(true,true,json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"not-active"}})).await.0, StatusCode::ACCEPTED);
}
