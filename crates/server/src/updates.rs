use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::signature;
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{io::AsyncWriteExt, sync::Mutex};

use crate::{installation::InstallationMethod, version::app_version};

pub const UPDATE_MANIFEST_SCHEMA: u32 = 1;
pub const UPDATE_CHANNEL: &str = "stable";
const RELEASE_BASE_URL: &str = "https://github.com/alissonlima086/ZMydia/releases/download";
const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/alissonlima086/ZMydia/releases/latest/download/zmydia-update.json";
const MANIFEST_LIMIT: usize = 1024 * 1024;
const CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateManifest {
    pub schema: u32,
    pub channel: String,
    pub version: String,
    pub published_at: String,
    pub min_updater_version: String,
    pub notes_url: String,
    pub assets: BTreeMap<String, UpdateAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateAsset {
    pub kind: UpdateAssetKind,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateAssetKind {
    Nsis,
    Tar,
    Deb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateTarget {
    WindowsX86_64,
    LinuxX86_64User,
    LinuxX86_64Deb,
}

impl UpdateTarget {
    pub fn for_installation(
        method: InstallationMethod,
        os: &str,
        architecture: &str,
    ) -> Result<Self, UpdateManifestError> {
        match (method, os, architecture) {
            (InstallationMethod::WindowsNsis, "windows", "x86_64") => Ok(Self::WindowsX86_64),
            (InstallationMethod::LinuxUser, "linux", "x86_64") => Ok(Self::LinuxX86_64User),
            (InstallationMethod::Debian, "linux", "x86_64") => Ok(Self::LinuxX86_64Deb),
            (InstallationMethod::Nix, "linux", "x86_64") => Ok(Self::LinuxX86_64User),
            (InstallationMethod::Portable, "linux", "x86_64") => Ok(Self::LinuxX86_64User),
            (InstallationMethod::Portable, "windows", "x86_64") => Ok(Self::WindowsX86_64),
            _ => Err(UpdateManifestError::UnsupportedTarget),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "windows-x86_64",
            Self::LinuxX86_64User => "linux-x86_64-user",
            Self::LinuxX86_64Deb => "linux-x86_64-deb",
        }
    }

    fn expected_kind(self) -> UpdateAssetKind {
        match self {
            Self::WindowsX86_64 => UpdateAssetKind::Nsis,
            Self::LinuxX86_64User => UpdateAssetKind::Tar,
            Self::LinuxX86_64Deb => UpdateAssetKind::Deb,
        }
    }

    fn expected_filename(self, version: &Version) -> String {
        match self {
            Self::WindowsX86_64 => {
                format!("zmydia-{version}-windows-x86_64-installer.exe")
            }
            Self::LinuxX86_64User => format!("zmydia-{version}-linux-x86_64.tar.gz"),
            Self::LinuxX86_64Deb => format!("zmydia_{version}_amd64.deb"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerifiedUpdate {
    pub version: Version,
    pub asset: UpdateAsset,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum UpdateManifestError {
    #[error("chave pública de atualização inválida")]
    InvalidPublicKey,
    #[error("assinatura do manifesto inválida")]
    InvalidSignature,
    #[error("manifesto de atualização inválido")]
    InvalidManifest,
    #[error("schema de atualização não suportado")]
    UnsupportedSchema,
    #[error("canal de atualização não suportado")]
    UnsupportedChannel,
    #[error("versão inválida no manifesto")]
    InvalidVersion,
    #[error("a versão do manifesto não é mais nova")]
    NotNewer,
    #[error("a versão atual não suporta este manifesto")]
    UpdaterTooOld,
    #[error("a instalação atual não possui atualização autoaplicável")]
    UnsupportedTarget,
    #[error("asset da plataforma não encontrado")]
    MissingAsset,
    #[error("asset da plataforma é inválido")]
    InvalidAsset,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Downloaded,
    Applying,
    Installed,
    RolledBack,
    Unsupported,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateStatus {
    pub phase: UpdatePhase,
    pub current_version: String,
    pub available_version: Option<String>,
    pub notes_url: Option<String>,
    pub can_self_update: bool,
    #[serde(default)]
    pub can_download: bool,
    #[serde(default = "default_installation_method")]
    pub installation_method: InstallationMethod,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f64,
    pub last_checked_at: Option<u64>,
    pub error_message: Option<String>,
}

fn default_installation_method() -> InstallationMethod {
    InstallationMethod::Portable
}

impl UpdateStatus {
    fn initial(
        can_self_update: bool,
        can_download: bool,
        installation_method: InstallationMethod,
    ) -> Self {
        Self {
            phase: if can_self_update {
                UpdatePhase::Idle
            } else {
                UpdatePhase::Unsupported
            },
            current_version: app_version().to_owned(),
            available_version: None,
            notes_url: None,
            can_self_update,
            can_download,
            installation_method,
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: 0.0,
            last_checked_at: None,
            error_message: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentUpdateState {
    status: UpdateStatus,
    verified: Option<VerifiedUpdate>,
    downloaded_path: Option<PathBuf>,
}

#[derive(Clone)]
pub struct UpdateService {
    inner: Arc<UpdateServiceInner>,
}

struct UpdateServiceInner {
    client: reqwest::Client,
    download_client: reqwest::Client,
    manifest_url: String,
    public_key: Option<String>,
    target: Option<UpdateTarget>,
    installation_method: InstallationMethod,
    can_download: bool,
    state_path: PathBuf,
    cache_dir: PathBuf,
    state: Mutex<PersistentUpdateState>,
    operation: Arc<Mutex<()>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ManifestCacheMetadata {
    fetched_at: u64,
}

impl UpdateService {
    pub fn start(cache_root: &Path) -> Self {
        let installation = crate::installation::detect().ok();
        let installation_method = installation
            .as_ref()
            .map(|info| info.method)
            .unwrap_or(InstallationMethod::Development);
        let target = installation.as_ref().and_then(|info| {
            UpdateTarget::for_installation(
                info.method,
                std::env::consts::OS,
                std::env::consts::ARCH,
            )
            .ok()
        });
        let can_self_update = installation
            .as_ref()
            .is_some_and(|info| info.method.can_self_update())
            && target.is_some();
        let can_download = target.is_some()
            && !matches!(
                installation_method,
                InstallationMethod::Nix | InstallationMethod::Development
            );
        let cache_dir = cache_root.join("updates");
        let state_path = cache_dir.join("state.json");
        let saved = std::fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistentUpdateState>(&bytes).ok());
        let completed_update = saved.as_ref().is_some_and(|state| {
            state.status.phase == UpdatePhase::Applying
                && state.status.available_version.as_deref() == Some(app_version())
        });
        let rolled_back_update = saved.as_ref().is_some_and(|state| {
            state.status.phase == UpdatePhase::Applying
                && state.status.current_version == app_version()
                && state.status.available_version.as_deref() != Some(app_version())
        });
        let mut persisted = saved
            .filter(|state| {
                state.status.current_version == app_version()
                    || completed_update
                    || rolled_back_update
            })
            .unwrap_or_else(|| PersistentUpdateState {
                status: UpdateStatus::initial(can_self_update, can_download, installation_method),
                verified: None,
                downloaded_path: None,
            });
        if completed_update {
            persisted.status.phase = UpdatePhase::Installed;
            persisted.status.current_version = app_version().to_owned();
            persisted.status.error_message = None;
        } else if rolled_back_update {
            persisted.status.phase = UpdatePhase::RolledBack;
            persisted.status.error_message = Some(
                "A nova versão não ficou pronta; a versão anterior foi restaurada.".to_owned(),
            );
        }
        persisted.status.can_self_update = can_self_update;
        persisted.status.can_download = can_download;
        persisted.status.installation_method = installation_method;
        if target.is_none() {
            persisted.status.phase = UpdatePhase::Unsupported;
            persisted.verified = None;
        } else if persisted.status.phase == UpdatePhase::Downloaded
            && !persisted
                .downloaded_path
                .as_ref()
                .is_some_and(|path| path.is_file())
        {
            persisted.status.phase = if persisted.verified.is_some() {
                UpdatePhase::Available
            } else {
                UpdatePhase::Idle
            };
            persisted.status.downloaded_bytes = 0;
            persisted.status.progress_percent = 0.0;
            persisted.downloaded_path = None;
        } else if matches!(
            persisted.status.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading
        ) {
            persisted.status.phase = UpdatePhase::Idle;
            persisted.status.error_message =
                Some("A operação de atualização foi interrompida; tente novamente.".to_owned());
        }

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("ZMydia/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("update HTTP client must be valid");
        let download_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("ZMydia/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("update download client must be valid");
        Self {
            inner: Arc::new(UpdateServiceInner {
                client,
                download_client,
                manifest_url: std::env::var("MYMEDIA_UPDATE_MANIFEST_URL")
                    .unwrap_or_else(|_| DEFAULT_MANIFEST_URL.to_owned()),
                public_key: embedded_public_key().map(str::to_owned),
                target,
                installation_method,
                can_download,
                state_path,
                cache_dir,
                state: Mutex::new(persisted),
                operation: Arc::new(Mutex::new(())),
            }),
        }
    }

    pub async fn status(&self) -> UpdateStatus {
        self.inner.state.lock().await.status.clone()
    }

    pub async fn check(&self) -> Result<UpdateStatus, String> {
        let _operation = self
            .inner
            .operation
            .try_lock()
            .map_err(|_| "Já existe uma operação de atualização em andamento".to_owned())?;
        let Some(target) = self.inner.target else {
            return Ok(self.status().await);
        };
        let Some(public_key) = self.inner.public_key.as_deref() else {
            return self
                .fail("Esta compilação não contém uma chave pública de atualização")
                .await;
        };
        self.update_state(|state| {
            state.status.phase = UpdatePhase::Checking;
            state.status.error_message = None;
        })
        .await;

        let result = async {
            let (manifest, signature) = self.fetch_manifest_with_cache().await?;
            verify_manifest(&manifest, &signature, public_key, app_version(), target)
                .map_err(|error| error.to_string())
        }
        .await;

        match result {
            Ok(verified) => {
                let version = verified.version.to_string();
                let notes_url =
                    format!("https://github.com/alissonlima086/ZMydia/releases/tag/v{version}");
                let size = verified.asset.size;
                self.update_state(|state| {
                    state.status.phase = UpdatePhase::Available;
                    state.status.available_version = Some(version);
                    state.status.notes_url = Some(notes_url);
                    state.status.downloaded_bytes = 0;
                    state.status.total_bytes = size;
                    state.status.progress_percent = 0.0;
                    state.status.last_checked_at = Some(now_seconds());
                    state.status.error_message = None;
                    state.verified = Some(verified);
                    state.downloaded_path = None;
                })
                .await;
                Ok(self.status().await)
            }
            Err(message) if message == UpdateManifestError::NotNewer.to_string() => {
                self.update_state(|state| {
                    state.status.phase = UpdatePhase::UpToDate;
                    state.status.available_version = None;
                    state.status.notes_url = None;
                    state.status.last_checked_at = Some(now_seconds());
                    state.status.error_message = None;
                    state.verified = None;
                })
                .await;
                Ok(self.status().await)
            }
            Err(message) => self.fail(&message).await,
        }
    }

    pub async fn start_download(&self) -> Result<UpdateStatus, String> {
        if !self.inner.can_download {
            return Err(
                "Esta instalação deve ser atualizada pelo gerenciador correspondente".to_owned(),
            );
        }
        let operation = self
            .inner
            .operation
            .clone()
            .try_lock_owned()
            .map_err(|_| "Já existe uma operação de atualização em andamento".to_owned())?;
        let verified = self
            .inner
            .state
            .lock()
            .await
            .verified
            .clone()
            .ok_or_else(|| "Verifique se há uma atualização antes de baixar".to_owned())?;
        let service = self.clone();
        self.update_state(|state| {
            state.status.phase = UpdatePhase::Downloading;
            state.status.downloaded_bytes = 0;
            state.status.progress_percent = 0.0;
            state.status.error_message = None;
        })
        .await;
        tokio::spawn(async move {
            let result = service.download(verified).await;
            if let Err(error) = result {
                let _ = service.fail(&error).await;
            }
            drop(operation);
        });
        Ok(self.status().await)
    }

    pub async fn apply(&self) -> Result<UpdateStatus, String> {
        let _operation = self
            .inner
            .operation
            .try_lock()
            .map_err(|_| "Já existe uma operação de atualização em andamento".to_owned())?;
        let target = self
            .inner
            .target
            .ok_or_else(|| "Esta instalação não permite aplicar atualizações".to_owned())?;
        if !self.inner.installation_method.can_self_update() {
            return Err("Esta instalação não permite aplicação automática".to_owned());
        }
        let (verified, artifact) = {
            let state = self.inner.state.lock().await;
            if state.status.phase != UpdatePhase::Downloaded {
                return Err("Baixe e valide a atualização antes de instalá-la".to_owned());
            }
            let verified = state
                .verified
                .clone()
                .ok_or_else(|| "Metadados da atualização não estão disponíveis".to_owned())?;
            let artifact = state
                .downloaded_path
                .clone()
                .filter(|path| path.is_file())
                .ok_or_else(|| "O pacote baixado não está mais disponível".to_owned())?;
            (verified, artifact)
        };
        self.update_state(|state| {
            state.status.phase = UpdatePhase::Applying;
            state.status.error_message = None;
        })
        .await;

        if let Err(error) =
            launch_installer(target, &artifact, &verified.version, &self.inner.state_path)
        {
            return self.fail(&error).await;
        }
        Ok(self.status().await)
    }

    async fn download(&self, verified: VerifiedUpdate) -> Result<(), String> {
        tokio::fs::create_dir_all(&self.inner.cache_dir)
            .await
            .map_err(|error| format!("Falha ao preparar o cache de atualização: {error}"))?;
        let filename = verified
            .asset
            .url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| "Nome do arquivo de atualização inválido".to_owned())?;
        let final_path = self.inner.cache_dir.join(filename);
        let part_path = self.inner.cache_dir.join(format!("{filename}.part"));
        let response = self
            .inner
            .download_client
            .get(&verified.asset.url)
            .send()
            .await
            .map_err(|error| format!("Falha ao baixar atualização: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Falha ao baixar atualização: {error}"))?;
        if response
            .content_length()
            .is_some_and(|size| size != verified.asset.size)
        {
            return Err("O tamanho informado pelo servidor diverge do manifesto".to_owned());
        }

        let mut response = response;
        let mut file = tokio::fs::File::create(&part_path)
            .await
            .map_err(|error| format!("Falha ao criar arquivo parcial: {error}"))?;
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        let mut downloaded = 0u64;
        let mut last_persisted = 0u64;
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(60), response.chunk())
                .await
                .map_err(|_| "O download ficou 60 segundos sem receber dados".to_owned())?
                .map_err(|error| format!("Falha durante o download: {error}"))?;
            let Some(chunk) = chunk else {
                break;
            };
            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| "Tamanho do download inválido".to_owned())?;
            if downloaded > verified.asset.size {
                return Err("O arquivo baixado é maior que o tamanho assinado".to_owned());
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| format!("Falha ao gravar arquivo parcial: {error}"))?;
            digest.update(&chunk);
            let percent = downloaded as f64 * 100.0 / verified.asset.size as f64;
            {
                let mut state = self.inner.state.lock().await;
                state.status.downloaded_bytes = downloaded;
                state.status.progress_percent = percent;
            }
            if downloaded.saturating_sub(last_persisted) >= 1024 * 1024 {
                self.persist().await;
                last_persisted = downloaded;
            }
        }
        file.flush()
            .await
            .map_err(|error| format!("Falha ao finalizar arquivo parcial: {error}"))?;
        file.sync_all()
            .await
            .map_err(|error| format!("Falha ao sincronizar arquivo parcial: {error}"))?;
        drop(file);
        if downloaded != verified.asset.size {
            return Err(format!(
                "Tamanho inválido: esperado {}, recebido {downloaded}",
                verified.asset.size
            ));
        }
        let actual_hash = digest
            .finish()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if actual_hash != verified.asset.sha256 {
            return Err("O hash SHA-256 do arquivo baixado é inválido".to_owned());
        }
        if final_path.is_file() {
            tokio::fs::remove_file(&final_path)
                .await
                .map_err(|error| format!("Falha ao substituir atualização anterior: {error}"))?;
        }
        tokio::fs::rename(&part_path, &final_path)
            .await
            .map_err(|error| format!("Falha ao concluir arquivo de atualização: {error}"))?;
        self.update_state(|state| {
            state.status.phase = UpdatePhase::Downloaded;
            state.status.downloaded_bytes = verified.asset.size;
            state.status.total_bytes = verified.asset.size;
            state.status.progress_percent = 100.0;
            state.status.error_message = None;
            state.downloaded_path = Some(final_path);
        })
        .await;
        Ok(())
    }

    async fn fetch_manifest_with_cache(&self) -> Result<(Vec<u8>, String), String> {
        match self.fetch_manifest().await {
            Ok(pair) => {
                self.save_manifest_cache(&pair.0, &pair.1).await;
                Ok(pair)
            }
            Err(network_error) => self.load_manifest_cache().await.map_err(|cache_error| {
                format!("{network_error}; cache indisponível: {cache_error}")
            }),
        }
    }

    async fn fetch_manifest(&self) -> Result<(Vec<u8>, String), String> {
        let manifest = self.fetch_limited(&self.inner.manifest_url).await?;
        let signature = self
            .fetch_limited(&format!("{}.sig", self.inner.manifest_url))
            .await?;
        let signature = String::from_utf8(signature)
            .map_err(|_| "Assinatura do manifesto não é texto válido".to_owned())?;
        Ok((manifest, signature.trim().to_owned()))
    }

    async fn fetch_limited(&self, url: &str) -> Result<Vec<u8>, String> {
        let response = self
            .inner
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("Falha ao consultar manifesto: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Falha ao consultar manifesto: {error}"))?;
        if response
            .content_length()
            .is_some_and(|size| size > MANIFEST_LIMIT as u64)
        {
            return Err("Manifesto de atualização excede o limite permitido".to_owned());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("Falha ao ler manifesto: {error}"))?;
        if bytes.len() > MANIFEST_LIMIT {
            return Err("Manifesto de atualização excede o limite permitido".to_owned());
        }
        Ok(bytes.to_vec())
    }

    async fn save_manifest_cache(&self, manifest: &[u8], signature: &str) {
        if tokio::fs::create_dir_all(&self.inner.cache_dir)
            .await
            .is_err()
        {
            return;
        }
        let metadata = serde_json::to_vec(&ManifestCacheMetadata {
            fetched_at: now_seconds(),
        })
        .unwrap_or_default();
        let _ = tokio::join!(
            tokio::fs::write(self.inner.cache_dir.join("manifest.json"), manifest),
            tokio::fs::write(self.inner.cache_dir.join("manifest.sig"), signature),
            tokio::fs::write(self.inner.cache_dir.join("manifest-cache.json"), metadata),
        );
    }

    async fn load_manifest_cache(&self) -> Result<(Vec<u8>, String), String> {
        let metadata: ManifestCacheMetadata = serde_json::from_slice(
            &tokio::fs::read(self.inner.cache_dir.join("manifest-cache.json"))
                .await
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if now_seconds().saturating_sub(metadata.fetched_at) > CACHE_MAX_AGE.as_secs() {
            return Err("manifesto em cache expirado".to_owned());
        }
        let manifest = tokio::fs::read(self.inner.cache_dir.join("manifest.json"))
            .await
            .map_err(|error| error.to_string())?;
        let signature = tokio::fs::read_to_string(self.inner.cache_dir.join("manifest.sig"))
            .await
            .map_err(|error| error.to_string())?;
        Ok((manifest, signature.trim().to_owned()))
    }

    async fn fail(&self, message: &str) -> Result<UpdateStatus, String> {
        self.update_state(|state| {
            state.status.phase = UpdatePhase::Error;
            state.status.error_message = Some(message.to_owned());
        })
        .await;
        Err(message.to_owned())
    }

    async fn update_state(&self, update: impl FnOnce(&mut PersistentUpdateState)) {
        {
            let mut state = self.inner.state.lock().await;
            update(&mut state);
        }
        self.persist().await;
    }

    async fn persist(&self) {
        let bytes = {
            let state = self.inner.state.lock().await;
            match serde_json::to_vec_pretty(&*state) {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::warn!(%error, "update state could not be encoded");
                    return;
                }
            }
        };
        if let Some(parent) = self.inner.state_path.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            tracing::warn!(%error, "update state directory could not be created");
            return;
        }
        let temporary = self.inner.state_path.with_extension("json.tmp");
        if let Err(error) = tokio::fs::write(&temporary, bytes).await {
            tracing::warn!(%error, "update state could not be written");
            return;
        }
        if let Err(first_error) = tokio::fs::rename(&temporary, &self.inner.state_path).await {
            let replaced = tokio::fs::remove_file(&self.inner.state_path).await.is_ok()
                && tokio::fs::rename(&temporary, &self.inner.state_path)
                    .await
                    .is_ok();
            if !replaced {
                tracing::warn!(error = %first_error, "update state could not be committed");
            }
        }
    }
}

fn launch_installer(
    target: UpdateTarget,
    artifact: &Path,
    version: &Version,
    state_path: &Path,
) -> Result<(), String> {
    match target {
        UpdateTarget::LinuxX86_64User => launch_linux_installer(artifact, version, state_path),
        UpdateTarget::LinuxX86_64Deb => {
            Err("Pacotes Debian devem ser aplicados pelo apt".to_owned())
        }
        UpdateTarget::WindowsX86_64 => launch_windows_installer(artifact),
    }
}

#[cfg(target_os = "linux")]
fn launch_linux_installer(
    artifact: &Path,
    version: &Version,
    state_path: &Path,
) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Falha ao localizar o atualizador: {error}"))?;
    let status = Command::new("systemd-run")
        .args(["--user", "--collect", "--property=Type=exec"])
        .arg(format!("--unit=zmydia-update-{}", version))
        .arg(executable)
        .args(["update-helper", "linux"])
        .arg(artifact)
        .arg(version.to_string())
        .arg(state_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Falha ao iniciar o aplicador Linux: {error}"))?;
    if !status.success() {
        return Err(format!(
            "O systemd recusou o aplicador da atualização ({status})"
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn launch_linux_installer(_: &Path, _: &Version, _: &Path) -> Result<(), String> {
    Err("O aplicador Linux não está disponível nesta plataforma".to_owned())
}

#[cfg(target_os = "windows")]
fn launch_windows_installer(artifact: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    Command::new(artifact)
        .arg("/S")
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Falha ao iniciar o instalador do Windows: {error}"))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn launch_windows_installer(_: &Path) -> Result<(), String> {
    Err("O aplicador Windows não está disponível nesta plataforma".to_owned())
}

#[derive(Deserialize)]
struct PackageBuildInfo {
    version: String,
    platform: String,
}

pub fn handle(arguments: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if arguments.first().map(String::as_str) != Some("update-helper") {
        return Ok(false);
    }
    if arguments.get(1).map(String::as_str) != Some("linux") || arguments.len() != 5 {
        return Err(
            "usage: zmydia-server update-helper linux <archive> <version> <state-path>".into(),
        );
    }
    #[cfg(not(target_os = "linux"))]
    return Err("the Linux update helper is unavailable on this platform".into());
    #[cfg(target_os = "linux")]
    {
        let archive = PathBuf::from(&arguments[2]);
        let version = Version::parse(&arguments[3])?;
        let state_path = PathBuf::from(&arguments[4]);
        apply_linux_archive(&archive, &version, &state_path)?;
        Ok(true)
    }
}

#[cfg(target_os = "linux")]
fn apply_linux_archive(
    archive: &Path,
    version: &Version,
    state_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = std::env::temp_dir().join(format!(
        "zmydia-update-{}-{}-{}",
        version,
        std::process::id(),
        now_seconds()
    ));
    fs::create_dir(&temporary)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        validate_archive_entries(archive, version)?;
        command_ok(
            Command::new("tar")
                .args(["-xzf"])
                .arg(archive)
                .arg("-C")
                .arg(&temporary),
            "extract update archive",
        )?;
        let root = temporary.join(format!("zmydia-{version}-linux-x86_64"));
        validate_linux_package(&root, version)?;
        command_ok(
            Command::new(root.join("bin/zmydia-server")).args(["service", "install-update"]),
            "install Linux update",
        )?;
        command_ok(
            Command::new("systemctl").args(["--user", "start", "zmydia.service"]),
            "start updated service",
        )?;
        if let Err(error) = wait_for_readiness(version) {
            let rollback = command_ok(
                Command::new(root.join("bin/zmydia-server")).args(["service", "rollback-update"]),
                "roll back Linux update",
            );
            return match rollback {
                Ok(()) => Err(format!("{error}; previous version was restored").into()),
                Err(rollback_error) => {
                    Err(format!("{error}; rollback also failed: {rollback_error}").into())
                }
            };
        }
        let _ = Command::new(root.join("bin/zmydia-server"))
            .args(["service", "cleanup-updates"])
            .status();
        Ok(())
    })();
    if let Err(error) = result {
        let rolled_back = error.to_string().contains("previous version was restored");
        let _ = write_helper_error(state_path, &error.to_string(), rolled_back);
        let _ = Command::new("systemctl")
            .args(["--user", "start", "zmydia.service"])
            .status();
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    let _ = fs::remove_dir_all(temporary);
    Ok(())
}

#[cfg(target_os = "linux")]
fn wait_for_readiness(version: &Version) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    let expected = format!("\"version\":\"{version}\"");
    while std::time::Instant::now() < deadline {
        if let Ok(mut stream) =
            TcpStream::connect_timeout(&"127.0.0.1:4173".parse()?, Duration::from_millis(500))
        {
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            stream.write_all(
                b"GET /api/health/ready HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            )?;
            let mut response = String::new();
            stream.read_to_string(&mut response)?;
            if response.starts_with("HTTP/1.1 200")
                && response.contains("\"status\":\"ready\"")
                && response.contains(&expected)
            {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(format!("updated service did not become ready as version {version}").into())
}

#[cfg(target_os = "linux")]
fn validate_archive_entries(
    archive: &Path,
    version: &Version,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("tar").args(["-tzf"]).arg(archive).output()?;
    if !output.status.success() {
        return Err("update archive could not be listed".into());
    }
    let expected_root = format!("zmydia-{version}-linux-x86_64");
    let listing = String::from_utf8(output.stdout)?;
    if listing.lines().next().is_none() {
        return Err("update archive is empty".into());
    }
    for entry in listing.lines() {
        let path = Path::new(entry);
        if path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
            || path
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str())
                != Some(expected_root.as_str())
        {
            return Err(format!("unsafe update archive entry: {entry}").into());
        }
    }
    Ok(())
}

fn validate_linux_package(
    root: &Path,
    version: &Version,
) -> Result<(), Box<dyn std::error::Error>> {
    let metadata: PackageBuildInfo =
        serde_json::from_slice(&fs::read(root.join("BUILD-INFO.json"))?)?;
    if metadata.version != version.to_string() || metadata.platform != "linux-x86_64" {
        return Err("Linux update metadata does not match the signed manifest".into());
    }
    if !root.join("bin/zmydia-server").is_file()
        || !root.join("lib/mymedia/frontend").is_dir()
        || !root.join("lib/mymedia/extension").is_dir()
    {
        return Err("Linux update package is incomplete".into());
    }
    Ok(())
}

fn command_ok(command: &mut Command, action: &str) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("failed to {action}: {status}").into());
    }
    Ok(())
}

fn write_helper_error(
    state_path: &Path,
    message: &str,
    rolled_back: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut state: PersistentUpdateState = serde_json::from_slice(&fs::read(state_path)?)?;
    state.status.phase = if rolled_back {
        UpdatePhase::RolledBack
    } else {
        UpdatePhase::Error
    };
    state.status.error_message = Some(format!("Falha ao aplicar atualização: {message}"));
    let temporary = state_path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(&state)?)?;
    fs::rename(temporary, state_path)?;
    Ok(())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn embedded_public_key() -> Option<&'static str> {
    option_env!("ZMYDIA_UPDATE_PUBLIC_KEY").filter(|value| !value.is_empty())
}

pub fn verify_manifest(
    manifest_bytes: &[u8],
    signature_base64: &str,
    public_key_base64: &str,
    current_version: &str,
    target: UpdateTarget,
) -> Result<VerifiedUpdate, UpdateManifestError> {
    let public_key = STANDARD
        .decode(public_key_base64.trim())
        .map_err(|_| UpdateManifestError::InvalidPublicKey)?;
    if public_key.len() != 32 {
        return Err(UpdateManifestError::InvalidPublicKey);
    }
    let signature = STANDARD
        .decode(signature_base64.trim())
        .map_err(|_| UpdateManifestError::InvalidSignature)?;
    if signature.len() != 64
        || signature::UnparsedPublicKey::new(&signature::ED25519, public_key)
            .verify(manifest_bytes, &signature)
            .is_err()
    {
        return Err(UpdateManifestError::InvalidSignature);
    }

    let manifest: UpdateManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| UpdateManifestError::InvalidManifest)?;
    validate_manifest(manifest, current_version, target)
}

fn validate_manifest(
    manifest: UpdateManifest,
    current_version: &str,
    target: UpdateTarget,
) -> Result<VerifiedUpdate, UpdateManifestError> {
    if manifest.schema != UPDATE_MANIFEST_SCHEMA {
        return Err(UpdateManifestError::UnsupportedSchema);
    }
    if manifest.channel != UPDATE_CHANNEL {
        return Err(UpdateManifestError::UnsupportedChannel);
    }
    if manifest.published_at.trim().is_empty() {
        return Err(UpdateManifestError::InvalidManifest);
    }

    let version =
        Version::parse(&manifest.version).map_err(|_| UpdateManifestError::InvalidVersion)?;
    let current =
        Version::parse(current_version).map_err(|_| UpdateManifestError::InvalidVersion)?;
    let minimum = Version::parse(&manifest.min_updater_version)
        .map_err(|_| UpdateManifestError::InvalidVersion)?;
    if version <= current {
        return Err(UpdateManifestError::NotNewer);
    }
    if current < minimum {
        return Err(UpdateManifestError::UpdaterTooOld);
    }

    validate_notes_url(&manifest.notes_url, &version)?;
    let asset = manifest
        .assets
        .get(target.key())
        .cloned()
        .ok_or(UpdateManifestError::MissingAsset)?;
    validate_asset(&asset, target, &version)?;
    Ok(VerifiedUpdate { version, asset })
}

fn validate_notes_url(url: &str, version: &Version) -> Result<(), UpdateManifestError> {
    let expected = format!("https://github.com/alissonlima086/ZMydia/releases/tag/v{version}");
    if url != expected {
        return Err(UpdateManifestError::InvalidManifest);
    }
    Ok(())
}

fn validate_asset(
    asset: &UpdateAsset,
    target: UpdateTarget,
    version: &Version,
) -> Result<(), UpdateManifestError> {
    if asset.kind != target.expected_kind()
        || asset.size == 0
        || asset.sha256.len() != 64
        || !asset
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(UpdateManifestError::InvalidAsset);
    }
    let expected_url = format!(
        "{RELEASE_BASE_URL}/v{version}/{}",
        target.expected_filename(version)
    );
    if asset.url != expected_url {
        return Err(UpdateManifestError::InvalidAsset);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    const TEST_SEED: [u8; 32] = [7; 32];

    fn signed_manifest(version: &str) -> (Vec<u8>, String, String) {
        let manifest = UpdateManifest {
            schema: UPDATE_MANIFEST_SCHEMA,
            channel: UPDATE_CHANNEL.to_owned(),
            version: version.to_owned(),
            published_at: "2026-11-20T12:00:00Z".to_owned(),
            min_updater_version: "0.1.1".to_owned(),
            notes_url: format!("https://github.com/alissonlima086/ZMydia/releases/tag/v{version}"),
            assets: BTreeMap::from([
                (
                    "windows-x86_64".to_owned(),
                    UpdateAsset {
                        kind: UpdateAssetKind::Nsis,
                        url: format!(
                            "{RELEASE_BASE_URL}/v{version}/zmydia-{version}-windows-x86_64-installer.exe"
                        ),
                        size: 100,
                        sha256: "ab".repeat(32),
                    },
                ),
                (
                    "linux-x86_64-user".to_owned(),
                    UpdateAsset {
                        kind: UpdateAssetKind::Tar,
                        url: format!(
                            "{RELEASE_BASE_URL}/v{version}/zmydia-{version}-linux-x86_64.tar.gz"
                        ),
                        size: 200,
                        sha256: "cd".repeat(32),
                    },
                ),
                (
                    "linux-x86_64-deb".to_owned(),
                    UpdateAsset {
                        kind: UpdateAssetKind::Deb,
                        url: format!("{RELEASE_BASE_URL}/v{version}/zmydia_{version}_amd64.deb"),
                        size: 300,
                        sha256: "ef".repeat(32),
                    },
                ),
            ]),
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let key_pair = Ed25519KeyPair::from_seed_unchecked(&TEST_SEED).unwrap();
        let signature = STANDARD.encode(key_pair.sign(&bytes).as_ref());
        let public_key = STANDARD.encode(key_pair.public_key().as_ref());
        (bytes, signature, public_key)
    }

    #[test]
    fn accepts_a_signed_manifest_for_the_expected_platform() {
        let (manifest, signature, public_key) = signed_manifest("0.1.2");
        let verified = verify_manifest(
            &manifest,
            &signature,
            &public_key,
            "0.1.1",
            UpdateTarget::WindowsX86_64,
        )
        .unwrap();
        assert_eq!(verified.version, Version::new(0, 1, 2));
        assert_eq!(verified.asset.kind, UpdateAssetKind::Nsis);
    }

    #[test]
    fn rejects_a_tampered_manifest_or_signature() {
        let (mut manifest, signature, public_key) = signed_manifest("0.1.2");
        manifest[0] ^= 1;
        assert_eq!(
            verify_manifest(
                &manifest,
                &signature,
                &public_key,
                "0.1.1",
                UpdateTarget::WindowsX86_64,
            ),
            Err(UpdateManifestError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_equal_versions_and_downgrades() {
        for version in ["0.1.1", "0.1.0"] {
            let (manifest, signature, public_key) = signed_manifest(version);
            assert_eq!(
                verify_manifest(
                    &manifest,
                    &signature,
                    &public_key,
                    "0.1.1",
                    UpdateTarget::LinuxX86_64User,
                ),
                Err(UpdateManifestError::NotNewer)
            );
        }
    }

    #[test]
    fn rejects_unsupported_installations_and_platforms() {
        assert_eq!(
            UpdateTarget::for_installation(InstallationMethod::Nix, "linux", "x86_64"),
            Ok(UpdateTarget::LinuxX86_64User)
        );
        assert_eq!(
            UpdateTarget::for_installation(InstallationMethod::Debian, "linux", "x86_64"),
            Ok(UpdateTarget::LinuxX86_64Deb)
        );
        assert_eq!(
            UpdateTarget::for_installation(InstallationMethod::WindowsNsis, "linux", "x86_64"),
            Err(UpdateManifestError::UnsupportedTarget)
        );
        assert_eq!(
            UpdateTarget::for_installation(InstallationMethod::LinuxUser, "linux", "aarch64"),
            Err(UpdateManifestError::UnsupportedTarget)
        );
    }

    #[test]
    fn validates_linux_package_identity_and_required_files() {
        let version = Version::new(0, 1, 2);
        let root = std::env::temp_dir().join(format!(
            "zmydia-package-validation-{}-{}",
            std::process::id(),
            now_seconds()
        ));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("lib/mymedia/frontend")).unwrap();
        fs::create_dir_all(root.join("lib/mymedia/extension")).unwrap();
        fs::write(root.join("bin/zmydia-server"), "binary").unwrap();
        fs::write(
            root.join("BUILD-INFO.json"),
            r#"{"version":"0.1.2","platform":"linux-x86_64"}"#,
        )
        .unwrap();

        validate_linux_package(&root, &version).unwrap();
        fs::write(
            root.join("BUILD-INFO.json"),
            r#"{"version":"0.1.3","platform":"linux-x86_64"}"#,
        )
        .unwrap();
        assert!(validate_linux_package(&root, &version).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
