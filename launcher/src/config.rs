use std::collections::HashSet;
pub type ConfigError = &'static str;
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('0')
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<i64>().is_ok()
}
pub fn parse_allowed_ids(owner: &str, allowed: Option<&str>) -> Result<Vec<String>, ConfigError> {
    if !valid_id(owner) {
        return Err("Invalid TELEGRAM_OWNER_ID");
    }
    let mut ids = vec![owner.to_owned()];
    let mut seen = HashSet::from([owner.to_owned()]);
    if let Some(list) = allowed.filter(|value| !value.trim().is_empty()) {
        for entry in list.split(',') {
            let id = entry.trim();
            if !valid_id(id) {
                return Err("Invalid TELEGRAM_ALLOWED_USER_IDS; use comma-separated positive IDs");
            }
            if seen.insert(id.to_owned()) {
                ids.push(id.to_owned());
            }
        }
    }
    Ok(ids)
}
/// Read deployment-independent model selection from the authoritative template.
pub fn configured_model() -> Result<String, ConfigError> {
    let config: toml::Value = toml::from_str(include_str!("../../deploy/bot/config.template.toml"))
        .map_err(|_| "Invalid embedded runtime configuration")?;
    config
        .get("providers")
        .and_then(|v| v.get("models"))
        .and_then(|v| v.get("openai"))
        .and_then(|v| v.get("codex"))
        .and_then(|v| v.get("model"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or("Missing configured Codex model")
}

pub fn render_config(ids: &[String], model: &str) -> Result<String, ConfigError> {
    if ids.is_empty() || ids.iter().any(|id| !valid_id(id)) {
        return Err("Empty or invalid Telegram allowlist");
    }
    if model.is_empty()
        || model.len() > 128
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
    {
        return Err("Invalid configured Codex model");
    }
    let mut config: toml::Value =
        toml::from_str(include_str!("../../deploy/bot/config.template.toml"))
            .map_err(|_| "Invalid embedded runtime configuration")?;
    // Launcher-owned settings are not part of the upstream ZeroClaw schema.
    config
        .as_table_mut()
        .ok_or("Invalid embedded runtime configuration")?
        .remove("heimdall");
    // These paths are fixed by the embedded template and pinned by configuration tests.
    config["peer_groups"]["telegram_home"]["external_peers"] =
        toml::Value::Array(ids.iter().cloned().map(toml::Value::String).collect());
    config["heartbeat"]["to"] = toml::Value::String(ids[0].clone());
    let codex = config["providers"]["models"]["openai"]["codex"]
        .as_table_mut()
        .ok_or("Invalid embedded Codex provider")?;
    if codex.get("model").and_then(toml::Value::as_str) != Some(model) {
        // Capacity belongs to the configured model, never to an arbitrary override.
        codex.remove("context_window");
    }
    codex.insert("model".into(), toml::Value::String(model.into()));
    if let Some(routes) = config["model_routes"].as_array_mut() {
        for route in routes {
            if route["hint"].as_str() == Some("codex") {
                route["model"] = toml::Value::String(model.into());
            }
        }
    }
    toml::to_string_pretty(&config).map_err(|_| "Cannot render runtime configuration")
}
pub fn secret_overrides(token: &str) -> Result<Vec<(String, String)>, ConfigError> {
    let Some((id, secret)) = token.split_once(':') else {
        return Err("Missing or invalid TELEGRAM_BOT_TOKEN");
    };
    if !valid_id(id)
        || secret.len() < 16
        || !secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err("Missing or invalid TELEGRAM_BOT_TOKEN");
    }
    Ok(vec![(
        "ZEROCLAW_channels__telegram__home__bot_token".into(),
        token.into(),
    )])
}

/// Builds the HTTP Authorization value without putting credentials in rendered TOML.
/// The raw GitHub token is shared with the restricted Codespaces CLI.
pub fn github_authorization(token: &str) -> Result<String, ConfigError> {
    if token.len() < 20 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("Invalid GITHUB_TOKEN; provide a raw token without Bearer or whitespace");
    }
    Ok(format!("Bearer {token}"))
}
