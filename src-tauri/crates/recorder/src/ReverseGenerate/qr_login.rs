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

fn set_or_update(table: &mut toml::value::Table, key: &str, value: &str) -> bool {
    if let Some(TomlValue::String(v)) = table.get(key) {
        if v == value {
            return false;
        }
    }
    table.insert(key.to_string(), TomlValue::String(value.to_string()));
    true
}

pub fn update_tiktok_config(
    device_id: Option<&str>,
    verify_fp: Option<&str>,
    ttwid: Option<&str>,
    force_update: bool,
) -> Result<(), std::io::Error> {
    let root = match crate::platforms::douyin::api::reverse_generate_root() {
        Some(path) => path,
        None => return Ok(()),
    };
    let path = root.join("qr_login.toml");
    
    // Read existing
    let raw = fs::read_to_string(&path).unwrap_or_default();
    let mut parsed: TomlValue = raw.parse().unwrap_or_else(|_| TomlValue::Table(Default::default()));
    
    let table = match table_mut(&mut parsed) {
        Some(table) => table,
        None => return Ok(()),
    };

    let tiktok = table
        .entry("tiktok")
        .or_insert_with(|| TomlValue::Table(Default::default()));
    
    let mut changed = false;
    if let Some(tiktok_table) = tiktok.as_table_mut() {
        if let Some(v) = device_id {
            if !v.is_empty() {
                if force_update {
                    changed |= set_or_update(tiktok_table, "device_id", v);
                } else {
                    tiktok_table.insert("device_id".to_string(), TomlValue::String(v.to_string()));
                    changed = true;
                }
            }
        }
        if let Some(v) = verify_fp {
            if !v.is_empty() {
                if force_update {
                    changed |= set_or_update(tiktok_table, "verify_fp", v);
                } else {
                    tiktok_table.insert("verify_fp".to_string(), TomlValue::String(v.to_string()));
                    changed = true;
                }
            }
        }
        if let Some(v) = ttwid {
            if !v.is_empty() {
                if force_update {
                    changed |= set_or_update(tiktok_table, "ttwid_migration_ticket", v);
                } else {
                    tiktok_table.insert("ttwid_migration_ticket".to_string(), TomlValue::String(v.to_string()));
                    changed = true;
                }
            }
        }
    }

    if changed {
        let serialized = toml::to_string_pretty(&parsed).unwrap_or(raw);
        fs::write(path, serialized)?;
    }
    
    Ok(())
}

pub fn update_kuaishou_config(
    did: Option<&str>,
    _user_id: Option<&str>,
    force_update: bool,
) -> Result<(), std::io::Error> {
    let root = match crate::platforms::douyin::api::reverse_generate_root() {
        Some(path) => path,
        None => return Ok(()),
    };
    let path = root.join("qr_login.toml");
    
    // Read existing
    let raw = fs::read_to_string(&path).unwrap_or_default();
    let mut parsed: TomlValue = raw.parse().unwrap_or_else(|_| TomlValue::Table(Default::default()));
    
    let table = match table_mut(&mut parsed) {
        Some(table) => table,
        None => return Ok(()),
    };

    let kuaishou = table
        .entry("kuaishou")
        .or_insert_with(|| TomlValue::Table(Default::default()));
    
    let mut changed = false;
    if let Some(kuaishou_table) = kuaishou.as_table_mut() {
        if let Some(v) = did {
            if !v.is_empty() {
                if force_update {
                    changed |= set_or_update(kuaishou_table, "device_id", v);
                } else {
                    // Original logic was implicit insert (overwrite) in my reading of `insert`,
                    // but `set_if_empty` was used elsewhere.
                    // The original code used `insert` directly:
                    // kuaishou_table.insert("device_id".to_string(), TomlValue::String(v.to_string()));
                    // So it WAS overwriting.
                    // Wait, `kuaishou_table.insert` definitely overwrites. 
                    // Let's keep `set_or_update` to be safe and `changed` tracking.
                    changed |= set_or_update(kuaishou_table, "device_id", v);
                }
            }
        }
        // If we want to save user_id (not currently in config standard but useful)
        // For now sticking to device_id
    }

    if changed {
        let serialized = toml::to_string_pretty(&parsed).unwrap_or(raw);
        fs::write(path, serialized)?;
    }
    
    Ok(())
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
    
    // Ensure critical Douyin params are set if using automatic construction
    if read_string(douyin_table, "params_raw").unwrap_or_default().is_empty() {
        changed |= set_if_empty(douyin_table, "aid", "6383");
        changed |= set_if_empty(douyin_table, "device_platform", "web_app");
        changed |= set_if_empty(douyin_table, "service", "https://www.douyin.com");
    }

    // TikTok defaults
    let tiktok = table
        .entry("tiktok")
        .or_insert_with(|| TomlValue::Table(Default::default()));
    if let Some(tiktok_table) = tiktok.as_table_mut() {
        changed |= set_if_empty(
            tiktok_table,
            "user_agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
        );
        // Generate valid-looking defaults instead of empty strings
        if read_string(tiktok_table, "device_id").unwrap_or_default().is_empty() {
            let did = super::utils::gen_random_numeric(19);
            tiktok_table.insert("device_id".to_string(), TomlValue::String(did));
            changed = true;
        } else {
             changed |= set_if_empty(tiktok_table, "device_id", "");
        }

        if read_string(tiktok_table, "verify_fp").unwrap_or_default().is_empty() {
            let fp = super::utils::gen_tiktok_verify_fp();
            tiktok_table.insert("verify_fp".to_string(), TomlValue::String(fp));
            changed = true;
        } else {
             changed |= set_if_empty(tiktok_table, "verify_fp", "");
        }
        
        changed |= set_if_empty(tiktok_table, "ms_token", "");
        changed |= set_if_empty(tiktok_table, "ttwid_migration_ticket", "");
    }

    // Kuaishou defaults
    let kuaishou = table
        .entry("kuaishou")
        .or_insert_with(|| TomlValue::Table(Default::default()));
    if let Some(kuaishou_table) = kuaishou.as_table_mut() {
        changed |= set_if_empty(
            kuaishou_table,
            "user_agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
        );
        changed |= set_if_empty(kuaishou_table, "cookie", "");
        changed |= set_if_empty(kuaishou_table, "device_id", "");
    }

    if changed {
        let serialized = toml::to_string_pretty(&parsed).unwrap_or(raw);
        fs::write(path, serialized)?;
    }
    Ok(())
}

pub fn fetch_kuaishou_overrides() -> Option<std::collections::HashMap<String, String>> {
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
    if let Some(table) = parsed.get("kuaishou").and_then(|v| v.as_table()) {
        for (k, v) in table {
            if let Some(s) = v.as_str() {
                map.insert(k.to_string(), s.to_string());
            }
        }
    }
    
    if map.is_empty() { None } else { Some(map) }
}

pub fn get_or_create_kuaishou_did() -> String {
    if let Some(overrides) = fetch_kuaishou_overrides() {
        if let Some(did) = overrides.get("device_id").filter(|v| !v.is_empty()) {
             return did.clone();
        }
    }
    
    // Generate new DID
    let did = crate::reverse_generate::kuaishou_sign::gen_kuaishou_web_did();
    // Save it
    let _ = update_kuaishou_config(Some(&did), None, true);
    
    did
}
