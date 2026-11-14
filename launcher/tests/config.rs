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
        80
    );
    assert_eq!(
        cfg["risk_profiles"]["primary"]["allowed_tools"][0].as_str(),
        Some("shell")
    );
    assert_eq!(
        cfg["providers"]["models"]["openai"]["codex"]["requires_openai_auth"].as_bool(),
        Some(true)
    );
    assert!(!text.contains("api_key"));
    assert!(!text.contains("fallback_providers"));
    assert!(!text.contains("Bearer"));
    assert!(!text.contains("bot_token"));
    assert!(cfg.get("reliability").is_none());
    for (vendor, aliases) in cfg["providers"]["models"].as_table().unwrap() {
        for (alias, entry) in aliases.as_table().unwrap() {
            assert!(
                entry.get("fallback").is_none(),
                "cross-provider fallback must not be configured on {vendor}.{alias}"
            );
            if let Some(models) = entry.get("fallback_models") {
                assert_eq!((vendor.as_str(), alias.as_str()), ("openrouter", "default"));
                for model in models.as_array().unwrap() {
                    let id = model.as_str().unwrap();
                    assert!(
                        id == "openrouter/free" || id.ends_with(":free"),
                        "fallback_models must remain free-tier only: {id}"
                    );
                }
            }
        }
    }
    assert!(render_config(&[], "gpt-6.1-sol").is_err());
    assert!(render_config(&ids, "").is_err());
}
#[test]
fn secret_overrides_are_in_memory_and_errors_do_not_echo_values() {
    let token = "123456:TEST_token_abcdefghijklmnop";
    let vars = secret_overrides(token).unwrap();
    assert_eq!(vars[0].0, "ZEROCLAW_channels__telegram__home__bot_token");
    assert_eq!(vars.len(), 1);
    let text = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    assert!(!text.contains(token));
    assert!(
        !secret_overrides("bad-secret-value")
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
    let mut vars = secret_overrides(token).unwrap();
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
        (
            "providers.models.opencode.default.model",
            "deepseek-v4.1-flash",
        ),
        (
            "providers.models.openrouter.default.model",
            "nvidia/nemotron-3-ultra-550b-a55b:free",
        ),
        (
            "providers.models.openrouter.default.context-window",
            "200000",
        ),
        ("providers.models.gemini.default.model", "gemini-3.5-flash"),
        ("agents.primary.model-provider", "openrouter.default"),
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
        ("storage.sqlite.default.open-timeout-secs", "30"),
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
        Some("openrouter.default")
    );
    assert_eq!(
        cfg["providers"]["models"]["opencode"]["default"]["model"].as_str(),
        Some("deepseek-v4.1-flash")
    );
    assert!(
        cfg["providers"]["models"]["opencode"]["default"]
            .get("context_window")
            .is_none()
    );
    assert_eq!(
        cfg["providers"]["models"]["openrouter"]["default"]["model"].as_str(),
        Some("nvidia/nemotron-3-ultra-550b-a55b:free")
    );
    assert_eq!(
        cfg["providers"]["models"]["openrouter"]["default"]["context_window"].as_integer(),
        Some(200_000)
    );
    let openrouter_fallbacks: Vec<&str> =
        cfg["providers"]["models"]["openrouter"]["default"]["fallback_models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
    assert_eq!(
        openrouter_fallbacks,
        vec![
            "cohere/north-mini-code:free",
            "inclusionai/ling-3.2-flash:free",
            "google/gemma-4-31b-it:free",
            "openrouter/free",
        ]
    );
    assert_eq!(
        cfg["providers"]["models"]["gemini"]["default"]["model"].as_str(),
        Some("gemini-3.5-flash")
    );
    assert!(
        cfg["providers"]["models"]["gemini"]["default"]
            .get("auth_mode")
            .is_none()
    );
    assert!(
        cfg["providers"]["models"]["gemini"]["default"]
            .get("context_window")
            .is_none()
    );
    let routes = cfg["model_routes"].as_array().unwrap();
    assert!(routes.iter().any(|r| r["hint"].as_str() == Some("opencode")
        && r["model_provider"].as_str() == Some("opencode.default")
        && r["model"].as_str() == Some("deepseek-v4.1-flash")));
    assert!(
        routes
            .iter()
            .any(|r| r["hint"].as_str() == Some("opencode-free")
                && r["model_provider"].as_str() == Some("opencode.default")
                && r["model"].as_str() == Some("deepseek-v4.1-flash-free"))
    );
    assert!(
        routes
            .iter()
            .any(|r| r["hint"].as_str() == Some("openrouter")
                && r["model_provider"].as_str() == Some("openrouter.default")
                && r["model"].as_str() == Some("nvidia/nemotron-3-ultra-550b-a55b:free"))
    );
    assert!(
        routes
            .iter()
            .any(|r| r["hint"].as_str() == Some("openrouter-free")
                && r["model_provider"].as_str() == Some("openrouter.default")
                && r["model"].as_str() == Some("nvidia/nemotron-3-ultra-550b-a55b:free"))
    );
    assert!(routes.iter().any(|r| r["hint"].as_str() == Some("codex")
        && r["model_provider"].as_str() == Some("openai.codex")));
    assert!(routes.iter().any(|r| r["hint"].as_str() == Some("gemini")
        && r["model_provider"].as_str() == Some("gemini.default")));
}

#[test]
fn codex_route_tracks_the_configured_model() {
    let cfg = render_config(&["1001".into()], "custom-codex-model").unwrap();
    let cfg: toml::Value = toml::from_str(&cfg).unwrap();
    let routes = cfg["model_routes"].as_array().unwrap();
    let opencode = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("opencode"))
        .unwrap();
    assert_eq!(opencode["model"].as_str(), Some("deepseek-v4.1-flash"));
    let opencode_free = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("opencode-free"))
        .unwrap();
    assert_eq!(
        opencode_free["model"].as_str(),
        Some("deepseek-v4.1-flash-free")
    );
    let codex = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("codex"))
        .unwrap();
    assert_eq!(codex["model"].as_str(), Some("custom-codex-model"));
    let openrouter = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("openrouter"))
        .unwrap();
    assert_eq!(
        openrouter["model"].as_str(),
        Some("nvidia/nemotron-3-ultra-550b-a55b:free")
    );
    let openrouter_free = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("openrouter-free"))
        .unwrap();
    assert_eq!(
        openrouter_free["model"].as_str(),
        Some("nvidia/nemotron-3-ultra-550b-a55b:free")
    );
    let gemini = routes
        .iter()
        .find(|route| route["hint"].as_str() == Some("gemini"))
        .unwrap();
    assert_eq!(gemini["model"].as_str(), Some("gemini-3.5-flash"));
}

#[test]
fn schema_four_openrouter_default_is_independent_of_codex_model_overrides() {
    for model in ["gpt-6.1-sol", "custom-codex-model"] {
        let rendered = render_config(&["1001".into()], model).unwrap();
        let cfg: toml::Value = toml::from_str(&rendered).unwrap();
        assert_eq!(cfg["schema_version"].as_integer(), Some(4));
        let provider_name = cfg["agents"]["primary"]["model_provider"].as_str().unwrap();
        assert_eq!(provider_name, "openrouter.default");
        let (provider, alias) = provider_name.split_once('.').unwrap();
        let active = &cfg["providers"]["models"][provider][alias];
        assert_eq!(
            active["model"].as_str(),
            Some("nvidia/nemotron-3-ultra-550b-a55b:free")
        );
        assert_eq!(active["context_window"].as_integer(), Some(200_000));
        assert_eq!(active["wire_api"].as_str(), Some("chat_completions"));
        // Codex capacity (475k) and subscription authentication must not leak to OpenRouter.
        for key in ["requires_openai_auth", "api_key"] {
            assert!(active.get(key).is_none(), "unexpected OpenRouter {key}");
        }
        let routes = cfg["model_routes"].as_array().unwrap();
        assert_eq!(routes.len(), 6);
        for hint in [
            "opencode",
            "opencode-free",
            "codex",
            "openrouter",
            "openrouter-free",
            "gemini",
        ] {
            assert_eq!(
                routes
                    .iter()
                    .filter(|route| route["hint"].as_str() == Some(hint))
                    .count(),
                1,
                "route must remain unique: {hint}"
            );
        }
        // Each retained route must still resolve to a configured provider alias.
        for route in routes {
            let (provider, alias) = route["model_provider"]
                .as_str()
                .unwrap()
                .split_once('.')
                .unwrap();
            assert!(cfg["providers"]["models"][provider][alias].is_table());
        }
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

#[test]
fn tavily_search_and_native_shell_are_explicitly_enabled() {
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
        "shell",
        "file_read",
        "file_write",
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
        for blocked in [
            "workspace__bash",
            "workspace__file_read",
            "workspace__file_write",
        ] {
            assert!(!actual.contains(&blocked), "unexpected host tool {blocked}");
        }
    }
    assert!(!config.contains("tavily_api_key"));
    assert!(
        cfg["risk_profiles"]["primary"]["allowed_commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("*"))
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
    assert_eq!(cfg["skills"]["open_skills_enabled"].as_bool(), Some(false));
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

#[test]
fn email_management_grants_are_explicit_and_bundled() {
    let cfg: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    let profile = &cfg["risk_profiles"]["primary"];
    for key in ["allowed_tools", "auto_approve"] {
        let names = tool_names(profile, key);
        assert!(names.contains(&"mail__filters"));
        assert!(names.contains(&"mail__set_enabled"));
    }
    assert_eq!(
        cfg["mcp_bundles"]["mail"]["servers"][0].as_str(),
        Some("mail")
    );
    let servers = cfg["mcp"]["servers"].as_array().unwrap();
    let server = servers
        .iter()
        .find(|s| s["name"].as_str() == Some("mail"))
        .unwrap();
    assert_eq!(server["args"][0].as_str(), Some("--mail-mcp"));
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
        assert!(!excluded.contains(&tool));
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
    assert_eq!(cfg["gateway"]["allow_remote_admin"].as_bool(), Some(true));
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
        cfg["runtime_profiles"]["primary"]["max_context_tokens"].as_integer(),
        Some(192_000)
    );
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
    assert_eq!(cfg["browser"]["automation_enabled"].as_bool(), Some(true));
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

#[test]
fn codex_capacity_is_not_borrowed_by_another_model() {
    let configured: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    assert_eq!(
        configured["providers"]["models"]["openai"]["codex"]["context_window"].as_integer(),
        Some(475_000)
    );
    let alternate: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "fixture-other-model").unwrap()).unwrap();
    assert_eq!(
        alternate["providers"]["models"]["openai"]["codex"]["model"].as_str(),
        Some("fixture-other-model")
    );
    assert!(
        alternate["providers"]["models"]["openai"]["codex"]
            .get("context_window")
            .is_none()
    );
}

#[test]
fn launcher_email_rules_never_enter_upstream_runtime_configuration() {
    let template: toml::Value =
        toml::from_str(include_str!("../../deploy/bot/config.template.toml")).unwrap();
    assert_eq!(
        template["heimdall"]["agentmail"]["filters"][0]["sop"].as_str(),
        Some("github-pr-review")
    );
    let rendered = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    let config: toml::Value = toml::from_str(&rendered).unwrap();
    assert!(config.get("heimdall").is_none());
    assert!(!rendered.contains("notifications@github.com"));
}

#[test]
fn subscription_model_selection_comes_from_the_template() {
    assert_eq!(
        heimdall_launcher::config::configured_model().unwrap(),
        "gpt-6.1-sol"
    );
}

#[test]
fn native_execution_is_networked_root_capable_without_executor_configuration() {
    let text = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    let config: toml::Value = toml::from_str(&text).unwrap();
    let profile = &config["risk_profiles"]["primary"];
    assert_eq!(profile["level"].as_str(), Some("full"));
    assert_eq!(profile["workspace_only"].as_bool(), Some(false));
    assert_eq!(profile["block_high_risk_commands"].as_bool(), Some(false));
    assert_eq!(
        profile["require_approval_for_medium_risk"].as_bool(),
        Some(false)
    );
    assert_eq!(profile["allowed_commands"][0].as_str(), Some("*"));
    assert_eq!(profile["shell_timeout_secs"].as_integer(), Some(300));
    assert_eq!(profile["delegation_policy"]["mode"].as_str(), Some("allow"));
    assert!(profile["forbidden_paths"].as_array().unwrap().is_empty());
    assert!(!text.contains("executor"));
    assert!(!text.contains("workspace__"));
    for field in ["allowed_tools", "auto_approve"] {
        let names = tool_names(profile, field);
        for name in [
            "shell",
            "git_operations",
            "browser",
            "spawn_subagent",
            "calculator",
            "backup",
            "data_management",
            "TodoWrite",
        ] {
            assert!(names.contains(&name), "missing {field}: {name}");
        }
    }
    assert_eq!(config["backup"]["max_keep"].as_integer(), Some(2));
    assert_eq!(config["data_retention"]["dry_run"].as_bool(), Some(true));
}

#[test]
fn sqlite_storage_is_explicit_and_unavailable_bulk_sync_is_disabled() {
    let rendered = render_config(&["1001".into()], "gpt-6.1-sol").unwrap();
    let config: toml::Value = toml::from_str(&rendered).unwrap();
    assert_eq!(
        config["storage"]["sqlite"]["default"]["open_timeout_secs"].as_integer(),
        Some(30)
    );
    assert_eq!(config["memory"]["backend"].as_str(), Some("sqlite"));
    assert_eq!(
        config["skills"]["open_skills_enabled"].as_bool(),
        Some(false)
    );
}

#[test]
fn runtime_upgrade_preserves_schema_and_openrouter_default_after_rendering() {
    let template: toml::Value =
        toml::from_str(include_str!("../../deploy/bot/config.template.toml")).unwrap();
    let rendered: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    for config in [&template, &rendered] {
        assert_eq!(config["schema_version"].as_integer(), Some(4));
        assert_eq!(
            config["agents"]["primary"]["model_provider"].as_str(),
            Some("openrouter.default")
        );
        let openrouter = &config["providers"]["models"]["openrouter"]["default"];
        assert_eq!(
            openrouter["model"].as_str(),
            Some("nvidia/nemotron-3-ultra-550b-a55b:free")
        );
        assert_eq!(openrouter["context_window"].as_integer(), Some(200_000));
        let fallbacks: Vec<&str> = openrouter["fallback_models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(
            fallbacks,
            vec![
                "cohere/north-mini-code:free",
                "inclusionai/ling-3.2-flash:free",
                "google/gemma-4-31b-it:free",
                "openrouter/free",
            ]
        );
        // The default needs only the operator's environment override for its key.
        assert!(openrouter.get("api_key").is_none());
    }
}

#[test]
fn openrouter_default_retains_all_explicit_model_routes() {
    let config: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    let routes = config["model_routes"].as_array().unwrap();
    assert_eq!(routes.len(), 6);
    for (hint, provider, model) in [
        ("opencode", "opencode.default", "deepseek-v4.1-flash"),
        (
            "opencode-free",
            "opencode.default",
            "deepseek-v4.1-flash-free",
        ),
        ("codex", "openai.codex", "gpt-6.1-sol"),
        (
            "openrouter",
            "openrouter.default",
            "nvidia/nemotron-3-ultra-550b-a55b:free",
        ),
        (
            "openrouter-free",
            "openrouter.default",
            "nvidia/nemotron-3-ultra-550b-a55b:free",
        ),
        ("gemini", "gemini.default", "gemini-3.5-flash"),
    ] {
        let matching: Vec<_> = routes
            .iter()
            .filter(|route| route["hint"].as_str() == Some(hint))
            .collect();
        assert_eq!(matching.len(), 1, "route {hint} must be unique");
        assert_eq!(matching[0]["model_provider"].as_str(), Some(provider));
        assert_eq!(matching[0]["model"].as_str(), Some(model));
        let (vendor, alias) = provider.split_once('.').unwrap();
        assert!(config["providers"]["models"][vendor][alias].is_table());
    }
}

#[test]
fn codex_override_cannot_replace_openrouter_default_or_other_provider_routes() {
    let original: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "gpt-6.1-sol").unwrap()).unwrap();
    let alternate: toml::Value =
        toml::from_str(&render_config(&["1001".into()], "fixture-other-model").unwrap()).unwrap();
    assert_eq!(
        alternate["agents"]["primary"]["model_provider"].as_str(),
        Some("openrouter.default")
    );
    for vendor in ["openrouter", "opencode", "gemini"] {
        assert_eq!(
            alternate["providers"]["models"][vendor], original["providers"]["models"][vendor],
            "Codex override changed {vendor}"
        );
    }
    let original_routes = original["model_routes"].as_array().unwrap();
    let alternate_routes = alternate["model_routes"].as_array().unwrap();
    assert_eq!(alternate_routes.len(), original_routes.len());
    for (before, after) in original_routes.iter().zip(alternate_routes) {
        if before["hint"].as_str() == Some("codex") {
            assert_eq!(after["model"].as_str(), Some("fixture-other-model"));
            assert_eq!(after["model_provider"].as_str(), Some("openai.codex"));
        } else {
            assert_eq!(before, after);
        }
    }
}
