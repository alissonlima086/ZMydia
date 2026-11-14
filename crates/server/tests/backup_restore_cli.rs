use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;

fn test_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "mymedia-backup-restore-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ))
}

fn command(binary: &Path, data_dir: &Path, frontend_dir: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .env_remove("DATABASE_URL")
        .env("MYMEDIA_PORT", "0")
        .env("MYMEDIA_DATA_DIR", data_dir)
        .env("MYMEDIA_CACHE_DIR", data_dir.join("cache"))
        .env("MYMEDIA_FRONTEND_DIR", frontend_dir)
        .env("MYMEDIA_EXTENSION_DIR", data_dir.join("extension"))
        .env("MYMEDIA_FFMPEG_BIN", "true")
        .env("MYMEDIA_FFPROBE_BIN", "true")
        .env("MYMEDIA_YTDLP_BIN", "true")
        .env("MYMEDIA_CURL_BIN", "true");
    command
}

async fn marker(database_url: &str) -> String {
    let pool = SqlitePool::connect(database_url)
        .await
        .expect("open test database");
    let value =
        sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key='restore-test'")
            .fetch_one(&pool)
            .await
            .expect("read marker");
    pool.close().await;
    value
}

#[tokio::test]
async fn cli_backup_and_restore_preserve_database_key_and_rollback_copy() {
    let root = test_dir();
    let data_dir = root.join("data");
    let frontend_dir = root.join("frontend");
    std::fs::create_dir_all(&frontend_dir).expect("create frontend fixture");
    std::fs::write(frontend_dir.join("index.html"), "<!doctype html>")
        .expect("write frontend fixture");

    let binary = Path::new(env!("CARGO_BIN_EXE_mymedia-server"));
    let mut server = command(binary, &data_dir, &frontend_dir)
        .arg("serve")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start server");
    let database = data_dir.join("mymedia.db");
    let key = data_dir.join(".mymedia_master.key");
    for _ in 0..100 {
        if database.is_file() && key.is_file() {
            break;
        }
        assert!(server.try_wait().expect("poll server").is_none());
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(database.is_file(), "server did not create database");
    assert!(key.is_file(), "server did not create master key");
    server.kill().expect("stop server");
    server.wait().expect("reap server");

    let database_url = format!("sqlite://{}", database.display());
    let pool = SqlitePool::connect(&database_url)
        .await
        .expect("open database");
    sqlx::query("INSERT INTO settings(key,value_json) VALUES('restore-test','1')")
        .execute(&pool)
        .await
        .expect("insert original marker");
    pool.close().await;
    let original_key = std::fs::read(&key).expect("read original key");

    let backup_output = command(binary, &data_dir, &frontend_dir)
        .arg("backup")
        .output()
        .expect("run backup");
    assert!(backup_output.status.success(), "backup command failed");
    let backup = PathBuf::from(
        String::from_utf8(backup_output.stdout)
            .expect("backup output utf8")
            .lines()
            .last()
            .expect("backup path output"),
    );
    assert!(backup.is_file());
    assert!(backup.with_extension("key").is_file());

    let pool = SqlitePool::connect(&database_url)
        .await
        .expect("reopen database");
    sqlx::query("UPDATE settings SET value_json='2' WHERE key='restore-test'")
        .execute(&pool)
        .await
        .expect("change marker after backup");
    pool.close().await;

    let restore = command(binary, &data_dir, &frontend_dir)
        .arg("restore")
        .arg(&backup)
        .output()
        .expect("run restore");
    assert!(restore.status.success(), "restore command failed");
    assert_eq!(marker(&database_url).await, "1");
    assert_eq!(
        std::fs::read(&key).expect("read restored key"),
        original_key
    );

    let rollback = database.with_extension("db.pre-restore");
    assert!(rollback.is_file());
    assert_eq!(
        marker(&format!("sqlite://{}", rollback.display())).await,
        "2"
    );

    std::fs::remove_dir_all(root).expect("remove isolated fixture");
}
