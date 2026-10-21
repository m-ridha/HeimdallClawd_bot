//! Preserve operator-owned native skill bundles across managed configuration rendering.
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const MAX_CONFIG_BYTES: u64 = 1_048_576;

fn failure() -> io::Error {
    io::Error::other("Cannot safely read skill bundle configuration")
}

/// Read the persistent configuration without following links or exposing its contents.
/// Absence is valid on first boot; malformed or unreadable state must not be overwritten.
pub fn read_config(path: &Path) -> io::Result<Option<String>> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure()),
    };
    if !file.metadata().map_err(|_| failure())?.is_file() {
        return Err(failure());
    }
    let mut content = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(|_| failure())?;
    if content.len() as u64 > MAX_CONFIG_BYTES {
        return Err(failure());
    }
    Ok(Some(content))
}

/// Import only native skill-bundle definitions and assign all of them to primary.
/// The embedded managed bundle wins; unrelated persisted settings are never imported.
/// Sorted extra aliases make native first-name-wins skill precedence deterministic.
pub fn attach(base: &str, previous: Option<&str>) -> Result<String, &'static str> {
    let mut config: toml::Value =
        toml::from_str(base).map_err(|_| "Invalid base skill bundle configuration")?;
    let mut bundles = config
        .get("skill_bundles")
        .and_then(toml::Value::as_table)
        .cloned()
        .ok_or("Missing base skill bundles")?;
    if let Some(previous) = previous {
        let saved: toml::Value =
            toml::from_str(previous).map_err(|_| "Invalid saved skill bundle configuration")?;
        if let Some(entries) = saved.get("skill_bundles") {
            let entries = entries.as_table().ok_or("Invalid saved skill bundles")?;
            for (alias, entry) in entries {
                if alias == "managed" {
                    continue;
                }
                if alias.is_empty()
                    || alias.len() > 64
                    || !alias.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
                    })
                {
                    return Err("Invalid saved skill bundle alias");
                }
                let entry = entry.as_table().ok_or("Invalid saved skill bundle entry")?;
                for (key, value) in entry {
                    let valid = match key.as_str() {
                        "directory" => value.as_str().is_some(),
                        "include" | "exclude" => value
                            .as_array()
                            .is_some_and(|items| items.iter().all(|item| item.as_str().is_some())),
                        _ => false,
                    };
                    if !valid {
                        return Err("Invalid saved skill bundle field");
                    }
                }
                bundles.insert(alias.clone(), toml::Value::Table(entry.clone()));
            }
        }
    }
    let mut aliases: Vec<_> = bundles
        .keys()
        .filter(|alias| *alias != "managed")
        .cloned()
        .collect();
    aliases.sort();
    aliases.insert(0, "managed".into());
    let primary = config
        .get_mut("agents")
        .and_then(|agents| agents.get_mut("primary"))
        .and_then(toml::Value::as_table_mut)
        .ok_or("Missing primary agent")?;
    primary.insert(
        "skill_bundles".into(),
        toml::Value::Array(aliases.into_iter().map(toml::Value::String).collect()),
    );
    config["skill_bundles"] = toml::Value::Table(bundles);
    toml::to_string_pretty(&config).map_err(|_| "Cannot render skill bundle configuration")
}
