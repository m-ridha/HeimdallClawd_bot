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
fn config_grants_workspace_and_search_tools_without_secrets_or_fallback() {
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
        56
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
    let mut vars = secret_overrides(token, &secret).unwrap();
    let deepgram_key = "fixture_deepgram_private_value";
    let cohere_key = "fixture_cohere_private_value";
    vars.push((
        "ZEROCLAW_providers__transcription__deepgram__voice__api_key".into(),
        deepgram_key.into(),
    ));
    vars.push((
        "ZEROCLAW_memory__embedding_api_key".into(),
        cohere_key.into(),
    ));
    for (property, expected) in [
        (
            "peer_groups.telegram_home.external-peers",
            "[\"1001\", \"1002\"]",
        ),
        ("providers.models.openai.codex.requires-openai-auth", "true"),
        ("heartbeat.enabled", "true"),
        ("heartbeat.to", "1001"),
        ("media_pipeline.enabled", "true"),
        ("transcription.enabled", "true"),
        ("providers.transcription.deepgram.voice.model", "nova-3"),
        ("agents.primary.transcription-provider", "deepgram.voice"),
        (
            "memory.embedding-provider",
            "custom:https://api.cohere.ai/compatibility/v1",
        ),
        ("memory.embedding-model", "embed-v5.0-fast"),
        ("memory.embedding-dimensions", "2048"),
        ("knowledge.enabled", "true"),
        ("memory.consolidation-extract-facts", "true"),
        ("memory.snapshot-enabled", "true"),
        ("memory.rerank-enabled", "true"),
        ("memory.rerank-strategy", "mmr"),
        ("memory.candidate-multiplier", "2"),
        ("agents.primary.acp-enable-mcp", "true"),
        ("observability.log-persistence", "rotating"),
        ("observability.log-tool-io", "off"),
        ("observability.log-llm-request-payload", "off"),
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
        assert_eq!(
            String::from_utf8_lossy(&result.stdout).trim(),
            expected,
            "effective runtime property: {property}"
        );
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
    assert!(!saved.contains(deepgram_key));
    assert!(!saved.contains(cohere_key));
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
fn codex_route_tracks_the_configured_model() {
    let cfg = render_config(&["1001".into()], "custom-codex-model").unwrap();
    let cfg: toml::Value = toml::from_str(&cfg).unwrap();
    let routes = cfg["model_routes"].as_array().unwrap();
    let codex = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("codex"))
        .unwrap();
    assert_eq!(codex["model"].as_str(), Some("custom-codex-model"));
    let free = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("free"))
        .unwrap();
    assert_eq!(free["model"].as_str(), Some("openrouter/free"));
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

#[test]
fn tavily_search_is_explicitly_enabled_without_broadening_host_tools() {
    let config = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    let cfg: toml::Value = toml::from_str(&config).unwrap();
    assert_eq!(cfg["web_search"]["enabled"].as_bool(), Some(true));
    assert_eq!(
        cfg["web_search"]["search_provider"].as_str(),
        Some("tavily")
    );
    assert_eq!(cfg["web_search"]["max_results"].as_integer(), Some(3));
    assert_eq!(cfg["web_search"]["timeout_secs"].as_integer(), Some(15));
    let expected = [
        "workspace__bash",
        "workspace__file_read",
        "workspace__file_write",
        "web_search_tool",
        "skills__install",
        "skills__list",
    ];
    for field in ["allowed_tools", "auto_approve"] {
        let actual: Vec<_> = cfg["risk_profiles"]["primary"][field]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        for name in expected {
            assert!(actual.contains(&name), "missing {name}");
        }
        for blocked in ["shell", "file_read", "file_write", "delegate", "schedule"] {
            assert!(!actual.contains(&blocked), "unexpected host tool {blocked}");
        }
    }
    assert!(!config.contains("tavily_api_key"));
    assert!(
        cfg["risk_profiles"]["primary"]["allowed_commands"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn requested_capabilities_are_enabled_with_owner_scoped_heartbeat() {
    let cfg: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    for section in [
        "scheduler",
        "pipeline",
        "hooks",
        "plugins",
        "web_fetch",
        "http_request",
        "text_browser",
        "link_enricher",
    ] {
        assert_eq!(cfg[section]["enabled"].as_bool(), Some(true), "{section}");
    }
    assert_eq!(cfg["skills"]["open_skills_enabled"].as_bool(), Some(true));
    assert_eq!(cfg["skills"]["allow_scripts"].as_bool(), Some(true));
    assert_eq!(
        cfg["runtime_profiles"]["primary"]["parallel_tools"].as_bool(),
        Some(true)
    );
    assert_eq!(cfg["heartbeat"]["enabled"].as_bool(), Some(true));
    assert_eq!(cfg["heartbeat"]["agent"].as_str(), Some("primary"));
    assert_eq!(cfg["heartbeat"]["target"].as_str(), Some("telegram"));
    assert_eq!(cfg["heartbeat"]["to"].as_str(), Some("1001"));
    assert_eq!(
        cfg["http_request"]["allow_private_hosts"].as_bool(),
        Some(false)
    );
    let tools = cfg["risk_profiles"]["primary"]["allowed_tools"]
        .as_array()
        .unwrap();
    for tool in [
        "cron_add",
        "cron_update",
        "execute_pipeline",
        "sop_execute",
        "web_fetch",
        "http_request",
        "text_browser",
        "poll",
        "reaction",
        "send_via",
    ] {
        assert!(tools.iter().any(|t| t.as_str() == Some(tool)), "{tool}");
    }
}

#[test]
fn agentmail_auth_is_an_environment_reference_not_a_persisted_key() {
    let text = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    let config: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(
        config["http_request"]["secrets"]["agentmail"].as_str(),
        Some("${AGENTMAIL_AUTHORIZATION}")
    );
}

fn tool_names<'a>(profile: &'a toml::Value, field: &str) -> Vec<&'a str> {
    profile[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect()
}

#[test]
fn codespace_tools_are_explicitly_granted_without_host_shell_access() {
    let cfg: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    let profile = &cfg["risk_profiles"]["primary"];
    let allowed = tool_names(profile, "allowed_tools");
    let approved = tool_names(profile, "auto_approve");
    for tool in [
        "codespace__status",
        "codespace__list",
        "codespace__create",
        "codespace__stop",
        "codespace__delete",
        "codespace__exec",
    ] {
        assert!(allowed.contains(&tool), "missing {tool}");
    }
    for tool in [
        "codespace__status",
        "codespace__list",
        "codespace__create",
        "codespace__stop",
        "codespace__delete",
        "codespace__exec",
    ] {
        assert!(approved.contains(&tool), "{tool} should be auto-approved");
    }
    assert_eq!(
        cfg["http_request"]["secrets"]["github"].as_str(),
        Some("${HEIMDALL_GITHUB_AUTHORIZATION}")
    );
    let excluded = tool_names(profile, "excluded_tools");
    for tool in [
        "shell",
        "file_read",
        "file_write",
        "git_operations",
        "schedule",
        "delegate",
    ] {
        assert!(excluded.contains(&tool));
    }
    assert_eq!(
        cfg["agents"]["primary"]["mcp_bundles"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn github_token_is_formatted_only_in_memory_and_never_rendered() {
    use heimdall_launcher::config::github_authorization;
    let token = "TEST_github_token_0123456789";
    assert_eq!(
        github_authorization(token).unwrap(),
        format!("Bearer {token}")
    );
    let rendered = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    assert!(!rendered.contains(token));
    assert!(!rendered.contains(&format!("Bearer {token}")));
    for invalid in [
        "",
        "short",
        "Bearer TEST_github_token_0123456789",
        "TEST_github_token_0123456789\n",
        "TEST_github_token_0123456789\r",
    ] {
        let error = github_authorization(invalid).unwrap_err();
        assert!(error.starts_with("Invalid GITHUB_TOKEN"));
        if !invalid.is_empty() {
            assert!(!error.contains(invalid));
        }
    }
}

#[test]
fn native_capabilities_have_explicit_grants_and_bounded_private_defaults() {
    let cfg: toml::Value =
        toml::from_str(&render_config(&["1001".into(), "1002".into()], "gpt-6.1-sol").unwrap())
            .unwrap();
    let profile = &cfg["risk_profiles"]["primary"];
    let allowed = tool_names(profile, "allowed_tools");
    let approved = tool_names(profile, "auto_approve");
    assert_eq!(allowed, approved);
    for name in [
        "tool_search",
        "image_info",
        "sessions_current",
        "sessions_list",
        "sessions_history",
        "sessions_send",
        "knowledge",
        "project_intel",
        "report_template",
        "cloud_ops",
        "cloud_patterns",
        "security_ops",
        "memory_store",
        "memory_recall",
        "memory_forget",
    ] {
        assert!(allowed.contains(&name), "{name}");
    }
    assert!(!allowed.contains(&"screenshot"));
    assert_eq!(cfg["heartbeat"]["to"].as_str(), Some("1001"));
    assert_eq!(
        cfg["heartbeat"]["task_timeout_secs"].as_integer(),
        Some(120)
    );
    assert_eq!(cfg["knowledge"]["max_nodes"].as_integer(), Some(10_000));
    assert_eq!(
        cfg["observability"]["log_persistence_retention_max_files"].as_integer(),
        Some(3)
    );
    assert_eq!(
        cfg["observability"]["log_persistence_retention_max_age_days"].as_integer(),
        Some(3)
    );
    assert_eq!(cfg["observability"]["log_tool_io"].as_str(), Some("off"));
    assert_eq!(
        cfg["observability"]["log_llm_request_payload"].as_str(),
        Some("off")
    );
    assert_eq!(cfg["gateway"]["require_pairing"].as_bool(), Some(true));
    assert_eq!(cfg["gateway"]["allow_remote_admin"].as_bool(), Some(false));
    for section in ["nodes", "webauthn"] {
        assert_eq!(cfg[section]["enabled"].as_bool(), Some(false));
    }
    assert_eq!(
        cfg["memory"]["embedding_provider"].as_str(),
        Some("custom:https://api.cohere.ai/compatibility/v1")
    );
}

#[test]
fn voice_and_embeddings_select_explicit_providers_without_persisted_credentials() {
    let text = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    let cfg: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(cfg["transcription"]["enabled"].as_bool(), Some(true));
    assert_eq!(
        cfg["agents"]["primary"]["transcription_provider"].as_str(),
        Some("deepgram.voice")
    );
    assert_eq!(
        cfg["providers"]["transcription"]["deepgram"]["voice"]["model"].as_str(),
        Some("nova-3")
    );
    assert_eq!(
        cfg["memory"]["embedding_model"].as_str(),
        Some("embed-v5.0-fast")
    );
    assert_eq!(
        cfg["memory"]["embedding_dimensions"].as_integer(),
        Some(2048)
    );
    assert_eq!(
        cfg["memory"]["embedding_cache_size"].as_integer(),
        Some(1024)
    );
    assert_eq!(cfg["memory"]["search_mode"].as_str(), Some("hybrid"));
    assert!(!text.contains("api_key"));
}

#[test]
fn managed_instructions_use_full_bootstrap_budget() {
    let config: toml::Value =
        toml::from_str(include_str!("../../deploy/bot/config.template.toml")).unwrap();
    assert_eq!(
        config["runtime_profiles"]["primary"]["compact_context"].as_bool(),
        Some(false)
    );
}

#[test]
fn primary_turn_budget_supports_multi_step_github_work() {
    let cfg: toml::Value =
        toml::from_str(include_str!("../../deploy/bot/config.template.toml")).unwrap();
    assert_eq!(
        cfg["runtime_profiles"]["primary"]["max_tool_iterations"].as_integer(),
        Some(48)
    );
    assert_eq!(
        cfg["runtime_profiles"]["primary"]["max_actions_per_hour"].as_integer(),
        Some(120)
    );
}

#[test]
fn upgraded_tool_inventory_stays_full_with_bounded_discovery() {
    let cfg: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    assert_eq!(
        cfg["tools"]["optional"].as_array().unwrap(),
        &[toml::Value::String("*".into())]
    );
    assert_eq!(
        cfg["runtime_profiles"]["primary"]["deferred_builtin_tools"].as_bool(),
        Some(true)
    );
    assert_eq!(cfg["mcp"]["deferred_loading"].as_bool(), Some(false));
    assert_eq!(cfg["browser"]["automation_enabled"].as_bool(), Some(false));
    for field in ["allowed_tools", "auto_approve"] {
        assert!(tool_names(&cfg["risk_profiles"]["primary"], field).contains(&"tool_search"));
    }
}

#[test]
fn memory_reranking_has_a_small_local_candidate_pool() {
    let cfg: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    assert_eq!(cfg["memory"]["rerank_enabled"].as_bool(), Some(true));
    assert_eq!(cfg["memory"]["rerank_strategy"].as_str(), Some("mmr"));
    assert_eq!(cfg["memory"]["candidate_multiplier"].as_integer(), Some(2));
}
