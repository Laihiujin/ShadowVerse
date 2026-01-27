use std::fs;
use std::path::Path;

use toml::Value as TomlValue;

const TIKTOK_FEED_UA_DEFAULT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

fn table_mut(value: &mut TomlValue) -> Option<&mut toml::value::Table> {
    value.as_table_mut()
}

fn set_if_empty(table: &mut toml::value::Table, key: &str, value: &str) -> bool {
    match table.get(key) {
        Some(TomlValue::String(v)) => {
            if v == value {
                return false;
            }
            if v.trim().is_empty() {
                if value.is_empty() {
                    return false;
                }
                table.insert(key.to_string(), TomlValue::String(value.to_string()));
                return true;
            }
            false
        }
        Some(_) => false,
        None => {
            table.insert(key.to_string(), TomlValue::String(value.to_string()));
            true
        }
    }
}

pub fn ensure_tiktok_web_defaults(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(path)?;
    let mut parsed: TomlValue = raw.parse().unwrap_or_else(|_| TomlValue::Table(Default::default()));
    let table = match table_mut(&mut parsed) {
        Some(table) => table,
        None => return Ok(()),
    };
    let tiktok = table
        .entry("tiktok_web")
        .or_insert_with(|| TomlValue::Table(Default::default()));
    let Some(tiktok_table) = tiktok.as_table_mut() else {
        return Ok(());
    };

    let mut changed = false;
    changed |= set_if_empty(tiktok_table, "user_agent", TIKTOK_FEED_UA_DEFAULT);
    changed |= set_if_empty(tiktok_table, "x_gnarly", "");

    if changed {
        let serialized = toml::to_string_pretty(&parsed).unwrap_or(raw);
        fs::write(path, serialized)?;
    }
    Ok(())
}

pub fn fetch_tiktok_overrides() -> Option<std::collections::HashMap<String, String>> {
    let root = match crate::platforms::douyin::api::reverse_generate_root() {
        Some(path) => path,
        None => return None,
    };
    let path = root.join("qr_login.toml");
    if !path.exists() {
        return None;
    }
    
    let raw = fs::read_to_string(path).ok()?;
    let parsed: TomlValue = raw.parse().ok()?;
    
    let mut map = std::collections::HashMap::new();
    if let Some(table) = parsed.get("tiktok").and_then(|v| v.as_table()) {
        for (k, v) in table {
            if let Some(s) = v.as_str() {
                map.insert(k.to_string(), s.to_string());
            }
        }
    }
    
    if map.is_empty() { None } else { Some(map) }
}
