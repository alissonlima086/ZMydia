use std::{
    env,
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    Connection, SqlitePool,
    sqlite::{
        SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePoolOptions,
        SqliteSynchronous,
    },
};

use crate::error::AppError;

static SQLITE_BUSY_TOTAL: AtomicU64 = AtomicU64::new(0);

pub fn observe_sqlite_error(error: &sqlx::Error) {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("database is locked") || message.contains("database is busy") {
        SQLITE_BUSY_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn sqlite_busy_total() -> u64 {
    SQLITE_BUSY_TOTAL.load(Ordering::Relaxed)
}

pub async fn connect(database_url: &str) -> Result<SqlitePool, AppError> {
    let max_connections = env::var("MYMEDIA_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(8)
        .clamp(2, 32);
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .pragma("cache_size", "-64000") // 64MB de cache de páginas em memória RAM
        .pragma("mmap_size", "268435456") // 256MB de memory-mapped I/O (leitura direta via kernel sem cópias em userspace)
        .pragma("temp_store", "memory"); // Tabelas temporárias e operações de ordenação diretamente em RAM

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    let integrity = sqlx::query_scalar::<_, String>("PRAGMA quick_check")
        .fetch_one(&pool)
        .await?;
    if integrity != "ok" {
        return Err(AppError::Config(format!(
            "verificação de integridade do SQLite falhou: {integrity}"
        )));
    }
    init_auth_security(&pool).await?;
    Ok(pool)
}

/// Creates a consistent SQLite snapshot before migrations touch an existing database.
/// Returns `None` for memory databases and databases that do not exist yet.
pub async fn backup_if_present(
    database_url: &str,
    key_path: &Path,
    backup_dir: &Path,
) -> Result<Option<PathBuf>, AppError> {
    let Some(raw_path) = database_url.strip_prefix("sqlite://") else {
        return Ok(None);
    };
    if raw_path == ":memory:" {
        return Ok(None);
    }

    let database_path = PathBuf::from(raw_path);
    if !database_path.is_file() {
        return Ok(None);
    }
    std::fs::create_dir_all(backup_dir)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stem = format!("mymedia-{timestamp}-{}", std::process::id());
    let backup_path = backup_dir.join(format!("{stem}.db"));

    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(30));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query("PRAGMA wal_checkpoint(FULL)")
        .execute(&mut connection)
        .await?;
    sqlx::query("VACUUM INTO ?")
        .bind(backup_path.to_string_lossy().as_ref())
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    if key_path.is_file() {
        let key_backup = backup_dir.join(format!("{stem}.key"));
        std::fs::copy(key_path, &key_backup)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_backup, std::fs::Permissions::from_mode(0o600))?;
        }
    }

    tracing::info!(path = %backup_path.display(), "pre-migration backup created");
    prune_backups(backup_dir, 14)?;
    Ok(Some(backup_path))
}

fn prune_backups(backup_dir: &Path, keep: usize) -> Result<(), AppError> {
    let mut databases = std::fs::read_dir(backup_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("db"))
        .collect::<Vec<_>>();
    databases.sort_by_key(|path| {
        std::cmp::Reverse(
            std::fs::metadata(path)
                .and_then(|value| value.modified())
                .ok(),
        )
    });
    for database in databases.into_iter().skip(keep) {
        let key = database.with_extension("key");
        std::fs::remove_file(&database)?;
        if key.is_file() {
            std::fs::remove_file(key)?;
        }
        tracing::info!(path = %database.display(), "expired backup removed by retention policy");
    }
    Ok(())
}

pub async fn init_auth_security(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM sessions WHERE expires_at <= datetime('now')")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM extension_pairing_challenges WHERE expires_at <= datetime('now')")
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn backup_can_be_restored_and_passes_integrity_check() {
        let temp_dir = std::env::temp_dir().join(format!(
            "mymedia_backup_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let backup_dir = temp_dir.join("backups");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let database_path = temp_dir.join("original.db");
        let database_url = format!("sqlite://{}", database_path.display());
        let pool = connect(&database_url).await.unwrap();
        sqlx::query("INSERT INTO settings (key, value_json) VALUES ('backup-test', '42')")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let key_path = temp_dir.join("master.key");
        std::fs::write(&key_path, [7_u8; 32]).unwrap();
        let backup_path = backup_if_present(&database_url, &key_path, &backup_dir)
            .await
            .unwrap()
            .expect("existing database should be backed up");

        let restored_url = format!("sqlite://{}", backup_path.display());
        let options = SqliteConnectOptions::from_str(&restored_url)
            .unwrap()
            .create_if_missing(false);
        let mut restored = SqliteConnection::connect_with(&options).await.unwrap();
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value_json FROM settings WHERE key = 'backup-test'",
        )
        .fetch_one(&mut restored)
        .await
        .unwrap();
        let integrity = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut restored)
            .await
            .unwrap();
        assert_eq!(value, "42");
        assert_eq!(integrity, "ok");
        assert!(
            backup_dir
                .join(format!(
                    "{}.key",
                    backup_path.file_stem().unwrap().to_string_lossy()
                ))
                .is_file()
        );
        restored.close().await.unwrap();
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[tokio::test]
    async fn migrations_are_complete_and_idempotent_on_a_clean_database() {
        let temp_dir = std::env::temp_dir().join(format!(
            "mymedia_migrations_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let database_path = temp_dir.join("fresh.db");
        let database_url = format!("sqlite://{}", database_path.display());

        let pool = connect(&database_url).await.unwrap();
        let first_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(first_count > 0);
        pool.close().await;

        let reopened = connect(&database_url).await.unwrap();
        let second_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&reopened)
            .await
            .unwrap();
        assert_eq!(second_count, first_count);
        reopened.close().await;
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[tokio::test]
    async fn partial_database_failure_rolls_back_the_whole_transaction() {
        let pool = connect("sqlite::memory:").await.unwrap();
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("INSERT INTO settings (key, value_json) VALUES ('partial-test', '1')")
            .execute(&mut *transaction)
            .await
            .unwrap();
        let failure = sqlx::query(
            "INSERT INTO settings (key, value_json) VALUES ('partial-test', 'duplicate')",
        )
        .execute(&mut *transaction)
        .await;
        assert!(failure.is_err());
        transaction.rollback().await.unwrap();

        let persisted = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM settings WHERE key = 'partial-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(persisted, 0);
    }
}
