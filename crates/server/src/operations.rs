use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use fs2::FileExt;
use serde::Serialize;
use sqlx::{Connection, SqliteConnection, SqlitePool};
use tokio::process::Command;

use crate::{
    config::{Config, external_command},
    error::AppError,
};

const MIN_FREE_MIB: u64 = 512;

#[derive(Clone, Debug, Serialize)]
pub struct Capability {
    pub name: String,
    pub required: bool,
    pub status: &'static str,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticReport {
    pub status: &'static str,
    pub version: &'static str,
    pub capabilities: Vec<Capability>,
}

impl DiagnosticReport {
    pub fn ready(&self) -> bool {
        self.capabilities
            .iter()
            .all(|item| !item.required || item.status == "ok")
    }
}

pub async fn report(config: &Config, pool: Option<&SqlitePool>) -> DiagnosticReport {
    let mut capabilities = vec![
        path_check(
            "frontend",
            &config.frontend_dir.join("index.html"),
            true,
            false,
        ),
        path_check("data_dir", &config.data_dir, true, true),
        path_check("cache", &config.cache_dir, true, true),
        path_check("key", &config.key_path, true, false),
        disk_check(&config.data_dir),
    ];

    if let Some(pool) = pool {
        let database = match sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(pool)
            .await
        {
            Ok(_) => capability(
                "database",
                true,
                true,
                "SQLite respondeu e as migrações foram aplicadas",
            ),
            Err(error) => capability(
                "database",
                true,
                false,
                format!("SQLite indisponível: {error}"),
            ),
        };
        capabilities.push(database);
        capabilities.extend(source_capabilities(pool).await);
    } else {
        capabilities.push(capability(
            "database",
            true,
            false,
            "conexão ainda não inicializada",
        ));
    }

    capabilities
        .push(command_check("ffmpeg", "MYMEDIA_FFMPEG_BIN", "ffmpeg", "-version", true).await);
    capabilities.push(
        command_check(
            "ffprobe",
            "MYMEDIA_FFPROBE_BIN",
            "ffprobe",
            "-version",
            true,
        )
        .await,
    );
    capabilities
        .push(command_check("yt-dlp", "MYMEDIA_YTDLP_BIN", "yt-dlp", "--version", false).await);
    capabilities.push(command_check("curl", "MYMEDIA_CURL_BIN", "curl", "--version", false).await);

    let ready = capabilities
        .iter()
        .all(|item| !item.required || item.status == "ok");
    DiagnosticReport {
        status: if ready { "ready" } else { "not_ready" },
        version: env!("CARGO_PKG_VERSION"),
        capabilities,
    }
}

pub async fn source_capabilities(pool: &SqlitePool) -> Vec<Capability> {
    let Ok(sources) = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, path, mode FROM media_sources WHERE enabled=1 AND removed_at IS NULL",
    )
    .fetch_all(pool)
    .await
    else {
        return vec![capability(
            "sources",
            true,
            false,
            "não foi possível validar as fontes configuradas",
        )];
    };
    sources
        .into_iter()
        .map(|(id, path, mode)| {
            path_check(
                &format!("source:{id}"),
                Path::new(&path),
                true,
                mode == "managed",
            )
        })
        .collect()
}

fn capability(name: &str, required: bool, ok: bool, detail: impl Into<String>) -> Capability {
    Capability {
        name: name.to_owned(),
        required,
        status: if ok { "ok" } else { "error" },
        detail: detail.into(),
    }
}

fn path_check(name: &str, path: &Path, required: bool, writable: bool) -> Capability {
    let exists = path.exists();
    let writable_ok = !writable || write_probe(path);
    capability(
        name,
        required,
        exists && writable_ok,
        if !exists {
            format!("não encontrado: {}", path.display())
        } else if !writable_ok {
            format!("sem permissão de escrita: {}", path.display())
        } else {
            path.display().to_string()
        },
    )
}

fn write_probe(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let probe = path.join(format!(".mymedia-write-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => std::fs::remove_file(probe).is_ok(),
        Err(_) => false,
    }
}

fn disk_check(path: &Path) -> Capability {
    match fs2::available_space(path) {
        Ok(bytes) => {
            let free_mib = bytes / 1024 / 1024;
            capability(
                "disk",
                true,
                free_mib >= MIN_FREE_MIB,
                format!("{free_mib} MiB livres (mínimo {MIN_FREE_MIB} MiB)"),
            )
        }
        Err(error) => capability(
            "disk",
            true,
            false,
            format!("não foi possível consultar espaço: {error}"),
        ),
    }
}

async fn command_check(
    name: &str,
    variable: &str,
    fallback: &str,
    version_arg: &str,
    required: bool,
) -> Capability {
    let binary = external_command(variable, fallback);
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(&binary)
            .arg(version_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;
    match result {
        Ok(Ok(status)) if status.success() => {
            capability(name, required, true, binary.to_string_lossy())
        }
        Ok(Ok(status)) => capability(
            name,
            required,
            false,
            format!("{} encerrou com {status}", binary.to_string_lossy()),
        ),
        Ok(Err(error)) => capability(
            name,
            required,
            false,
            format!("{}: {error}", binary.to_string_lossy()),
        ),
        Err(_) => capability(
            name,
            required,
            false,
            format!("{} excedeu 5 segundos", binary.to_string_lossy()),
        ),
    }
}

pub async fn recover_account(
    config: &Config,
    username: &str,
    password: &str,
) -> Result<(), AppError> {
    if password.chars().count() < 8 {
        return Err(AppError::Config(
            "a nova senha deve ter ao menos 8 caracteres".to_owned(),
        ));
    }
    let pool = crate::db::connect(&config.database_url).await?;
    let hash = crate::crypto::hash_password(password)?;
    let mut transaction = pool.begin().await?;
    let result = sqlx::query("UPDATE users SET password_hash=?,must_change_password=0,password_changed_at=datetime('now'),updated_at=datetime('now') WHERE username=? COLLATE NOCASE")
        .bind(hash).bind(username).execute(&mut *transaction).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Config(format!(
            "usuário não encontrado: {username}"
        )));
    }
    sqlx::query(
        "DELETE FROM sessions WHERE user_id=(SELECT id FROM users WHERE username=? COLLATE NOCASE)",
    )
    .bind(username)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE api_tokens SET revoked_at=datetime('now') WHERE user_id=(SELECT id FROM users WHERE username=? COLLATE NOCASE) AND revoked_at IS NULL").bind(username).execute(&mut *transaction).await?;
    transaction.commit().await?;
    pool.close().await;
    Ok(())
}

pub async fn validate_backup(path: &Path) -> Result<(), AppError> {
    if !path.is_file() {
        return Err(AppError::Config(format!(
            "backup não encontrado: {}",
            path.display()
        )));
    }
    let url = format!("sqlite://{}", path.display());
    let mut connection = SqliteConnection::connect(&url).await?;
    let integrity = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    if integrity != "ok" {
        return Err(AppError::Config(format!("backup inválido: {integrity}")));
    }
    Ok(())
}

pub fn database_path(config: &Config) -> Result<PathBuf, AppError> {
    config
        .database_url
        .strip_prefix("sqlite://")
        .map(PathBuf::from)
        .filter(|path| path.as_os_str() != ":memory:")
        .ok_or_else(|| {
            AppError::Config("a operação exige DATABASE_URL sqlite em arquivo".to_owned())
        })
}

pub fn acquire_instance_lock(config: &Config) -> Result<std::fs::File, AppError> {
    let path = config.data_dir.join(".mymedia.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;
    file.try_lock_exclusive().map_err(|error| {
        AppError::Config(format!(
            "outra instância usa o diretório de dados {}: {error}",
            config.data_dir.display()
        ))
    })?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SecurityConfig;

    #[tokio::test]
    async fn local_account_recovery_changes_password_and_revokes_access() {
        let root = std::env::temp_dir().join(format!(
            "mymedia-account-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("mymedia.db");
        let config = Config {
            address: "127.0.0.1:0".parse().unwrap(),
            data_dir: root.clone(),
            database_url: format!("sqlite://{}", database.display()),
            frontend_dir: root.join("frontend"),
            extension_dir: root.join("extension"),
            cache_dir: root.join("cache"),
            key_path: root.join("master.key"),
            backup_dir: root.join("backups"),
            security: SecurityConfig::default(),
        };

        let pool = crate::db::connect(&config.database_url).await.unwrap();
        let old_hash = crate::crypto::hash_password("senha-antiga").unwrap();
        let user_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users(username,password_hash) VALUES('owner',?) RETURNING id",
        )
        .bind(old_hash)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES('sessao',?,datetime('now','+1 day'))")
            .bind(user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO api_tokens(user_id,token_hash,token_hint,name,scopes) VALUES(?,'token','hint','teste','downloads:write')")
            .bind(user_id).execute(&pool).await.unwrap();
        pool.close().await;

        recover_account(&config, "OWNER", "senha-nova-123")
            .await
            .unwrap();

        let pool = crate::db::connect(&config.database_url).await.unwrap();
        let password_hash =
            sqlx::query_scalar::<_, String>("SELECT password_hash FROM users WHERE id=?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(crate::crypto::verify_password(
            "senha-nova-123",
            &password_hash
        ));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE user_id=?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM api_tokens WHERE user_id=? AND revoked_at IS NULL",
            )
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        pool.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}
