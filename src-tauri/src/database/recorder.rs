use super::Database;
use super::DatabaseError;
use chrono::Utc;
use recorder::platforms::PlatformType;
/// Recorder in database is pretty simple
/// because many room infos are collected in realtime
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct RecorderRow {
    pub room_id: String,
    pub created_at: String,
    pub platform: String,
    pub auto_start: bool,
    pub extra: String,
    pub room_title: Option<String>,
    pub room_cover: Option<String>,
    pub user_name: Option<String>,
    pub user_avatar: Option<String>,
}

// recorders
impl Database {
    pub async fn add_recorder(
        &self,
        platform: PlatformType,
        room_id: &str,
        extra: &str,
    ) -> Result<RecorderRow, DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        let recorder = RecorderRow {
            room_id: room_id.to_string(),
            created_at: Utc::now().to_rfc3339(),
            platform: platform.as_str().to_string(),
            auto_start: true,
            extra: extra.to_string(),
            room_title: None,
            room_cover: None,
            user_name: None,
            user_avatar: None,
        };
        let _ = sqlx::query(
            "INSERT OR REPLACE INTO recorders (room_id, created_at, platform, auto_start, extra) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(room_id)
        .bind(&recorder.created_at)
        .bind(platform.as_str())
        .bind(recorder.auto_start)
        .bind(extra)
        .execute(&lock)
        .await?;
        Ok(recorder)
    }

    pub async fn remove_recorder(&self, room_id: &str) -> Result<RecorderRow, DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        let recorder =
            sqlx::query_as::<_, RecorderRow>("SELECT * FROM recorders WHERE room_id = $1")
                .bind(room_id)
                .fetch_one(&lock)
                .await?;
        let sql = sqlx::query("DELETE FROM recorders WHERE room_id = $1")
            .bind(room_id)
            .execute(&lock)
            .await?;
        if sql.rows_affected() != 1 {
            return Err(DatabaseError::NotFound);
        }

        Ok(recorder)
    }

    pub async fn get_recorders(&self) -> Result<Vec<RecorderRow>, DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        Ok(sqlx::query_as::<_, RecorderRow>(
            "SELECT room_id, created_at, platform, auto_start, extra, room_title, room_cover, user_name, user_avatar FROM recorders",
        )
        .fetch_all(&lock)
        .await?)
    }

    pub async fn get_recorder(
        &self,
        platform: PlatformType,
        room_id: &str,
    ) -> Result<RecorderRow, DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        Ok(sqlx::query_as::<_, RecorderRow>(
            "SELECT room_id, created_at, platform, auto_start, extra, room_title, room_cover, user_name, user_avatar FROM recorders WHERE platform = $1 AND room_id = $2",
        )
        .bind(platform.as_str().to_string())
        .bind(room_id)
        .fetch_one(&lock)
        .await?)
    }

    pub async fn remove_archive(&self, room_id: &str) -> Result<(), DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        let _ = sqlx::query("DELETE FROM records WHERE room_id = $1")
            .bind(room_id)
            .execute(&lock)
            .await?;
        Ok(())
    }

    pub async fn update_recorder(
        &self,
        platform: PlatformType,
        room_id: &str,
        auto_start: bool,
    ) -> Result<(), DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        let _ = sqlx::query(
            "UPDATE recorders SET auto_start = $1 WHERE platform = $2 AND room_id = $3",
        )
        .bind(auto_start)
        .bind(platform.as_str().to_string())
        .bind(room_id)
        .execute(&lock)
        .await?;
        Ok(())
    }

    pub async fn update_recorder_cached_info(
        &self,
        platform: PlatformType,
        room_id: &str,
        room_title: &str,
        room_cover: &str,
        user_name: &str,
        user_avatar: &str,
    ) -> Result<(), DatabaseError> {
        let lock = self.db.read().await.clone().unwrap();
        let _ = sqlx::query(
            "UPDATE recorders SET
                room_title = COALESCE(NULLIF($1, ''), room_title),
                room_cover = COALESCE(NULLIF($2, ''), room_cover),
                user_name = COALESCE(NULLIF($3, ''), user_name),
                user_avatar = COALESCE(NULLIF($4, ''), user_avatar)
             WHERE platform = $5 AND room_id = $6",
        )
        .bind(room_title)
        .bind(room_cover)
        .bind(user_name)
        .bind(user_avatar)
        .bind(platform.as_str().to_string())
        .bind(room_id)
        .execute(&lock)
        .await?;
        Ok(())
    }
}
