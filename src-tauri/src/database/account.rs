use recorder::account::Account;
use serde_json;

use super::Database;
use super::DatabaseError;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct AccountRow {
    pub platform: String,
    pub uid: String,
    pub name: String,
    pub avatar: String,
    pub csrf: String,
    pub cookies: String,
    pub extra: String,
    pub created_at: String,
}

impl AccountRow {
    pub fn to_account(&self) -> Account {
        let cookies = merge_kuaishou_extra_cookies(&self.platform, &self.cookies, &self.extra);
        Account {
            platform: self.platform.clone(),
            id: self.uid.clone(),
            name: self.name.clone(),
            avatar: self.avatar.clone(),
            csrf: self.csrf.clone(),
            cookies,
        }
    }
}

fn merge_kuaishou_extra_cookies(platform: &str, cookies: &str, extra: &str) -> String {
    if platform.to_ascii_lowercase() != "kuaishou" {
        return cookies.to_string();
    }
    if extra.trim().is_empty() {
        return filter_kuaishou_cookie_header(cookies);
    }
    let mut map = std::collections::HashMap::<String, String>::new();
    for part in cookies.split(';').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        if let Some((k, v)) = part.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    let parsed: serde_json::Value = match serde_json::from_str(extra) {
        Ok(v) => v,
        Err(_) => return cookies.to_string(),
    };
    let Some(items) = parsed
        .get("cookie_info")
        .and_then(|v| v.get("cookies"))
        .and_then(|v| v.as_array())
    else {
        return cookies.to_string();
    };

    for item in items {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = item.get("value").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() || value.is_empty() {
            continue;
        }
        map.entry(name.to_string())
            .or_insert_with(|| value.to_string());
    }

    if map.is_empty() {
        return filter_kuaishou_cookie_header(cookies);
    }
    let merged = map
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    filter_kuaishou_cookie_header(&merged)
}

fn filter_kuaishou_cookie_header(cookies: &str) -> String {
    let mut kept = Vec::new();
    for part in cookies.split(';').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        if key.is_empty() || val.is_empty() {
            continue;
        }
        kept.push(format!("{}={}", key, val));
    }
    kept.join("; ")
}

// accounts
impl Database {
    // CREATE TABLE accounts (uid INTEGER PRIMARY KEY, name TEXT, avatar TEXT, csrf TEXT, cookies TEXT, created_at TEXT);
    pub async fn add_account(&self, account: &AccountRow) -> Result<(), DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        sqlx::query(
            "INSERT INTO accounts (uid, platform, name, avatar, csrf, cookies, extra, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT(uid, platform) DO UPDATE SET
                name = excluded.name,
                avatar = excluded.avatar,
                csrf = excluded.csrf,
                cookies = excluded.cookies,
                extra = excluded.extra,
                created_at = excluded.created_at",
        )
        .bind(&account.uid)
        .bind(&account.platform)
        .bind(&account.name)
        .bind(&account.avatar)
        .bind(&account.csrf)
        .bind(&account.cookies)
        .bind(&account.extra)
        .bind(&account.created_at)
        .execute(&lock)
        .await?;

        Ok(())
    }

    pub async fn remove_account(&self, platform: &str, uid: &str) -> Result<(), DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        let sql = sqlx::query("DELETE FROM accounts WHERE uid = $1 and platform = $2")
            .bind(uid)
            .bind(platform)
            .execute(&lock)
            .await?;
        if sql.rows_affected() == 0 {
            return Err(DatabaseError::NotFound);
        }
        Ok(())
    }

    pub async fn get_accounts(&self) -> Result<Vec<AccountRow>, DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        Ok(sqlx::query_as::<_, AccountRow>("SELECT * FROM accounts")
            .fetch_all(&lock)
            .await?)
    }

    pub async fn get_account(
        &self,
        platform: &str,
        uid: &str,
    ) -> Result<AccountRow, DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        Ok(sqlx::query_as::<_, AccountRow>(
            "SELECT * FROM accounts WHERE uid = $1 and platform = $2",
        )
        .bind(uid)
        .bind(platform)
        .fetch_one(&lock)
        .await?)
    }

    pub async fn get_account_by_platform(
        &self,
        platform: &str,
    ) -> Result<AccountRow, DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        let accounts =
            sqlx::query_as::<_, AccountRow>("SELECT * FROM accounts WHERE platform = $1")
                .bind(platform)
                .fetch_all(&lock)
                .await?;
        if accounts.is_empty() {
            return Err(DatabaseError::NotFound);
        }

        // Return the "best" account (highest profile score)
        // We no longer automatically merge accounts to ensure that Guest and Real identities remain distinct.
        let best = accounts
            .iter()
            .max_by_key(|account| account_profile_score(account))
            .ok_or(DatabaseError::NotFound)?;

        Ok(best.clone())
    }
}

fn account_profile_score(account: &AccountRow) -> usize {
    let mut score = 0usize;
    if !account.uid.trim().is_empty() {
        score += 1;
    }
    if !account.name.trim().is_empty() {
        score += 1;
    }
    if !account.avatar.trim().is_empty() {
        score += 1;
    }
    if !account.csrf.trim().is_empty() {
        score += 1;
    }
    if !account.cookies.trim().is_empty() {
        score += 1;
    }

    let cookie_pairs = cookie_kv_count(&account.cookies);
    score * 100 + cookie_pairs
}

fn cookie_kv_count(cookies: &str) -> usize {
    cookies
        .split(';')
        .map(str::trim)
        .filter(|pair| pair.contains('='))
        .count()
}
