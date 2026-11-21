use std::path::Path;

use anyhow::{Context, bail};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use shared::character::CharacterDefinition;
use shared::entity::InventoryConfig;
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use uuid::Uuid;

use super::repository::{CharacterSaveRecord, Storage};

type HmacSha256 = Hmac<Sha256>;
const PBKDF2_ITERS: u32 = 60_000;

fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iters: u32) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(password).expect("HMAC accepts any key length");
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u: [u8; 32] = mac.finalize().into_bytes().into();
    let mut out = u;
    for _ in 1..iters {
        let mut next = HmacSha256::new_from_slice(password).expect("HMAC accepts any key length");
        next.update(&u);
        u = next.finalize().into_bytes().into();
        for (dst, src) in out.iter_mut().zip(u.iter()) {
            *dst ^= *src;
        }
    }
    out
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

fn hash_password(password: &str) -> String {
    let salt = *Uuid::new_v4().as_bytes();
    let dk = pbkdf2_hmac_sha256(password.as_bytes(), &salt, PBKDF2_ITERS);
    format!("pbkdf2${PBKDF2_ITERS}${}${}", to_hex(&salt), to_hex(&dk))
}

fn verify_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2" {
        return false;
    }
    let Ok(iters) = parts[1].parse::<u32>() else {
        return false;
    };
    if iters == 0 || iters > 500_000 {
        return false;
    }
    let (Some(salt), Some(expected)) = (from_hex(parts[2]), from_hex(parts[3])) else {
        return false;
    };
    if expected.len() != 32 {
        return false;
    }
    let actual = pbkdf2_hmac_sha256(password.as_bytes(), &salt, iters);
    let mut diff = 0u8;
    for (a, b) in actual.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub struct SqliteStorage {
    pool: sqlx::SqlitePool,
}

impl SqliteStorage {
    pub async fn open(db_path: &Path) -> anyhow::Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        // Replace early prototype `accounts` table if it lacks `password_hash`.
        let accounts_exists = sqlx::query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'accounts';",
        )
        .fetch_optional(&pool)
        .await?
        .is_some();

        if accounts_exists {
            let has_pw_hash = sqlx::query(
                "SELECT 1 FROM pragma_table_info('accounts') WHERE name = 'password_hash';",
            )
            .fetch_optional(&pool)
            .await?
            .is_some();
            if !has_pw_hash {
                sqlx::query("DROP TABLE IF EXISTS sessions;")
                    .execute(&pool)
                    .await?;
                sqlx::query("DROP TABLE IF EXISTS accounts;")
                    .execute(&pool)
                    .await?;
            }
        }

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT UNIQUE,
                password_hash TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
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

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS character_saves (
                account_id INTEGER PRIMARY KEY,
                area_id TEXT NOT NULL,
                pos_x REAL NOT NULL,
                pos_y REAL NOT NULL,
                pos_z REAL NOT NULL,
                yaw REAL NOT NULL,
                appearance_json TEXT NOT NULL,
                inventory_json TEXT NOT NULL DEFAULT '{\"items\":[]}',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )
        .execute(&pool)
        .await?;

        let has_inventory_col = sqlx::query(
            "SELECT 1 FROM pragma_table_info('character_saves') WHERE name = 'inventory_json';",
        )
        .fetch_optional(&pool)
        .await?
        .is_some();
        if !has_inventory_col {
            sqlx::query(
                "ALTER TABLE character_saves ADD COLUMN inventory_json TEXT NOT NULL DEFAULT '{\"items\":[]}';",
            )
            .execute(&pool)
            .await?;
        }

        Ok(Self { pool })
    }

    async fn insert_session(&self, account_id: i64) -> anyhow::Result<String> {
        let token = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO sessions (token, account_id) VALUES (?, ?);")
            .bind(&token)
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(token)
    }
}

impl Storage for SqliteStorage {
    async fn create_guest_session(&self) -> anyhow::Result<(u64, String)> {
        let res = sqlx::query("INSERT INTO accounts DEFAULT VALUES;")
            .execute(&self.pool)
            .await?;
        let account_id = res.last_insert_rowid();
        let token = self.insert_session(account_id).await?;
        Ok((account_id as u64, token))
    }

    async fn register_account(
        &self,
        existing_account_id: Option<u64>,
        username: &str,
        password: &str,
    ) -> anyhow::Result<u64> {
        let password_hash = hash_password(password);
        if let Some(acc_id) = existing_account_id {
            let res = sqlx::query(
                "UPDATE accounts SET username = ?, password_hash = ? WHERE id = ? AND username IS NULL;",
            )
            .bind(username)
            .bind(&password_hash)
            .bind(acc_id as i64)
            .execute(&self.pool)
            .await?;
            if res.rows_affected() == 0 {
                bail!("account does not exist or is already registered");
            }
            Ok(acc_id)
        } else {
            let res = sqlx::query("INSERT INTO accounts (username, password_hash) VALUES (?, ?);")
                .bind(username)
                .bind(&password_hash)
                .execute(&self.pool)
                .await?;
            Ok(res.last_insert_rowid() as u64)
        }
    }

    async fn login_with_password(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<Option<(u64, String)>> {
        let row = sqlx::query("SELECT id, password_hash FROM accounts WHERE username = ?;")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let account_id: i64 = row.try_get("id")?;
        let password_hash: Option<String> = row.try_get("password_hash")?;
        let Some(hash) = password_hash else {
            return Ok(None);
        };
        if !verify_password(password, &hash) {
            return Ok(None);
        }
        let token = self.insert_session(account_id).await?;
        Ok(Some((account_id as u64, token)))
    }

    async fn validate_session(&self, token: &str) -> anyhow::Result<Option<u64>> {
        let row = sqlx::query("SELECT account_id FROM sessions WHERE token = ?;")
            .bind(token)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|r| r.try_get::<i64, _>("account_id").ok().map(|id| id as u64)))
    }

    async fn delete_session(&self, token: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token = ?;")
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn load_character_save(&self, account_id: u64) -> anyhow::Result<Option<CharacterSaveRecord>> {
        let row = sqlx::query(
            "SELECT area_id, pos_x, pos_y, pos_z, yaw, appearance_json, inventory_json FROM character_saves WHERE account_id = ?;",
        )
        .bind(account_id as i64)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let area_id: String = row.try_get("area_id")?;
        let pos_x: f64 = row.try_get("pos_x")?;
        let pos_y: f64 = row.try_get("pos_y")?;
        let pos_z: f64 = row.try_get("pos_z")?;
        let yaw: f64 = row.try_get("yaw")?;
        let appearance_json: String = row.try_get("appearance_json")?;
        let appearance: CharacterDefinition = serde_json::from_str(&appearance_json)
            .context("invalid appearance_json in character_saves")?;
        let inventory_json: String = row.try_get("inventory_json")?;
        let inventory: InventoryConfig = serde_json::from_str(&inventory_json)
            .context("invalid inventory_json in character_saves")?;
        Ok(Some(CharacterSaveRecord {
            account_id,
            area_id,
            pos: [pos_x as f32, pos_y as f32, pos_z as f32],
            yaw: yaw as f32,
            appearance,
            inventory,
        }))
    }

    async fn save_character_state(&self, save: &CharacterSaveRecord) -> anyhow::Result<()> {
        let appearance_json = serde_json::to_string(&save.appearance)?;
        let inventory_json = serde_json::to_string(&save.inventory)?;
        sqlx::query(
            "INSERT INTO character_saves (account_id, area_id, pos_x, pos_y, pos_z, yaw, appearance_json, inventory_json, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, unixepoch())
             ON CONFLICT(account_id) DO UPDATE SET
                 area_id = excluded.area_id,
                 pos_x = excluded.pos_x,
                 pos_y = excluded.pos_y,
                 pos_z = excluded.pos_z,
                 yaw = excluded.yaw,
                 appearance_json = excluded.appearance_json,
                 inventory_json = excluded.inventory_json,
                 updated_at = unixepoch();",
        )
        .bind(save.account_id as i64)
        .bind(&save.area_id)
        .bind(save.pos[0] as f64)
        .bind(save.pos[1] as f64)
        .bind(save.pos[2] as f64)
        .bind(save.yaw as f64)
        .bind(appearance_json)
        .bind(inventory_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ff_test_{label}_{}.db", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn guest_register_login_logout_and_character_save_roundtrip() {
        let path = temp_db_path("auth_save");
        let db = SqliteStorage::open(&path).await.expect("open db");

        // 1. Play as guest mints an account and session token.
        let (guest_id, guest_token) = db.create_guest_session().await.expect("guest session");
        assert_eq!(db.validate_session(&guest_token).await.unwrap(), Some(guest_id));
        assert!(db.load_character_save(guest_id).await.unwrap().is_none());

        // 2. Register credentials on the guest account and verify login.
        let reg_id = db
            .register_account(Some(guest_id), "survivor", "correct-horse-battery")
            .await
            .expect("register guest account");
        assert_eq!(reg_id, guest_id);

        assert!(
            db.login_with_password("survivor", "wrong-pass")
                .await
                .unwrap()
                .is_none()
        );
        let (login_id, login_token) = db
            .login_with_password("survivor", "correct-horse-battery")
            .await
            .unwrap()
            .expect("valid login");
        assert_eq!(login_id, guest_id);
        assert_eq!(db.validate_session(&login_token).await.unwrap(), Some(guest_id));

        // 3. Delete session invalidates token.
        db.delete_session(&login_token).await.unwrap();
        assert_eq!(db.validate_session(&login_token).await.unwrap(), None);

        // 4. Character save roundtrip.
        let save = CharacterSaveRecord {
            account_id: guest_id,
            area_id: "main".to_owned(),
            pos: [32.5, 0.25, 31.0],
            yaw: 1.25,
            appearance: CharacterDefinition::cryo_awakening_default(),
            inventory: InventoryConfig {
                items: vec![shared::entity::InventoryEntry {
                    item_id: "guard_rifle".to_owned(),
                    count: 1,
                    equipped: true,
                }],
            },
        };
        db.save_character_state(&save).await.unwrap();
        let loaded = db
            .load_character_save(guest_id)
            .await
            .unwrap()
            .expect("saved character");
        assert_eq!(loaded.area_id, "main");
        assert_eq!(loaded.pos, [32.5, 0.25, 31.0]);
        assert!((loaded.yaw - 1.25).abs() < 1e-6);
        assert_eq!(loaded.appearance, save.appearance);
        assert_eq!(loaded.inventory, save.inventory);

        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn upgrades_legacy_accounts_table_without_password_hash() {
        let path = temp_db_path("legacy_upgrade");
        {
            let opts = SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true);
            let pool = SqlitePoolOptions::new().connect_with(opts).await.unwrap();
            sqlx::query(
                "CREATE TABLE accounts (id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT NOT NULL UNIQUE);",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "CREATE TABLE sessions (token TEXT PRIMARY KEY, account_id INTEGER NOT NULL);",
            )
            .execute(&pool)
            .await
            .unwrap();
        }

        let db = SqliteStorage::open(&path).await.expect("upgrade legacy db");
        let (guest_id, token) = db.create_guest_session().await.expect("guest works");
        assert_eq!(db.validate_session(&token).await.unwrap(), Some(guest_id));

        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
