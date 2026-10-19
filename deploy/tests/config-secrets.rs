#[tokio::test(flavor = "current_thread")]
async fn heimdall_secret_overrides_reach_native_config_without_persistence() {
    let temp = tempfile::tempdir().unwrap();
    let template = std::fs::read_to_string(std::env::var("HEIMDALL_TEST_CONFIG").unwrap()).unwrap();
    std::fs::write(temp.path().join("config.toml"),template).unwrap();
    let token = "123456:TEST_token_abcdefghijklmnop";
    let openrouter_key = concat!("sk-or-v1-", "placeholder_secret_abcdef");
    let tavily_key = concat!("tvly-test-", "placeholder_secret_abcdef");
    let deepgram_key = "fixture_deepgram_private_value";
    let cohere_key = "fixture_cohere_private_value";
    let authorization = format!("Bearer {}","e".repeat(64));
    // This integration test has one current-thread case, in a separate test process.
    unsafe {
        std::env::set_var("ZEROCLAW_CONFIG_DIR",temp.path());
        std::env::set_var("ZEROCLAW_providers__transcription__deepgram__voice__api_key",deepgram_key);
        std::env::set_var("ZEROCLAW_memory__embedding_api_key",cohere_key);
        std::env::set_var("ZEROCLAW_web_search__tavily_api_key",tavily_key);
        std::env::set_var("ZEROCLAW_providers__models__anthropic__custom__model","placeholder-model");
        std::env::set_var("ZEROCLAW_providers__models__anthropic__custom__api_key","sk-ant-fixture-private");
        std::env::set_var("ZEROCLAW_providers__models__openrouter__free__api_key",openrouter_key);
        std::env::set_var("ZEROCLAW_channels__telegram__home__bot_token",token);
        std::env::set_var("ZEROCLAW_mcp__servers__workspace__headers__authorization",&authorization);
        std::env::set_var("ZEROCLAW_gateway__paired_tokens",serde_json::json!([zeroclaw_config::pairing::PairingGuard::token_hash("fixture_adapter_bearer_01234567890123456789")]).to_string());
    }
    let config = zeroclaw_config::schema::Config::load_or_init().await.unwrap();
    assert_eq!(config.gateway.host,"0.0.0.0");
    let pairing = zeroclaw_config::pairing::PairingGuard::new(true, &config.gateway.paired_tokens, Default::default());
    assert!(pairing.token_is_paired("fixture_adapter_bearer_01234567890123456789"));
    assert!(!pairing.token_is_paired("wrong_fixture_token"));
    assert!(zeroclaw_infra::parse_gateway_bind_socket_addr(&config.gateway.host,config.gateway.port).is_ok());
    assert_eq!(config.data_dir,temp.path().join("data"));
    assert_eq!(config.channels.telegram["home"].bot_token,token);
    assert_eq!(config.mcp.servers[0].headers.get("authorization"),Some(&authorization));
    assert_eq!(config.mcp_servers_for_agent("primary").len(),5);
    assert!(config.mcp_servers_for_agent("unknown").is_empty());
    assert_eq!(config.providers.models.find("openrouter","free").unwrap().api_key.as_deref(),Some(openrouter_key));
    assert_eq!(config.model_routes.iter().find(|r| r.hint == "free").unwrap().model_provider,"openrouter.free");
    let custom = config.providers.models.find("anthropic","custom").unwrap();
    assert_eq!(custom.model.as_deref(),Some("placeholder-model"));
    assert_eq!(custom.api_key.as_deref(),Some("sk-ant-fixture-private"));
    assert!(config.web_search.enabled);
    assert_eq!(config.web_search.search_provider,"tavily");
    assert_eq!(config.web_search.max_results,3);
    assert_eq!(config.web_search.timeout_secs,15);
    assert_eq!(config.web_search.tavily_api_key.as_deref(),Some(tavily_key));
    assert!(config.transcription.enabled);
    assert_eq!(config.agents["primary"].transcription_provider.as_str(), "deepgram.voice");
    let voice = &config.providers.transcription.deepgram["voice"];
    assert_eq!(voice.base.api_key.as_deref(), Some(deepgram_key));
    assert_eq!(voice.model.as_deref(), Some("nova-3"));
    assert_eq!(config.memory.embedding_api_key.as_deref(), Some(cohere_key));
    assert_eq!(config.memory.embedding_provider, "custom:https://api.cohere.ai/compatibility/v1");
    assert_eq!(config.memory.embedding_model, "embed-v5.0-fast");
    assert_eq!(config.memory.embedding_dimensions, 2048);
    config.save().await.unwrap();
    let saved = std::fs::read_to_string(temp.path().join("config.toml")).unwrap();
    assert!(!saved.contains("sk-ant-fixture-private"));
    assert!(!saved.contains(openrouter_key));
    assert!(!saved.contains(tavily_key));
    assert!(!saved.contains(deepgram_key));
    assert!(!saved.contains(cohere_key));
    assert!(!saved.contains(token));
    assert!(!saved.contains(&authorization));
    assert!(!saved.contains("fixture_adapter_bearer_01234567890123456789"));
}
