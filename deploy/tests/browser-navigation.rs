//! Real Chromium requests must use the guarded proxy, including loopback redirects.
#![cfg(target_os = "linux")]
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{any, path},
};
use zeroclaw_api::tool::Tool;
use zeroclaw_config::policy::SecurityPolicy;
use zeroclaw_tools::browser::{BrowserTool, ComputerUseConfig};

#[tokio::test]
async fn heimdall_chromium_blocks_redirects_subresources_and_click_navigation() {
    tokio::time::timeout(Duration::from_secs(60), check_chromium())
        .await
        .unwrap();
}

async fn check_chromium() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let forbidden = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .mount(&forbidden)
        .await;
    let forbidden_port = forbidden.address().port();
    let source = MockServer::start().await;
    let body = format!(
        r#"<!doctype html><title>guarded fixture</title>
        <img src="http://localhost:{forbidden_port}/image">
        <script>fetch('http://localhost:{forbidden_port}/fetch').catch(() => {{}})</script>
        <a id="blocked" href="http://localhost:{forbidden_port}/click">blocked</a>"#
    );
    Mock::given(path("/page"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/html"))
        .expect(1)
        .mount(&source)
        .await;
    Mock::given(path("/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "Location",
            format!("http://localhost:{forbidden_port}/redirect-target"),
        ))
        .expect(1)
        .mount(&source)
        .await;

    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let driver_port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let driver_binary = std::env::var("HEIMDALL_TEST_CHROMEDRIVER")
        .unwrap_or_else(|_| "/usr/bin/chromedriver".into());
    let chrome_binary =
        std::env::var("HEIMDALL_TEST_CHROMIUM").unwrap_or_else(|_| "/usr/bin/chromium".into());
    let mut driver = tokio::process::Command::new(driver_binary)
        .arg(format!("--port={driver_port}"))
        .arg("--allowed-ips=127.0.0.1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, driver_port))
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                driver.try_wait().unwrap().is_none(),
                "ChromeDriver exited before readiness"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    // Only the fixture's exact IP is allowed. localhost is a different denied host.
    let browser = BrowserTool::new_with_backend(
        Arc::new(SecurityPolicy::default()),
        vec!["*".into()],
        None,
        "rust_native".into(),
        None,
        true,
        format!("http://127.0.0.1:{driver_port}"),
        Some(chrome_binary),
        ComputerUseConfig::default(),
        vec!["127.0.0.1".into()],
        // Configured NAT64 translation is covered by the proxy's unit tests;
        // this fixture only has loopback destinations.
        Vec::new(),
    )
    .unwrap();
    let open = browser
        .execute(json!({"action":"open", "url":format!("{}/page", source.uri())}))
        .await
        .unwrap();
    assert!(open.success, "{open:?}");
    let title = browser
        .execute(json!({"action":"get_title"}))
        .await
        .unwrap();
    assert!(
        title.success && title.output.as_str().contains("guarded fixture"),
        "{title:?}"
    );
    // Chrome may return an error page or a navigation error for a denied request.
    let _ = browser
        .execute(json!({"action":"click", "selector":"#blocked"}))
        .await;
    let _ = browser
        .execute(json!({"action":"open", "url":format!("{}/redirect", source.uri())}))
        .await;
    let closed = browser.execute(json!({"action":"close"})).await.unwrap();
    assert!(closed.success);
    drop(browser);
    driver.kill().await.unwrap();
    source.verify().await;
    assert!(
        forbidden.received_requests().await.unwrap().is_empty(),
        "a denied browser request reached loopback"
    );
}

#[tokio::test]
async fn heimdall_browser_nat64_rejects_translated_private_destination() {
    tokio::time::timeout(Duration::from_secs(60), check_nat64_rejection())
        .await
        .unwrap();
}

async fn check_nat64_rejection() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let driver_port = reservation.local_addr().unwrap().port();
    drop(reservation);

    let driver_binary = std::env::var("HEIMDALL_TEST_CHROMEDRIVER")
        .unwrap_or_else(|_| "/usr/bin/chromedriver".into());
    let chrome_binary =
        std::env::var("HEIMDALL_TEST_CHROMIUM").unwrap_or_else(|_| "/usr/bin/chromium".into());
    let mut driver = tokio::process::Command::new(driver_binary)
        .arg(format!("--port={driver_port}"))
        .arg("--allowed-ips=127.0.0.1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, driver_port))
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                driver.try_wait().unwrap().is_none(),
                "ChromeDriver exited before readiness"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    // A configured NAT64 prefix is passed through BrowserTool construction to
    // the native proxy. A destination inside the prefix that embeds a private
    // address (10.0.0.1) must be rejected before Chromium dials it.
    let browser = BrowserTool::new_with_backend(
        Arc::new(SecurityPolicy::default()),
        vec!["*".into()],
        None,
        "rust_native".into(),
        None,
        true,
        format!("http://127.0.0.1:{driver_port}"),
        Some(chrome_binary),
        ComputerUseConfig::default(),
        Vec::new(),
        vec!["2001:67c:2b0:db32:0:1::/96".into()],
    )
    .unwrap();

    let result = browser
        .execute(json!({
            "action": "open",
            "url": "http://[2001:67c:2b0:db32:0:1:a00:1]/image"
        }))
        .await
        .unwrap();

    assert!(
        !result.success,
        "translated private destination should be rejected: {result:?}"
    );

    driver.kill().await.unwrap();
}
