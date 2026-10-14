use heimdall_launcher::config::{parse_allowed_ids, render_config, secret_overrides};
#[test]
fn owner_and_optional_comma_list_are_strict() {
    assert_eq!(parse_allowed_ids("1001", None).unwrap(), ["1001"]);
    assert_eq!(
        parse_allowed_ids("1001", Some(" 123,456,123,1001 ")).unwrap(),
        ["1001", "123", "456"]
    );
    assert_eq!(parse_allowed_ids("1001", Some(" ")).unwrap(), ["1001"]);
    for bad in ["0", "-1", "1,", "1,,2", "*", "1\n2", "01"] {
        assert!(parse_allowed_ids("1001", Some(bad)).is_err(), "{bad}");
    }
    for bad in ["", "0", "-1", "*", "1,2"] {
        assert!(parse_allowed_ids(bad, None).is_err());
    }
}
#[test]
fn config_grants_only_remote_tools_without_secrets_or_fallback() {
    let ids = parse_allowed_ids("1001", Some("123")).unwrap();
    let text = render_config(&ids, "gpt-6.1-sol").unwrap();
    let cfg: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(cfg["gateway"]["host"].as_str(), Some("0.0.0.0"));
    assert_eq!(
        cfg["peer_groups"]["telegram_home"]["external_peers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        cfg["risk_profiles"]["primary"]["allowed_tools"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        cfg["risk_profiles"]["primary"]["allowed_tools"][0].as_str(),
        Some("workspace__bash")
    );
    assert_eq!(
        cfg["providers"]["models"]["openai"]["codex"]["requires_openai_auth"].as_bool(),
        Some(true)
    );
    assert!(!text.contains("api_key"));
    assert!(!text.contains("fallback"));
    assert!(!text.contains("Bearer"));
    assert!(!text.contains("bot_token"));
    assert!(render_config(&[], "gpt-6.1-sol").is_err());
    assert!(render_config(&ids, "").is_err());
}
#[test]
fn secret_overrides_are_in_memory_and_errors_do_not_echo_values() {
    let token = "123456:TEST_token_abcdefghijklmnop";
    let executor = "e".repeat(64);
    let vars = secret_overrides(token, &executor).unwrap();
    assert_eq!(vars[0].0, "ZEROCLAW_channels__telegram__home__bot_token");
    assert_eq!(
        vars[1].0,
        "ZEROCLAW_mcp__servers__workspace__headers__authorization"
    );
    let text = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    assert!(!text.contains(token));
    assert!(!text.contains(&executor));
    assert!(
        !secret_overrides("bad-secret-value", &executor)
            .unwrap_err()
            .contains("bad-secret-value")
    );
}

#[test]
fn pinned_runtime_accepts_effective_policy_and_secret_overrides() {
    let Ok(binary) = std::env::var("HEIMDALL_TEST_RUNTIME") else {
        eprintln!("native runtime smoke check requires HEIMDALL_TEST_RUNTIME; portable tests only");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let config = render_config(&["1001".into(), "1002".into()], "gpt-6.1-sol").unwrap();
    std::fs::write(dir.path().join("config.toml"), &config).unwrap();
    let token = "123456:TEST_token_abcdefghijklmnop";
    let secret = "e".repeat(64);
    let vars = secret_overrides(token, &secret).unwrap();
    for (property, expected) in [
        (
            "risk_profiles.primary.allowed-tools",
            "[\"workspace__bash\", \"workspace__file_read\", \"workspace__file_write\"]",
        ),
        (
            "peer_groups.telegram_home.external-peers",
            "[\"1001\", \"1002\"]",
        ),
        ("providers.models.openai.codex.requires-openai-auth", "true"),
    ] {
        let result = std::process::Command::new(&binary)
            .args(["config", "get", property])
            .env("ZEROCLAW_CONFIG_DIR", dir.path())
            .env("ZEROCLAW_DATA_DIR", dir.path())
            .envs(vars.iter().cloned())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), expected);
    }
    let result = std::process::Command::new(&binary)
        .args(["config", "migrate"])
        .env("ZEROCLAW_CONFIG_DIR", dir.path())
        .env("ZEROCLAW_DATA_DIR", dir.path())
        .envs(vars)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let saved = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    assert!(!saved.contains(token));
    assert!(!saved.contains(&secret));
}

#[test]
fn model_choices_preserve_codex_default_and_share_tool_boundary() {
    let cfg: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    assert_eq!(
        cfg["agents"]["primary"]["model_provider"].as_str(),
        Some("openai.codex")
    );
    assert_eq!(
        cfg["providers"]["models"]["openrouter"]["free"]["model"].as_str(),
        Some("openrouter/free")
    );
    let routes = cfg["model_routes"].as_array().unwrap();
    assert!(routes.iter().any(|r| r["hint"].as_str() == Some("free")
        && r["model_provider"].as_str() == Some("openrouter.free")));
    assert!(routes.iter().any(|r| r["hint"].as_str() == Some("codex")
        && r["model_provider"].as_str() == Some("openai.codex")));
}

#[test]
fn optional_openrouter_key_is_private_and_validated() {
    use heimdall_launcher::config::openrouter_override;
    assert_eq!(openrouter_override(None).unwrap(), None);
    assert_eq!(openrouter_override(Some(" ")).unwrap(), None);
    let key = concat!("sk-or-v1-", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let (name, value) = openrouter_override(Some(key)).unwrap().unwrap();
    assert_eq!(
        name,
        "ZEROCLAW_providers__models__openrouter__free__api_key"
    );
    assert_eq!(value, key);
    let cfg = render_config(&["1001".into()], "custom-codex-model").unwrap();
    assert!(!cfg.contains(key));
    let cfg: toml::Value = toml::from_str(&cfg).unwrap();
    assert_eq!(
        cfg["model_routes"][1]["model"].as_str(),
        Some("custom-codex-model")
    );
    for bad in ["secret-invalid-value", "sk-or-v1-invalid\nsecret"] {
        let error = openrouter_override(Some(bad)).unwrap_err();
        assert!(!error.contains(bad));
    }
}

#[test]
fn owner_must_be_configured_explicitly() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_heimdall-launcher"))
        .arg("--render-config")
        .env_remove("TELEGRAM_OWNER_ID")
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "A deployment must not silently select a default owner"
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("TELEGRAM_OWNER_ID is required"));
}
