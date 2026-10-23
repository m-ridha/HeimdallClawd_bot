use super::*;
const FILTERS: &str = r#"[{"name":"github-review-request","sender":"notifications@github.com","contains":"requested your review on:","sop":"github-pr-review"}]"#;

#[test]
fn omitted_body_is_fetched_for_each_unresolved_sender_rule() {
    let filters = parse_filters(
        r#"[
        {"name":"subject-rule","sender":"notifications@github.com","contains":"subject match"},
        {"name":"body-rule","sender":"notifications@github.com","contains":"body match"}
    ]"#,
    )
    .unwrap();
    let mut message = json!({"from":"notifications@github.com","subject":"subject match","body_url":"https://unused.example/body"});
    assert!(filters[0].matches(&message));
    assert!(!filters[1].matches(&message));
    assert!(needs_body(&filters, &message));
    assert!(!needs_body(&filters[..1], &message));
    message["extracted_text"] = json!("body match");
    assert!(!needs_body(&filters, &message));
    let selected: Vec<_> = filters
        .iter()
        .filter(|f| f.matches(&message))
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(selected, ["subject-rule", "body-rule"]);
    message["extracted_text"] = json!("unmatched but already available body");
    assert!(!needs_body(&filters, &message));
    message.as_object_mut().unwrap().remove("extracted_text");
    message["from"] = json!("unrelated@example.com");
    assert!(!needs_body(&filters, &message));
}

#[test]
fn reading_ledger_does_not_rewrite_or_normalize_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    state(&path, |_| Ok(())).unwrap();
    let original = "{\n  \"enabled\": {},\n  \"events\": {}\n}\n";
    fs::write(&path, original).unwrap();
    state(&path, |s| {
        assert!(s["events"].as_object().unwrap().is_empty());
        Ok(())
    })
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    state(&path, |s| {
        s["enabled"]["subject-rule"] = json!(true);
        Ok(())
    })
    .unwrap();
    assert_ne!(fs::read_to_string(path).unwrap(), original);
}

#[tokio::test(flavor = "current_thread")]
async fn held_ledger_lock_does_not_block_the_async_worker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    state(&path, |_| Ok(())).unwrap();
    let settings = Arc::new(Settings {
        secret: vec![],
        token: String::new(),
        filters: vec![],
        state: path.clone(),
        paired_tokens: String::new(),
        authorization: None,
        gateway: String::new(),
        telegram: None,
        gateway_secret: String::new(),
    });
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        // SAFETY: valid owned fixture descriptor; dropping it releases the lock.
        assert_eq!(unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) }, 0);
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5))
    });
    ready_rx.await.unwrap();
    let waiting = tokio::spawn(async_state(settings, |_| Ok("read complete")));
    tokio::task::yield_now().await;
    // With a synchronous state call, this worker cannot resume until the holder
    // times out. Sending then fails, and the holder's returned result fails too.
    let released = release_tx.send(());
    let read = tokio::time::timeout(Duration::from_secs(3), waiting)
        .await
        .unwrap()
        .unwrap();
    let held = holder.join().unwrap();
    assert!(released.is_ok());
    assert!(held.is_ok());
    assert_eq!(read.unwrap(), "read complete");
}
// Ephemeral keys avoid embedding signing material; assertions still use fixed
// messages/timestamps and share the same generated key within each fixture.
fn test_signing_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).expect("OS randomness unavailable for test fixture");
    key
}

fn signed(secret: &[u8], body: &[u8], stamp: u64) -> hyper::HeaderMap {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(format!("msg_fixture.{stamp}.").as_bytes());
    mac.update(body);
    let mut headers = hyper::HeaderMap::new();
    headers.insert("svix-id", "msg_fixture".parse().unwrap());
    headers.insert("svix-timestamp", stamp.to_string().parse().unwrap());
    headers.insert(
        "svix-signature",
        format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
            .parse()
            .unwrap(),
    );
    headers
}
#[test]
fn sender_and_content_are_both_required() {
    let filters = parse_filters(FILTERS).unwrap();
    for from in [
        "notifications@github.com",
        "GitHub <notifications@github.com>",
        "NOTIFICATIONS@GITHUB.COM",
        "\"GitHub, Reviews\" <notifications@github.com>",
        "  \"GitHub, Reviews\" <notifications@github.com>  ",
        "\"GitHub; Reviews\" <notifications@github.com>",
        "\"GitHub \\\"Reviews\\\", Team\" <notifications@github.com>",
    ] {
        assert!(
            filters[0].matches(&json!({"from":from,"text":"Alice REQUESTED YOUR REVIEW ON: test"}))
        );
    }
    for from in [
        "notifications@github.com.attacker.test",
        "notifications@github.com <other@example.com>",
        "other@example.com",
        "notifications@github.com, other@example.com",
        "other@example.com, GitHub <notifications@github.com>",
        "\"Other\", GitHub <notifications@github.com>",
        "\"Other\"; GitHub <notifications@github.com>",
        "\"Unclosed, Team <notifications@github.com>",
        "\"GitHub, Reviews\" <notifications@github.com>, other@example.com",
        "\"GitHub, Reviews\" <notifications@github.com;other@example.com>",
        "\"GitHub,\r\n Reviews\" <notifications@github.com>",
    ] {
        assert!(!filters[0].matches(&json!({"from":from,"text":"requested your review on:"})));
    }
    assert!(
        !filters[0].matches(
            &json!({"from":"notifications@github.com","text":"commented on a pull request"})
        )
    );
    assert!(!filters[0].matches(&Value::Null));
    assert!(parse_filters("[]").is_err());
    assert!(parse_filters(r#"[{"name":"x","sender":"x@example.com","contains":""}]"#).is_err());
}
#[test]
fn svix_authenticates_raw_body_timestamp_and_rotation_signatures() {
    let secret = test_signing_key();
    let body = b"{\"event_type\":\"message.received\"}";
    let mut headers = signed(&secret, body, 1000);
    assert!(verify(&secret, &headers, body, 1000));
    assert!(!verify(&secret, &headers, body, 1301));
    assert!(!verify(&secret, &headers, body, 699));
    assert!(!verify(&secret, &headers, b"tampered", 1000));
    let mut wrong_key = secret;
    wrong_key[0] ^= 1;
    assert!(!verify(&wrong_key, &headers, body, 1000));
    headers.insert(
        "svix-signature",
        format!("v1,invalid {}", headers["svix-signature"].to_str().unwrap())
            .parse()
            .unwrap(),
    );
    assert!(verify(&secret, &headers, body, 1000));
    headers.remove("svix-timestamp");
    assert!(!verify(&secret, &headers, body, 1000));
}
#[test]
fn state_persists_disabled_defaults_and_rejects_symlink_or_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    state(&path, |s| {
        assert!(s["enabled"].as_object().unwrap().is_empty());
        s["enabled"]["github-review-request"] = json!(true);
        Ok(())
    })
    .unwrap();
    state(&path, |s| {
        assert_eq!(s["enabled"]["github-review-request"], true);
        Ok(())
    })
    .unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(state(&link, |_| Ok(())).is_err());
    fs::write(&path, "broken").unwrap();
    assert!(state(&path, |_| Ok(())).is_err());
}
#[test]
fn native_token_override_contains_only_adapter_digest_and_preserves_clients() {
    let raw = "fixture_adapter_bearer_01234567890123456789";
    let result = paired_tokens(Some(r#"["existing-client"]"#), raw).unwrap();
    assert!(!result.contains(raw));
    assert!(result.contains("existing-client"));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&result).unwrap().len(),
        2
    );
    assert!(paired_tokens(Some("invalid"), raw).is_err());
}
#[test]
fn management_enables_only_known_rules_and_survives_reload() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings {
        secret: vec![],
        token: String::new(),
        filters: parse_filters(FILTERS).unwrap(),
        state: dir.path().join("state"),
        paired_tokens: "[]".into(),
        authorization: None,
        gateway: String::new(),
        telegram: None,
        gateway_secret: String::new(),
    };
    let status: Value =
        serde_json::from_str(&manage(&settings, "filters", &json!({})).unwrap()).unwrap();
    assert_eq!(status["filters"][0]["enabled"], false);
    manage(
        &settings,
        "set_enabled",
        &json!({"name":"github-review-request","enabled":true}),
    )
    .unwrap();
    let status: Value =
        serde_json::from_str(&manage(&settings, "filters", &json!({})).unwrap()).unwrap();
    assert_eq!(status["filters"][0]["enabled"], true);
    assert!(
        manage(
            &settings,
            "set_enabled",
            &json!({"name":"unknown","enabled":true})
        )
        .is_err()
    );
    assert!(
        manage(
            &settings,
            "set_enabled",
            &json!({"name":"github-review-request","enabled":"true"})
        )
        .is_err()
    );
    manage(
        &settings,
        "set_enabled",
        &json!({"name":"github-review-request","enabled":false}),
    )
    .unwrap();
    let status: Value =
        serde_json::from_str(&manage(&settings, "filters", &json!({})).unwrap()).unwrap();
    assert_eq!(status["filters"][0]["enabled"], false);
}
async fn ingress(settings: Settings) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let settings = Arc::new(settings);
    let jobs = Arc::new(Semaphore::new(2));
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let settings = settings.clone();
            let jobs = jobs.clone();
            tokio::spawn(async move {
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        TokioIo::new(socket),
                        service_fn(move |req| {
                            let settings = settings.clone();
                            let jobs = jobs.clone();
                            async move { Ok::<_, Infallible>(receive(req, settings, jobs).await) }
                        }),
                    )
                    .await;
            });
        }
    });
    (url, task)
}
#[tokio::test]
async fn ingress_rejects_forgery_and_ignores_disabled_unmatched_duplicate_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    let key = test_signing_key();
    let (url, task) = ingress(Settings {
        secret: key.to_vec(),
        token: "fixture_not_a_live_token".into(),
        filters: parse_filters(FILTERS).unwrap(),
        state: path.clone(),
        paired_tokens: "[]".into(),
        authorization: None,
        gateway: "http://127.0.0.1:3887".into(),
        telegram: None,
        gateway_secret: String::new(),
    })
    .await;
    let body = serde_json::to_vec(&json!({"event_type":"message.received","event_id":"evt_fixture","message":{"from":"notifications@github.com","text":"requested your review on:"}})).unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .post(&url)
            .body(body.clone())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(&url)
            .headers(signed(&key, &body, now()))
            .body(body.clone())
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    state(&path, |s| {
        assert!(s["events"].as_object().unwrap().is_empty());
        s["enabled"]["github-review-request"] = json!(true);
        s["events"]["evt_fixture"] = json!({"status":"reserved","time":1000});
        Ok(())
    })
    .unwrap();
    assert_eq!(
        client
            .post(&url)
            .headers(signed(&key, &body, now()))
            .body(body)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let unrelated = br#"{"event_type":"message.received","event_id":"evt_other","message":{"from":"other@example.com","text":"requested your review on:"}}"#;
    assert_eq!(
        client
            .post(&url)
            .headers(signed(&key, unrelated, now()))
            .body(unrelated.to_vec())
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    state(&path, |s| {
        assert_eq!(s["events"].as_object().unwrap().len(), 1);
        Ok(())
    })
    .unwrap();
    task.abort();
}
#[tokio::test]
async fn proxy_preserves_requests_and_blocks_loopback_admin_escalation() {
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_address = backend.local_addr().unwrap().to_string();
    let remote = tokio::spawn(async move {
        for _ in 0..10 {
            let (socket, _) = backend.accept().await.unwrap();
            tokio::spawn(async move {
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        TokioIo::new(socket),
                        service_fn(|req: Request<Incoming>| async move {
                            assert_eq!(req.headers()["authorization"], "Bearer fixture_client");
                            assert_ne!(req.headers()["x-forwarded-for"], "attacker");
                            let method = req.method().clone();
                            let uri = req.uri().to_string();
                            let body = req.into_body().collect().await.unwrap().to_bytes();
                            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(format!(
                                "{method} {uri}:{}",
                                String::from_utf8_lossy(&body)
                            )))))
                        }),
                    )
                    .await;
            });
        }
    });
    let public = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", public.local_addr().unwrap());
    let front = tokio::spawn(async move {
        loop {
            let (socket, peer) = public.accept().await.unwrap();
            let backend = backend_address.clone();
            tokio::spawn(async move {
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        TokioIo::new(socket),
                        service_fn(move |req| {
                            let backend = backend.clone();
                            async move { proxy(req, peer, &backend, None).await }
                        }),
                    )
                    .with_upgrades()
                    .await;
            });
        }
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for path in [
        "/",
        "/pair",
        "/pair/code",
        "/acp?session=fixture",
        "/api/sops/runs",
        "/admin/reload",
        "/admin/sop/pending",
        "/admin/sop/logs?run_id=fixture",
        "/admin/sop/approve",
        "/admin/sop/deny",
    ] {
        let method = if matches!(path, "/" | "/pair/code" | "/admin/sop/pending")
            || path.starts_with("/admin/sop/logs?")
        {
            reqwest::Method::GET
        } else {
            reqwest::Method::POST
        };
        let result = client
            .request(method.clone(), format!("{url}{path}"))
            .header("Authorization", "Bearer fixture_client")
            .header("X-Forwarded-For", "attacker")
            .body("fixture body")
            .send()
            .await
            .unwrap();
        assert_eq!(
            result.text().await.unwrap(),
            format!("{method} {path}:fixture body")
        );
    }
    for path in ["/admin/shutdown", "/admin/paircode", "/admin/paircode/new"] {
        for method in [reqwest::Method::GET, reqwest::Method::POST] {
            assert_eq!(
                client
                    .request(method, format!("{url}{path}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403,
                "blocked local-admin route: {path}"
            );
        }
    }
    remote.await.unwrap();
    front.abort();
}

#[tokio::test]
async fn matching_event_dispatches_once_and_retains_only_metadata() {
    event_dispatch_case(
        StatusCode::OK,
        br#"{"status":"accepted","source":"webhook","path":"/sop/webhook/github-pr-review","results":[{"status":"started","sop":"github-pr-review","run_id":"fixture-run"}]}"#,
        "accepted",
        "complete",
    )
    .await;
}

#[tokio::test]
async fn dispatch_failures_retain_safe_diagnostics_and_suppress_replay() {
    event_dispatch_case(
        StatusCode::REQUEST_TIMEOUT,
        b"PRIVATE_ERROR fixture_not_a_live_token",
        "uncertain",
        "gateway",
    )
    .await;
    event_dispatch_case(
        StatusCode::UNAUTHORIZED,
        b"PRIVATE_ERROR",
        "uncertain",
        "gateway",
    )
    .await;
    for body in [
        br#"{"response":"old synchronous result"}"#.as_slice(),
        br#"{"status":"accepted","source":"webhook","path":"/sop/webhook/github-pr-review","results":[{"status":"skipped","sop":"github-pr-review","reason":"busy"}]}"#,
        br#"{"status":"accepted","source":"webhook","path":"/sop/webhook/github-pr-review","results":[{"status":"started","sop":"wrong-sop","run_id":"fixture-run"}]}"#,
    ] {
        event_dispatch_case(StatusCode::OK, body, "uncertain", "gateway").await;
    }
    event_dispatch_case(
        StatusCode::NOT_FOUND,
        b"no matching SOP",
        "uncertain",
        "gateway",
    )
    .await;
    event_dispatch_case(
        StatusCode::OK,
        br#"{"error":"PRIVATE_ERROR"}"#,
        "uncertain",
        "gateway",
    )
    .await;
}

async fn event_dispatch_case(
    status: StatusCode,
    response_body: &'static [u8],
    expected_status: &str,
    expected_stage: &str,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    state(&path, |s| {
        s["enabled"]["github-review-request"] = json!(true);
        Ok(())
    })
    .unwrap();
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway = format!("http://{}", backend.local_addr().unwrap());
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let model = tokio::spawn(async move {
        let (socket, _) = backend.accept().await.unwrap();
        let tx = Arc::new(std::sync::Mutex::new(Some(done_tx)));
        hyper::server::conn::http1::Builder::new()
            .serve_connection(
                TokioIo::new(socket),
                service_fn(move |req: Request<Incoming>| {
                    let tx = tx.clone();
                    async move {
                        assert_eq!(req.method(), "POST");
                        assert_eq!(
                            req.headers()["authorization"],
                            "Bearer fixture_not_a_live_token"
                        );
                        assert_eq!(req.headers()["x-idempotency-key"], "evt_new");
                        assert!(req.headers().get("x-session-id").is_none());
                        let uri = req.uri().to_string();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        let value: Value = serde_json::from_slice(&body).unwrap();
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send((uri, value));
                        }
                        Ok::<_, Infallible>(
                            Response::builder()
                                .status(status)
                                .body(Full::new(Bytes::from_static(response_body)))
                                .unwrap(),
                        )
                    }
                }),
            )
            .await
            .unwrap();
    });
    let key = test_signing_key();
    let (url, front) = ingress(Settings {
        secret: key.to_vec(),
        token: "fixture_not_a_live_token".into(),
        filters: parse_filters(FILTERS).unwrap(),
        state: path.clone(),
        paired_tokens: "[]".into(),
        authorization: None,
        gateway,
        telegram: None,
        gateway_secret: String::new(),
    })
    .await;
    let body=serde_json::to_vec(&json!({"event_type":"message.received","event_id":"evt_new","message":{"from":"notifications@github.com","text":"Alice requested your review on: PRIVATE_BODY","subject":"Fixture subject"}})).unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .post(&url)
            .headers(signed(&key, &body, now()))
            .body(body.clone())
            .send()
            .await
            .unwrap()
            .status(),
        202
    );
    let (uri, prompt) = tokio::time::timeout(Duration::from_secs(3), done_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(uri, "/sop/webhook/github-pr-review");
    assert_eq!(prompt["rules"], json!(["github-review-request"]));
    assert!(prompt.get("message").is_none());
    assert!(prompt.get("subject").is_none());
    assert!(!prompt.to_string().contains("PRIVATE_BODY"));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let finished = state(
                &path,
                |s| Ok(s["events"]["evt_new"]["status"] != "reserved"),
            )
            .unwrap();
            if finished {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    state(&path, |s| {
        assert_eq!(s["events"]["evt_new"]["status"], expected_status);
        assert_eq!(
            s["events"]["evt_new"]["diagnostic"]["stage"],
            expected_stage
        );
        if expected_status == "accepted" {
            assert_eq!(
                s["events"]["evt_new"]["runs"],
                json!([{"sop":"github-pr-review","run_id":"fixture-run","status":"started"}])
            );
            assert_eq!(
                s["events"]["evt_new"]["diagnostic"]["execution_completed"],
                false
            );
        }
        if !status.is_success() {
            assert_eq!(
                s["events"]["evt_new"]["diagnostic"]["http_status"],
                status.as_u16()
            );
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        client
            .post(&url)
            .headers(signed(&key, &body, now()))
            .body(body)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let saved = fs::read_to_string(path).unwrap();
    assert!(!saved.contains("PRIVATE_BODY"));
    assert!(!saved.contains("PRIVATE_ERROR"));
    assert!(!saved.contains("Fixture subject"));
    assert!(!saved.contains("fixture_not_a_live_token"));
    model.abort();
    front.abort();
}

#[tokio::test]
async fn proxy_relays_upgraded_connections_bidirectionally() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = backend.local_addr().unwrap().to_string();
    let server = tokio::spawn(async move {
        let (socket, _) = backend.accept().await.unwrap();
        hyper::server::conn::http1::Builder::new()
            .serve_connection(
                TokioIo::new(socket),
                service_fn(|mut req: Request<Incoming>| async move {
                    let upgrade = hyper::upgrade::on(&mut req);
                    tokio::spawn(async move {
                        let mut io = TokioIo::new(upgrade.await.unwrap());
                        let mut data = [0; 4];
                        io.read_exact(&mut data).await.unwrap();
                        assert_eq!(&data, b"ping");
                        io.write_all(b"pong").await.unwrap();
                    });
                    let mut r = response(StatusCode::SWITCHING_PROTOCOLS, "");
                    r.headers_mut()
                        .insert("connection", "upgrade".parse().unwrap());
                    r.headers_mut()
                        .insert("upgrade", "fixture".parse().unwrap());
                    Ok::<_, Infallible>(r)
                }),
            )
            .with_upgrades()
            .await
            .unwrap();
    });
    let public = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = public.local_addr().unwrap();
    let front = tokio::spawn(async move {
        let (socket, peer) = public.accept().await.unwrap();
        hyper::server::conn::http1::Builder::new()
            .serve_connection(
                TokioIo::new(socket),
                service_fn(move |req| {
                    let target = target.clone();
                    async move { proxy(req, peer, &target, None).await }
                }),
            )
            .with_upgrades()
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client
        .get(format!("http://{address}/ws/chat?session=fixture"))
        .header("Connection", "upgrade")
        .header("Upgrade", "fixture")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 101);
    let mut io = response.upgrade().await.unwrap();
    io.write_all(b"ping").await.unwrap();
    let mut data = [0; 4];
    tokio::time::timeout(Duration::from_secs(3), io.read_exact(&mut data))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&data, b"pong");
    server.await.unwrap();
    front.await.unwrap();
}

#[test]
fn digest_hex_matches_sha256_vectors_and_paired_token_format() {
    assert_eq!(
        sha256_hex(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(sha256_hex("abc"), expected);
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&paired_tokens(None, "abc").unwrap()).unwrap(),
        [expected]
    );
    assert_eq!(
        serde_json::from_str::<Vec<String>>(
            &paired_tokens(Some(&format!("[\"{expected}\"]")), "abc").unwrap()
        )
        .unwrap(),
        [expected]
    );
}

#[test]
fn optional_sop_is_strict_operator_configuration() {
    assert_eq!(
        parse_filters(FILTERS).unwrap()[0].sop.as_deref(),
        Some("github-pr-review")
    );
    let legacy = r#"[{"name":"notice","sender":"sender@example.com","contains":"text"}]"#;
    assert!(parse_filters(legacy).unwrap()[0].sop.is_none());
    for name in ["", "../review", "review --unsafe", "review\n", "a.b"] {
        let value =
            json!([{"name":"notice","sender":"sender@example.com","contains":"text","sop":name}]);
        assert!(parse_filters(&value.to_string()).is_err());
    }
    let unknown =
        r#"[{"name":"notice","sender":"sender@example.com","contains":"text","action":"unsafe"}]"#;
    assert!(parse_filters(unknown).is_err());
}

#[test]
fn dispatch_diagnostics_distinguish_delivery_from_unknown_execution() {
    assert_eq!(DISPATCH_TIMEOUT, Duration::from_secs(35));
    assert_eq!(
        DispatchError::GatewayHttp(408).diagnostic(),
        json!({"stage":"gateway","phase":"response_status","kind":"http","http_status":408,"gateway_completed":false})
    );
    assert_eq!(
        DispatchError::TelegramHttp(403).diagnostic(),
        json!({"stage":"telegram","phase":"response_status","kind":"http","http_status":403,"gateway_completed":true})
    );
    assert_eq!(
        DispatchError::GatewayTransport(true).diagnostic()["kind"],
        "timeout"
    );
    assert_eq!(
        DispatchError::GatewayTransport(false).diagnostic()["kind"],
        "transport"
    );
    assert_eq!(
        DispatchError::TelegramResponse(200).diagnostic()["gateway_completed"],
        true
    );
}

#[test]
fn filter_status_exposes_ten_recent_safe_diagnostics_without_raw_event_ids() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    state(&path, |data| {
        for n in 1..=12 {
            data["events"][format!("fixture{n}")] = json!({"time":n,"status":"uncertain","diagnostic":{"stage":"gateway","phase":"response_status","kind":"http","http_status":408,"gateway_completed":false}});
        }
        Ok(())
    }).unwrap();
    let before = fs::read(&path).unwrap();
    let settings = Settings {
        secret: vec![],
        token: String::new(),
        filters: parse_filters(FILTERS).unwrap(),
        state: path.clone(),
        paired_tokens: String::new(),
        authorization: None,
        gateway: String::new(),
        telegram: None,
        gateway_secret: String::new(),
    };
    let text = manage(&settings, "filters", &json!({})).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["recent_events"].as_array().unwrap().len(), 10);
    assert_eq!(value["recent_events"][0]["time"], 12);
    assert_eq!(value["recent_events"][9]["time"], 3);
    assert_eq!(value["recent_events"][0]["diagnostic"]["http_status"], 408);
    assert_eq!(
        value["recent_events"][0]["event_ref"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(value["filters"][0]["sop"], "github-pr-review");
    assert_eq!(value["events"]["uncertain"], 12);
    assert!(!text.contains("fixture12"));
    assert_eq!(before, fs::read(path).unwrap());
}

#[derive(Clone, Copy)]
enum BodyFixture {
    Timeout,
    Disconnect,
    InvalidJson,
    TooLarge,
}

async fn response_body_fixture(
    fixture: BodyFixture,
) -> (
    reqwest::Client,
    reqwest::Response,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<io::Result<()>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (release, released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut headers = Vec::new();
        let mut buffer = [0; 1024];
        while !headers.windows(4).any(|b| b == b"\r\n\r\n") {
            let n = socket.read(&mut buffer).await?;
            if n == 0 || headers.len() + n > 16_384 {
                return Err(io::Error::other("fixture request missing headers"));
            }
            headers.extend_from_slice(&buffer[..n]);
        }
        let body = match fixture {
            BodyFixture::TooLarge => vec![b'x'; 65_537],
            _ => b"PRIVATE_ERROR".to_vec(),
        };
        let length = match fixture {
            BodyFixture::Timeout | BodyFixture::Disconnect => body.len() + 100,
            _ => body.len(),
        };
        socket
            .write_all(
                format!(
                    "HTTP/1.1 201 Created\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await?;
        socket.write_all(&body).await?;
        if matches!(fixture, BodyFixture::Timeout) {
            let _ = released.await;
        }
        Ok(())
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .read_timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let response = client
        .get(format!("http://{addr}/fixture"))
        .send()
        .await
        .unwrap();
    (client, response, release, server)
}

#[tokio::test]
async fn body_read_diagnostics_preserve_status_cause_and_stage_without_payloads() {
    for (fixture, kind) in [
        (BodyFixture::Timeout, "timeout"),
        (BodyFixture::Disconnect, "transport"),
        (BodyFixture::InvalidJson, "invalid_response"),
        (BodyFixture::TooLarge, "response_too_large"),
    ] {
        for telegram in [false, true] {
            let (_client, response, release, server) = response_body_fixture(fixture).await;
            let result = if telegram {
                validate_telegram_response(response).await
            } else {
                read_sop_dispatch(
                    response,
                    "github-pr-review",
                    "/sop/webhook/github-pr-review",
                )
                .await
                .map(|_| ())
            };
            let diagnostic = result.unwrap_err().diagnostic();
            assert_eq!(
                diagnostic,
                json!({
                    "stage":if telegram { "telegram" } else { "gateway" },
                    "phase":"response_body", "kind":kind, "http_status":201,
                    "gateway_completed":telegram,
                })
            );
            assert!(!diagnostic.to_string().contains("PRIVATE_ERROR"));
            let _ = release.send(());
            tokio::time::timeout(Duration::from_secs(3), server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        }
    }
}

#[tokio::test]
async fn notification_only_rule_never_contacts_gateway() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Arc::new(Settings {
        secret: test_signing_key().to_vec(),
        token: "fixture_not_a_live_token".into(),
        filters: parse_filters(
            r#"[{"name":"notice","sender":"sender@example.com","contains":"text"}]"#,
        )
        .unwrap(),
        state: dir.path().join("state"),
        paired_tokens: "[]".into(),
        authorization: None,
        // An attempted request would fail, so this tests the absence of dispatch.
        gateway: "http://127.0.0.1:0".into(),
        telegram: None,
        gateway_secret: String::new(),
    });
    dispatch(&settings, "fixture", &json!({}), &[])
        .await
        .unwrap();
    assert!(!settings.state.exists());
}

#[test]
fn template_rules_are_the_default_and_use_strict_shared_validation() {
    let filters =
        template_filters(include_str!("../../../deploy/bot/config.template.toml")).unwrap();
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].name, "github-review-request");
    assert_eq!(filters[0].sender, "notifications@github.com");
    assert_eq!(filters[0].contains, "requested your review on:");
    assert_eq!(filters[0].sop.as_deref(), Some("github-pr-review"));
    for invalid in [
        "",
        "[heimdall.agentmail]\nfilters = 'wrong-type'",
        "[[heimdall.agentmail.filters]]\nname = 'bad'\nsender = 'sender@example.com'\ncontains = 'text'\nsop = '../escape'",
        "[[heimdall.agentmail.filters]]\nname = 'bad'\nsender = 'sender@example.com'\ncontains = 'text'\nunknown = true",
    ] {
        assert!(template_filters(invalid).is_err());
    }
}

#[test]
fn ingress_enablement_is_a_required_template_boolean() {
    assert!(template_enabled(include_str!("../../../deploy/bot/config.template.toml")).unwrap());
    assert!(!template_enabled("[heimdall.agentmail]\nenabled = false").unwrap());
    for invalid in [
        "",
        "[heimdall.agentmail]\nenabled = '1'",
        "[heimdall.agentmail]\nenabled = 1",
    ] {
        assert!(template_enabled(invalid).is_err());
    }
}

#[test]
fn startup_errors_identify_configuration_without_echoing_credentials() {
    let fixtures = [
        (
            "HEIMDALL_AGENTMAIL_WEBHOOK_SECRET",
            "private-fixture",
            "AgentMail startup: HEIMDALL_AGENTMAIL_WEBHOOK_SECRET must start with whsec_",
        ),
        (
            "HEIMDALL_AGENTMAIL_WEBHOOK_SECRET",
            "whsec_private-fixture!",
            "AgentMail startup: HEIMDALL_AGENTMAIL_WEBHOOK_SECRET must contain standard Base64 after whsec_",
        ),
        (
            "HEIMDALL_AGENTMAIL_WEBHOOK_SECRET",
            "whsec_YQ==",
            "AgentMail startup: HEIMDALL_AGENTMAIL_WEBHOOK_SECRET must decode to at least 16 bytes",
        ),
        (
            "HEIMDALL_AGENTMAIL_GATEWAY_TOKEN",
            "private-fixture\n",
            "AgentMail startup: HEIMDALL_AGENTMAIL_GATEWAY_TOKEN must be 32-1024 printable ASCII characters without spaces or newlines",
        ),
        (
            "AGENTMAIL_AUTHORIZATION",
            "private-fixture",
            "AgentMail startup: AGENTMAIL_AUTHORIZATION must start with Bearer followed by a space and contain at most 1024 printable ASCII characters",
        ),
        (
            "ZEROCLAW_gateway__paired_tokens",
            "private-fixture",
            "AgentMail startup: ZEROCLAW_gateway__paired_tokens must be a JSON array of at most 64 strings, each at most 1024 bytes",
        ),
    ];
    for (name, value, expected) in fixtures {
        let lookup = |key: &str| {
            if key == name {
                return Ok(value.to_string());
            }
            match key {
                "HEIMDALL_AGENTMAIL_WEBHOOK_SECRET" => {
                    Ok(format!("whsec_{}", STANDARD.encode([0_u8; 32])))
                }
                "HEIMDALL_AGENTMAIL_GATEWAY_TOKEN" => Ok("a".repeat(64)),
                "TELEGRAM_OWNER_ID" => Ok("1001".into()),
                "TELEGRAM_BOT_TOKEN" => Ok("123456:TEST_token_abcdefghijklmnop".into()),
                _ => Err(std::env::VarError::NotPresent),
            }
        };
        let error = Settings::from_environment(lookup).err().unwrap();
        assert_eq!(error.to_string(), expected);
        assert!(!format!("{error:?}").contains("private-fixture"));
    }
    let error = Settings::from_environment(|_| Err(std::env::VarError::NotPresent))
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "AgentMail startup: HEIMDALL_AGENTMAIL_WEBHOOK_SECRET is required and must be Unicode"
    );
}

#[tokio::test]
async fn matching_rules_share_one_sop_admission_and_suppress_replay() {
    multi_sop_event_case(false).await;
}

#[tokio::test]
async fn later_sop_failure_retains_earlier_admission_and_suppresses_replay() {
    multi_sop_event_case(true).await;
}

async fn multi_sop_event_case(fail_second: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    state(&path, |s| {
        s["enabled"] = json!({"first":true,"second":true});
        Ok(())
    })
    .unwrap();
    let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway = format!("http://{}", backend.local_addr().unwrap());
    let (requests_tx, mut requests_rx) = tokio::sync::mpsc::unbounded_channel();
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = tokio::select! {
                _ = &mut stop_rx => break,
                accepted = backend.accept() => accepted.unwrap(),
            };
            let requests_tx = requests_tx.clone();
            hyper::server::conn::http1::Builder::new().serve_connection(
                TokioIo::new(socket),
                service_fn(move |req: Request<Incoming>| {
                    let requests_tx = requests_tx.clone();
                    async move {
                        let path = req.uri().path().to_string();
                        let authorization = req.headers().get("authorization").cloned();
                        let idempotency = req.headers().get("x-idempotency-key").cloned();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        let payload: Value = serde_json::from_slice(&body).unwrap();
                        requests_tx.send((path.clone(), authorization, idempotency, payload)).unwrap();
                        let failed = fail_second && path == "/sop/webhook/z-second";
                        let body = if failed {
                            json!({"error":"PRIVATE_ERROR"})
                        } else {
                            json!({"status":"accepted","source":"webhook","path":path,"results":[{"status":"coalesced","sop":"a-first","run_id":"retained-run"}]})
                        };
                        Ok::<_, Infallible>(Response::builder()
                            .status(if failed { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK })
                            .header("connection", "close")
                            .body(Full::new(Bytes::from(body.to_string()))).unwrap())
                    }
                }),
            ).await.unwrap();
        }
    });
    let key = test_signing_key();
    // Reverse SOP order proves dispatch order is sorted, not rule order.
    let filters = parse_filters(&json!([
        {"name":"first","sender":"notifications@github.com","contains":"review","sop":if fail_second {"z-second"} else {"a-first"}},
        {"name":"second","sender":"notifications@github.com","contains":"review","sop":"a-first"}
    ]).to_string()).unwrap();
    let (url, front) = ingress(Settings {
        secret: key.to_vec(),
        token: "fixture_not_a_live_token".into(),
        filters,
        state: path.clone(),
        paired_tokens: "[]".into(),
        authorization: None,
        gateway,
        telegram: None,
        gateway_secret: String::new(),
    })
    .await;
    let body = serde_json::to_vec(&json!({"event_type":"message.received","event_id":"evt_multi","message":{"from":"notifications@github.com","text":"PRIVATE_BODY review"}})).unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .post(&url)
            .headers(signed(&key, &body, now()))
            .body(body.clone())
            .send()
            .await
            .unwrap()
            .status(),
        202
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if state(&path, |s| {
                Ok(s["events"]["evt_multi"]["status"] != "reserved")
            })
            .unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    state(&path, |s| {
        let event = &s["events"]["evt_multi"];
        assert_eq!(event["status"], if fail_second {"uncertain"} else {"accepted"});
        assert_eq!(event["runs"], json!([{"sop":"a-first","run_id":"retained-run","status":"coalesced"}]));
        if fail_second {
            assert_eq!(event["diagnostic"], json!({"stage":"gateway","phase":"response_status","kind":"http","http_status":503,"gateway_completed":false}));
        }
        Ok(())
    }).unwrap();
    assert_eq!(
        client
            .post(&url)
            .headers(signed(&key, &body, now()))
            .body(body)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    let mut paths = Vec::new();
    while let Some((path, auth, idempotency, payload)) = requests_rx.recv().await {
        paths.push(path);
        assert_eq!(auth.unwrap(), "Bearer fixture_not_a_live_token");
        assert_eq!(idempotency.unwrap(), "evt_multi");
        assert_eq!(payload["rules"], json!(["first", "second"]));
        assert!(!payload.to_string().contains("PRIVATE_BODY"));
    }
    assert_eq!(
        paths,
        if fail_second {
            vec!["/sop/webhook/a-first", "/sop/webhook/z-second"]
        } else {
            vec!["/sop/webhook/a-first"]
        }
    );
    let saved = fs::read_to_string(&path).unwrap();
    for private in ["PRIVATE_BODY", "PRIVATE_ERROR", "fixture_not_a_live_token"] {
        assert!(!saved.contains(private));
    }
    front.abort();
}

#[test]
fn oversized_state_update_preserves_readable_ledger_and_reservations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state");
    state(&path, |data| {
        data["events"]["fixture"] = json!({"time":1,"status":"reserved"});
        Ok(())
    })
    .unwrap();
    let before = fs::read(&path).unwrap();
    let result = state(&path, |data| {
        data["events"]["fixture"]["runs"] =
            json!([{"sop":"fixture","run_id":"x".repeat(2_000_000)}]);
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    state(&path, |data| {
        assert_eq!(data["events"]["fixture"]["status"], "reserved");
        assert!(data["events"]["fixture"].get("runs").is_none());
        Ok(())
    })
    .unwrap();
}
