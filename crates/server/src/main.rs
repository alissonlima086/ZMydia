use std::path::PathBuf;

use mymedia_server::{config::Config, db, operations, service, web};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if service::handle(&arguments)? {
        return Ok(());
    }
    let config = Config::from_env()?;
    if handle_command(&config, &arguments).await? {
        return Ok(());
    }
    let _instance_lock = operations::acquire_instance_lock(&config)?;
    db::backup_if_present(&config.database_url, &config.key_path, &config.backup_dir).await?;
    let crypto = mymedia_server::crypto::AppCrypto::init_or_generate(&config.key_path)?;
    let pool = db::connect(&config.database_url).await?;
    let recovery = mymedia_server::filesystem::recover_incomplete_operations(&pool).await?;
    if recovery.inspected > 0 {
        tracing::info!(
            inspected = recovery.inspected,
            completed = recovery.completed,
            failed = recovery.failed,
            "journal de filesystem reconciliado no startup"
        );
    }
    mymedia_server::music::migrate_listenbrainz_token_if_needed(&pool, &crypto).await?;
    let diagnostics = operations::report(&config, Some(&pool)).await;
    for item in diagnostics
        .capabilities
        .iter()
        .filter(|item| item.status != "ok")
    {
        if item.required {
            tracing::error!(capability = %item.name, detail = %item.detail, "required capability unavailable");
        } else {
            tracing::warn!(capability = %item.name, detail = %item.detail, "optional capability unavailable");
        }
    }
    let (app, runtime) = web::router_with_runtime(
        pool,
        crypto,
        &config.frontend_dir,
        &config.extension_dir,
        &config.cache_dir,
        diagnostics,
        config.security.clone(),
    );
    let listener = TcpListener::bind(config.address).await?;

    tracing::info!(address = %config.address, "ZMydia listening on loopback");
    let shutdown_runtime = runtime.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_runtime.shutdown().await;
        })
        .await?;
    Ok(())
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "mymedia_server=info,tower_http=info".into());
    if std::env::var("MYMEDIA_LOG_FORMAT").is_ok_and(|value| value.eq_ignore_ascii_case("json")) {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }
}

async fn handle_command(
    config: &Config,
    arguments: &[String],
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Ok(false);
    };
    match command {
        "serve" => Ok(false),
        "diagnose" => {
            let pool = db::connect(&config.database_url).await.ok();
            let report = operations::report(config, pool.as_ref()).await;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if let Some(pool) = pool {
                pool.close().await;
            }
            if !report.ready() {
                std::process::exit(2);
            }
            Ok(true)
        }
        "backup" => {
            let backup =
                db::backup_if_present(&config.database_url, &config.key_path, &config.backup_dir)
                    .await?;
            match backup {
                Some(path) => println!("{}", path.display()),
                None => println!("no existing database to back up"),
            }
            Ok(true)
        }
        "account-recover" => {
            let username = arguments.get(1).ok_or(
                "usage: zmydia-server account-recover <username>; set MYMEDIA_RECOVERY_PASSWORD",
            )?;
            let password = std::env::var("MYMEDIA_RECOVERY_PASSWORD")
                .map_err(|_| "MYMEDIA_RECOVERY_PASSWORD não definida")?;
            operations::recover_account(config, username, &password).await?;
            println!("password reset; sessions and tokens were revoked");
            Ok(true)
        }
        "restore" => {
            let backup = PathBuf::from(
                arguments
                    .get(1)
                    .ok_or("usage: zmydia-server restore <backup.db>")?,
            );
            restore_backup(config, &backup).await?;
            println!("backup restored from {}", backup.display());
            Ok(true)
        }
        "--help" | "-h" | "help" => {
            println!(
                "ZMydia {}\n\nUSAGE:\n  zmydia-server [serve]\n  zmydia-server diagnose\n  zmydia-server backup\n  zmydia-server restore <backup.db>\n  zmydia-server service <install|status|restart|logs|uninstall>\n  MYMEDIA_RECOVERY_PASSWORD=... zmydia-server account-recover <username>",
                env!("CARGO_PKG_VERSION")
            );
            Ok(true)
        }
        value => Err(format!("unknown command: {value}").into()),
    }
}

async fn restore_backup(
    config: &Config,
    backup: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let _instance_lock = operations::acquire_instance_lock(config)?;
    operations::validate_backup(backup).await?;
    let database = operations::database_path(config)?;
    let rollback = database.with_extension("db.pre-restore");
    if database.exists() {
        std::fs::copy(&database, &rollback)?;
    }
    std::fs::copy(backup, &database)?;
    let backup_key = backup.with_extension("key");
    if backup_key.is_file() {
        std::fs::copy(backup_key, &config.key_path)?;
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
