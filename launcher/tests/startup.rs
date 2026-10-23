use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn recovery_starts_without_credentials_and_only_exposes_labeled_health() {
    #[cfg(target_os = "linux")]
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "Linux startup tests require root builder"
    );
    let reservation = TcpListener::bind("[::1]:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_heimdall-launcher"))
            .env_clear()
            .env("HEIMDALL_RECOVERY", "1")
            .env("PORT", port.to_string())
            // Invalid secrets must not be read or validated in recovery mode.
            .env("GITHUB_TOKEN", "synthetic-invalid-secret")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    for (path, status, body) in [
        ("/health", "200 OK", "recovery\n"),
        ("/", "503 Service Unavailable", "bot offline: recovery\n"),
    ] {
        let mut stream = loop {
            if let Ok(stream) = TcpStream::connect(("::1", port)) {
                break stream;
            }
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "recovery exited before health"
            );
            assert!(Instant::now() < deadline, "recovery failed to listen");
            std::thread::sleep(Duration::from_millis(10));
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with(&format!("HTTP/1.1 {status}")));
        assert!(reply.ends_with(body));
        assert!(!reply.contains("synthetic-invalid-secret"));
    }
}

#[test]
fn nonstandard_executor_override_fails_before_agentmail_startup() {
    let output = Command::new(env!("CARGO_BIN_EXE_heimdall-launcher"))
        .env_clear()
        .env("TELEGRAM_OWNER_ID", "1001")
        .env("TELEGRAM_BOT_TOKEN", "123456:TEST_token_abcdefghijklmnop")
        .env("HEIMDALL_EXECUTOR_TOKEN", "a".repeat(64))
        .env("HEIMDALL_EXECUTOR_URL", "https://private-fixture.invalid")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("HEIMDALL_EXECUTOR_URL must use the configured private executor"));
    assert!(!error.contains("private-fixture"));
}
