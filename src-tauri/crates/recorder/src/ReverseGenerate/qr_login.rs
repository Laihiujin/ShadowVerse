use std::fs;
use std::path::Path;

use toml::Value as TomlValue;

const DOUYIN_PASSPORT_UA_DEFAULT: &str =
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

fn read_string(table: &toml::value::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

fn build_params_raw_status_from_params_raw(raw: &str) -> Option<String> {
    if raw.trim().is_empty() {
        return None;
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for part in raw.split('&') {
        let mut iter = part.splitn(2, '=');
        let key = iter.next()?.trim();
        let value = iter.next().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }
        if matches!(key, "next" | "need_short_url" | "need_logo") {
            continue;
        }
        pairs.push((key.to_string(), value.to_string()));
    }
    if pairs.is_empty() {
        return None;
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(&key, &value);
    }
    Some(serializer.finish())
}

pub fn ensure_qr_login_defaults(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(path)?;
    let mut parsed: TomlValue = raw.parse().unwrap_or_else(|_| TomlValue::Table(Default::default()));
    let table = match table_mut(&mut parsed) {
        Some(table) => table,
        None => return Ok(()),
    };
    let douyin = table
        .entry("douyin")
        .or_insert_with(|| TomlValue::Table(Default::default()));
    let Some(douyin_table) = douyin.as_table_mut() else {
        return Ok(());
    };

    let mut changed = false;
    changed |= set_if_empty(douyin_table, "user_agent", DOUYIN_PASSPORT_UA_DEFAULT);
    changed |= set_if_empty(douyin_table, "challenge_body", "");
    changed |= set_if_empty(
        douyin_table,
        "challenge_content_type",
        "application/x-www-form-urlencoded",
    );

    let params_raw = read_string(douyin_table, "params_raw").unwrap_or_default();
    if read_string(douyin_table, "params_raw_status")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        if let Some(status) = build_params_raw_status_from_params_raw(&params_raw) {
            changed |= set_if_empty(douyin_table, "params_raw_status", &status);
        }
    }

    for key in [
        "x_tt_passport_csrf_token",
        "x_tt_passport_aid_sign",
        "x_tt_passport_trace_id",
        "x_tt_passport_verify_portrait",
        "x_tt_session_dtrait",
    ] {
        changed |= set_if_empty(douyin_table, key, "");
    }

    if changed {
        let serialized = toml::to_string_pretty(&parsed).unwrap_or(raw);
        fs::write(path, serialized)?;
    }
    Ok(())
}
