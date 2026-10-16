//! Synthetic Telegram/MCP failures through the real terminal logging subscriber.
use serde_json::json;
use std::sync::Arc;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::body_partial_json};
use zeroclaw_api::channel::{Channel, SendMessage};
const TOKEN: &str = "123456:TEST_token_abcdefghijklmnop";
const MESSAGE: &str = "HEIMDALL_SYNTHETIC_PRIVATE_MESSAGE";

#[tokio::test]
async fn heimdall_logging_child() {
    let Ok(verbose) = std::env::var("HEIMDALL_LOG_FIXTURE") else {
        return;
    };
    zeroclaw_log::install_global_subscriber(Some("warn"), "warn", verbose == "1");
    let endpoint = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({"ok":false,"description":"synthetic rejection"})),
        )
        .mount(&endpoint)
        .await;
    let channel = zeroclaw_channels::telegram::TelegramChannel::new(
        TOKEN.into(),
        "fixture",
        Arc::new(|| vec!["1001".into()]),
        false,
    )
    .with_api_base(endpoint.uri());
    assert!(
        channel
            .send(&SendMessage::new(MESSAGE, "1001"))
            .await
            .is_err()
    );
    let mcp = MockServer::start().await;
    for (method, id, result) in [
        ("initialize", 1, json!({"capabilities":{"tools":{}}})),
        (
            "tools/list",
            2,
            json!({"tools":[{"name":"bash","inputSchema":{"type":"object"}}]}),
        ),
        (
            "tools/call",
            3,
            json!({"isError":true,"content":[{"type":"text","text":MESSAGE}]}),
        ),
    ] {
        Mock::given(body_partial_json(json!({"method":method})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"jsonrpc":"2.0","id":id,"result":result})),
            )
            .mount(&mcp)
            .await;
    }
    Mock::given(body_partial_json(
        json!({"method":"notifications/initialized"}),
    ))
    .respond_with(ResponseTemplate::new(202))
    .mount(&mcp)
    .await;
    let config = serde_json::from_value(
        json!({"name":"fixture","transport":"http","url":mcp.uri(),"tool_timeout_secs":5}),
    )
    .unwrap();
    let client = zeroclaw_tools::mcp_client::McpServer::connect(config)
        .await
        .unwrap();
    assert!(
        client
            .call_tool("bash", json!({"command":"fixture"}))
            .await
            .is_err()
    );
}

#[test]
fn heimdall_terminal_logging_excludes_private_payloads() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "heimdall_logging_child", "--nocapture"])
        .env(
            "HEIMDALL_LOG_FIXTURE",
            std::env::var("HEIMDALL_TEST_DAEMON_VERBOSE").unwrap_or_else(|_| "0".into()),
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "synthetic logging child failed");
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    for forbidden in [TOKEN, MESSAGE] {
        assert!(
            !out.contains(forbidden) && !err.contains(forbidden),
            "terminal logs contain synthetic private payload"
        );
    }
}
