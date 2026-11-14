use std::{env, ffi::OsString, net::SocketAddr, path::PathBuf};

use crate::error::AppError;

const DEFAULT_PORT: u16 = 4173;

pub fn external_command(variable: &str, fallback: &str) -> OsString {
    if let Some(value) = env::var_os(variable).filter(|value| !value.is_empty()) {
        return value;
    }

    if let Ok(executable) = env::current_exe()
        && let Some(prefix) = executable.parent().and_then(|path| path.parent())
    {
        let bundled = prefix
            .join("lib/mymedia/bin")
            .join(format!("{fallback}{}", env::consts::EXE_SUFFIX));
        if bundled.is_file() {
            return bundled.into_os_string();
        }
    }

    fallback.into()
}

pub fn external_base_url(variable: &str, fallback: &str) -> String {
    env::var(variable)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .unwrap_or_else(|| fallback.trim_end_matches('/').to_owned())
}

#[derive(Clone, Debug)]
pub struct Config {
    pub address: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub frontend_dir: PathBuf,
    pub extension_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub key_path: PathBuf,
    pub backup_dir: PathBuf,
    pub security: SecurityConfig,
}

#[derive(Clone, Debug)]
pub struct SecurityConfig {
    pub trust_forwarded_for: bool,
    pub cookie_secure: bool,
    pub cookie_same_site: String,
    pub cookie_domain: Option<String>,
    pub extension_origins: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            trust_forwarded_for: false,
            cookie_secure: false,
            cookie_same_site: "Lax".to_string(),
            cookie_domain: None,
            extension_origins: Vec::new(),
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        let port = env::var("MYMEDIA_PORT")
            .ok()
            .map(|value| value.parse::<u16>())
            .transpose()
            .map_err(|error| AppError::Config(format!("MYMEDIA_PORT inválida: {error}")))?
            .unwrap_or(DEFAULT_PORT);

        let data_dir = env::var_os("MYMEDIA_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir);
        std::fs::create_dir_all(&data_dir)?;

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| format!("sqlite://{}", data_dir.join("mymedia.db").display()));
        ensure_sqlite_parent(&database_url)?;
        let frontend_dir = env::var_os("MYMEDIA_FRONTEND_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| installed_asset_dir("frontend", "frontend/dist"));
        let extension_dir = env::var_os("MYMEDIA_EXTENSION_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| installed_asset_dir("extension", "extension"));
        let cache_dir = env::var_os("MYMEDIA_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_cache_dir)
            .join("thumbnails");
        std::fs::create_dir_all(&cache_dir)?;

        let key_path = env::var_os("MYMEDIA_KEY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if let Some(db_file) = database_url.strip_prefix("sqlite://") {
                    let db_path = PathBuf::from(db_file);
                    if let Some(parent) = db_path.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        return parent.join(".mymedia_master.key");
                    }
                }
                data_dir.join(".mymedia_master.key")
            });
        let backup_dir = env::var_os("MYMEDIA_BACKUP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("backups"));
        std::fs::create_dir_all(&backup_dir)?;

        let mut security = SecurityConfig {
            trust_forwarded_for: env_bool("MYMEDIA_TRUST_FORWARDED_FOR", false),
            cookie_secure: env_bool("MYMEDIA_COOKIE_SECURE", false),
            cookie_same_site: match env::var("MYMEDIA_COOKIE_SAME_SITE")
                .unwrap_or_else(|_| "Lax".to_string())
                .to_ascii_lowercase()
                .as_str()
            {
                "strict" => "Strict".to_string(),
                "none" => "None".to_string(),
                _ => "Lax".to_string(),
            },
            cookie_domain: env::var("MYMEDIA_COOKIE_DOMAIN")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            extension_origins: env::var("MYMEDIA_EXTENSION_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|value| value.starts_with("moz-extension://") && value.len() <= 128)
                .map(ToOwned::to_owned)
                .collect(),
        };
        if security.cookie_same_site == "None" {
            security.cookie_secure = true;
        }

        Ok(Self {
            address: SocketAddr::from(([127, 0, 0, 1], port)),
            data_dir,
            database_url,
            frontend_dir,
            extension_dir,
            cache_dir,
            key_path,
            backup_dir,
            security,
        })
    }
}

/// Installed packages keep the binary in `bin/` and immutable assets in
/// `share/mymedia/`. Development keeps using paths relative to the checkout.
fn installed_asset_dir(name: &str, development_fallback: &str) -> PathBuf {
    if let Ok(executable) = env::current_exe()
        && let Some(prefix) = executable.parent().and_then(|path| path.parent())
    {
        let candidate = prefix.join("lib/mymedia").join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(development_fallback)
}

fn env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(default)
}

fn default_cache_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Local"))
            })
            .unwrap_or_else(|| PathBuf::from("data"));
        return prefer_legacy_brand_dir(base, "ZMydia", "MyMedia").join("cache");
    }

    #[cfg(not(windows))]
    {
        let base = env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .unwrap_or_else(|| PathBuf::from(".cache"));
        prefer_legacy_brand_dir(base, "zmydia", "mymedia")
    }
}

fn ensure_sqlite_parent(database_url: &str) -> Result<(), AppError> {
    let Some(path) = database_url.strip_prefix("sqlite://") else {
        return Ok(());
    };
    if path == ":memory:" {
        return Ok(());
    }
    if let Some(parent) = PathBuf::from(path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn default_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Local"))
            })
            .unwrap_or_else(|| PathBuf::from("data"));
        return prefer_legacy_brand_dir(base, "ZMydia", "MyMedia");
    }

    #[cfg(not(windows))]
    {
        let base = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("data"));
        prefer_legacy_brand_dir(base, "zmydia", "mymedia")
    }
}

fn prefer_legacy_brand_dir(base: PathBuf, branded_name: &str, legacy_name: &str) -> PathBuf {
    let legacy = base.join(legacy_name);
    if legacy.exists() {
        legacy
    } else {
        base.join(branded_name)
    }
}
