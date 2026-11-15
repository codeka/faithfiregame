use std::path::Path;
use std::str::FromStr;

use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use uuid::Uuid;

use super::repository::Storage;

pub struct SqliteStorage {
    pool: sqlx::SqlitePool,
}

impl SqliteStorage {
    pub async fn open(db_path: &Path) -> anyhow::Result<Self> {
        let url = format!("sqlite://{}", db_path.to_string_lossy());
        let opts = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL UNIQUE
            );",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                token TEXT PRIMARY KEY,
                account_id INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

impl Storage for SqliteStorage {
    async fn create_dev_session(&self, username: &str) -> anyhow::Result<(u64, String)> {
        sqlx::query("INSERT OR IGNORE INTO accounts (username) VALUES (?);")
            .bind(username)
            .execute(&self.pool)
            .await?;

        let row = sqlx::query("SELECT id FROM accounts WHERE username = ?;")
            .bind(username)
            .fetch_one(&self.pool)
            .await?;
        let account_id: i64 = row.try_get("id")?;

        let token = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO sessions (token, account_id) VALUES (?, ?);")
            .bind(&token)
            .bind(account_id)
            .execute(&self.pool)
            .await?;

        Ok((account_id as u64, token))
    }

    async fn validate_session(&self, token: &str) -> anyhow::Result<Option<u64>> {
        let row = sqlx::query("SELECT account_id FROM sessions WHERE token = ?;")
            .bind(token)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|r| r.try_get::<i64, _>("account_id").ok().map(|id| id as u64)))
    }
}
