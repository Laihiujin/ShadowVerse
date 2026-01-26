use recorder::account::Account;

use super::Database;
use super::DatabaseError;
use rand::seq::SliceRandom;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct AccountRow {
    pub platform: String,
    pub uid: String,
    pub name: String,
    pub avatar: String,
    pub csrf: String,
    pub cookies: String,
    pub created_at: String,
}

impl AccountRow {
    pub fn to_account(&self) -> Account {
        Account {
            platform: self.platform.clone(),
            id: self.uid.clone(),
            name: self.name.clone(),
            avatar: self.avatar.clone(),
            csrf: self.csrf.clone(),
            cookies: self.cookies.clone(),
        }
    }
}

// accounts
impl Database {
    // CREATE TABLE accounts (uid INTEGER PRIMARY KEY, name TEXT, avatar TEXT, csrf TEXT, cookies TEXT, created_at TEXT);
    pub async fn add_account(&self, account: &AccountRow) -> Result<(), DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        sqlx::query(
            "INSERT INTO accounts (uid, platform, name, avatar, csrf, cookies, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT(uid, platform) DO UPDATE SET
                name = excluded.name,
                avatar = excluded.avatar,
                csrf = excluded.csrf,
                cookies = excluded.cookies,
                created_at = excluded.created_at",
        )
        .bind(&account.uid)
        .bind(&account.platform)
        .bind(&account.name)
        .bind(&account.avatar)
        .bind(&account.csrf)
        .bind(&account.cookies)
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
        if should_merge_accounts(platform) {
            let best = accounts
                .iter()
                .max_by_key(|account| account_profile_score(account))
                .unwrap();
            let merged = merge_accounts(best, &accounts);
            if merged.name != best.name
                || merged.avatar != best.avatar
                || merged.csrf != best.csrf
                || merged.cookies != best.cookies
            {
                let _ = self.add_account(&merged).await;
                let merged_cookie_key = normalize_cookie_string(&merged.cookies);
                for account in accounts.iter() {
                    if account.uid == merged.uid {
                        continue;
                    }
                    if normalize_cookie_string(&account.cookies) == merged_cookie_key {
                        let _ = self.remove_account(&account.platform, &account.uid).await;
                    }
                }
            }
            return Ok(merged);
        }

        // randomly select one account
        let account = accounts.choose(&mut rand::thread_rng()).unwrap();
        Ok(account.clone())
    }
}

fn should_merge_accounts(platform: &str) -> bool {
    matches!(platform, "tiktok" | "douyin" | "kuaishou" | "huya")
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

fn cookie_kv_map(cookies: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in cookies.split(';').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        map.insert(key.to_string(), value.trim().to_string());
    }
    map
}

fn normalize_cookie_string(cookies: &str) -> String {
    let mut pairs: Vec<String> = cookie_kv_map(cookies)
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    pairs.sort();
    pairs.join("; ")
}

fn merge_accounts(base: &AccountRow, accounts: &[AccountRow]) -> AccountRow {
    let mut merged = base.clone();
    let mut cookies = cookie_kv_map(&merged.cookies);

    for account in accounts {
        if merged.name.trim().is_empty() && !account.name.trim().is_empty() {
            merged.name = account.name.clone();
        }
        if merged.avatar.trim().is_empty() && !account.avatar.trim().is_empty() {
            merged.avatar = account.avatar.clone();
        }
        if merged.csrf.trim().is_empty() && !account.csrf.trim().is_empty() {
            merged.csrf = account.csrf.clone();
        }

        let extra = cookie_kv_map(&account.cookies);
        for (key, value) in extra {
            match cookies.get(&key) {
                Some(existing) => {
                    let existing = existing.trim();
                    let incoming = value.trim();
                    if existing.is_empty()
                        || (incoming.len() > existing.len() && !incoming.is_empty())
                    {
                        cookies.insert(key, value);
                    }
                }
                _ => {
                    if !value.trim().is_empty() {
                        cookies.insert(key, value);
                    }
                }
            }
        }
    }

    merged.cookies = normalize_cookie_string(
        &cookies
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("; "),
    );
    merged
}
