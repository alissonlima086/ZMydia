use std::{env, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{error::AppError, version::app_version};

pub const INSTALLATION_SCHEMA: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallationMethod {
    WindowsNsis,
    LinuxUser,
    Debian,
    Nix,
    Portable,
    Development,
}

impl InstallationMethod {
    pub fn can_self_update(self) -> bool {
        matches!(self, Self::WindowsNsis | Self::LinuxUser)
    }
}

impl FromStr for InstallationMethod {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "windows-nsis" => Ok(Self::WindowsNsis),
            "linux-user" => Ok(Self::LinuxUser),
            "debian" => Ok(Self::Debian),
            "nix" => Ok(Self::Nix),
            "portable" => Ok(Self::Portable),
            "development" => Ok(Self::Development),
            value => Err(AppError::Config(format!(
                "método de instalação desconhecido: {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InstallationInfo {
    pub schema: u32,
    pub method: InstallationMethod,
    pub version: String,
}

impl InstallationInfo {
    pub fn current(method: InstallationMethod) -> Self {
        Self {
            schema: INSTALLATION_SCHEMA,
            method,
            version: app_version().to_owned(),
        }
    }
}

pub fn detect() -> Result<InstallationInfo, AppError> {
    if let Some(method) = env::var_os("MYMEDIA_INSTALLATION_METHOD") {
        let method = method.to_string_lossy().parse()?;
        return Ok(InstallationInfo::current(method));
    }

    let executable = env::current_exe()?;
    if let Some(root) = executable.parent().and_then(Path::parent) {
        let metadata = root.join("INSTALLATION.json");
        if metadata.is_file() {
            return read_metadata(&metadata);
        }
        if root.starts_with("/nix/store") {
            return Ok(InstallationInfo::current(InstallationMethod::Nix));
        }
        if root == Path::new("/usr/lib/zmydia") {
            return Ok(InstallationInfo::current(InstallationMethod::Debian));
        }
    }

    Ok(InstallationInfo::current(if cfg!(debug_assertions) {
        InstallationMethod::Development
    } else {
        InstallationMethod::Portable
    }))
}

fn read_metadata(path: &Path) -> Result<InstallationInfo, AppError> {
    let contents = std::fs::read_to_string(path)?;
    let info: InstallationInfo = serde_json::from_str(&contents)
        .map_err(|error| AppError::Config(format!("INSTALLATION.json inválido: {error}")))?;
    if info.schema != INSTALLATION_SCHEMA {
        return Err(AppError::Config(format!(
            "schema de instalação não suportado: {}",
            info.schema
        )));
    }
    semver::Version::parse(&info.version).map_err(|error| {
        AppError::Config(format!("versão inválida em INSTALLATION.json: {error}"))
    })?;
    if info.version != app_version() {
        return Err(AppError::Config(format!(
            "INSTALLATION.json informa {}, mas o binário informa {}",
            info.version,
            app_version()
        )));
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_installation_methods_have_stable_names() {
        for (name, method, self_update) in [
            ("windows-nsis", InstallationMethod::WindowsNsis, true),
            ("linux-user", InstallationMethod::LinuxUser, true),
            ("debian", InstallationMethod::Debian, false),
            ("nix", InstallationMethod::Nix, false),
            ("portable", InstallationMethod::Portable, false),
            ("development", InstallationMethod::Development, false),
        ] {
            assert_eq!(name.parse::<InstallationMethod>().unwrap(), method);
            assert_eq!(method.can_self_update(), self_update);
            assert_eq!(
                serde_json::to_string(&method).unwrap(),
                format!("\"{name}\"")
            );
        }
    }

    #[test]
    fn unknown_installation_method_is_rejected() {
        assert!("automatic".parse::<InstallationMethod>().is_err());
    }
}
