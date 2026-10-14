#![cfg(target_os = "linux")]
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    process::Command,
};
#[test]
fn bootstrap_does_not_follow_state_symlinks_and_keeps_secrets_out_of_config() {
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "Linux bootstrap tests require root builder"
    );
    fs::create_dir_all("/data").unwrap();
    fs::create_dir_all("/run/launcher-protected").unwrap();
    std::os::unix::fs::symlink("/run/launcher-protected", "/data/zeroclaw").unwrap();
    let launch = || {
        Command::new(env!("CARGO_BIN_EXE_heimdall-launcher"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TELEGRAM_OWNER_ID", "1001")
            .env("TELEGRAM_BOT_TOKEN", "123456:TEST_token_abcdefghijklmnop")
            .env("HEIMDALL_EXECUTOR_TOKEN", "e".repeat(64))
            .output()
            .unwrap()
    };
    let blocked = launch();
    assert!(!blocked.status.success());
    assert_eq!(fs::metadata("/run/launcher-protected").unwrap().uid(), 0);
    fs::remove_file("/data/zeroclaw").unwrap();
    let missing_auth = launch();
    assert!(!missing_auth.status.success());
    assert!(String::from_utf8_lossy(&missing_auth.stderr).contains("ChatGPT sign-in required"));
    let cfg = fs::read_to_string("/data/zeroclaw/config.toml").unwrap();
    assert!(!cfg.contains("TEST_token"));
    assert!(!cfg.contains(&"e".repeat(64)));
    assert_eq!(fs::metadata("/data").unwrap().uid(), 0);
    assert_eq!(fs::metadata("/data/zeroclaw").unwrap().uid(), 1000);
    assert_eq!(
        fs::metadata("/data/zeroclaw/config.toml")
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // A file alone must never count as a usable subscription sign-in.
    fs::write("/data/zeroclaw/auth-profiles.json", "{}").unwrap();
    let invalid_auth = launch();
    assert!(!invalid_auth.status.success());
    assert!(
        String::from_utf8_lossy(&invalid_auth.stderr).contains("authentication validation failed"),
        "launcher failed before validating the fixture profile: {}",
        String::from_utf8_lossy(&invalid_auth.stderr)
    );
    fs::remove_dir_all("/data/zeroclaw").unwrap();
}
