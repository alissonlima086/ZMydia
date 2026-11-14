use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use walkdir::WalkDir;

const SERVICE_NAME: &str = "zmydia.service";
const BACKUP_SERVICE_NAME: &str = "zmydia-backup.service";
const BACKUP_TIMER_NAME: &str = "zmydia-backup.timer";

#[derive(Debug, Clone)]
struct InstallPaths {
    binary: PathBuf,
    app: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    config: PathBuf,
    units: PathBuf,
}

impl InstallPaths {
    fn from_home(home: &Path) -> Self {
        Self {
            binary: home.join(".local/bin/zmydia-server"),
            app: home.join(".local/lib/zmydia"),
            data: home.join(".local/share/zmydia"),
            cache: home.join(".cache/zmydia"),
            config: home.join(".config/zmydia"),
            units: home.join(".config/systemd/user"),
        }
    }
}

pub fn handle(arguments: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if arguments.first().map(String::as_str) != Some("service") {
        return Ok(false);
    }
    let action = arguments.get(1).map(String::as_str).unwrap_or("status");
    ensure_linux()?;
    match action {
        "install" => install()?,
        "uninstall" => uninstall()?,
        "status" => forward("systemctl", &["--user", "status", SERVICE_NAME])?,
        "restart" => forward("systemctl", &["--user", "restart", SERVICE_NAME])?,
        "logs" => forward("journalctl", &["--user", "-u", SERVICE_NAME, "-f"])?,
        "help" | "--help" | "-h" => print_help(),
        value => return Err(format!("unknown service command: {value}").into()),
    }
    Ok(true)
}

fn ensure_linux() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err("service management is available only on Linux".into())
    }
}

fn home_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not defined".into())
}

fn package_root(executable: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("cannot determine the ZMydia package directory")?;
    if !root.join("lib/mymedia/frontend/index.html").is_file()
        || !root.join("lib/mymedia/extension/manifest.json").is_file()
    {
        return Err(
            "service install must be run from an extracted ZMydia Linux release package".into(),
        );
    }
    Ok(root.to_path_buf())
}

fn install() -> Result<(), Box<dyn std::error::Error>> {
    require_command("systemctl")?;
    let executable = env::current_exe()?;
    let package = package_root(&executable)?;
    let paths = InstallPaths::from_home(&home_dir()?);

    let _ = systemctl(&["stop", SERVICE_NAME]);
    create_directories(&paths)?;
    copy_file(&executable, &paths.binary, true)?;
    copy_tree(&package.join("lib/mymedia"), &paths.app)?;
    write_units(&paths)?;
    create_private_env_file(&paths.config.join("zmydia.env"))?;

    systemctl_ok(&["daemon-reload"])?;
    systemctl_ok(&["enable", "--now", BACKUP_TIMER_NAME])?;
    systemctl_ok(&["enable", "--now", SERVICE_NAME])?;

    println!("ZMydia is installed and running at http://127.0.0.1:4173");
    println!("Run `zmydia-server service status` to inspect the service.");
    Ok(())
}

fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
    require_command("systemctl")?;
    let paths = InstallPaths::from_home(&home_dir()?);
    let _ = systemctl(&["disable", "--now", BACKUP_TIMER_NAME]);
    let _ = systemctl(&["stop", BACKUP_SERVICE_NAME]);
    let _ = systemctl(&["disable", "--now", SERVICE_NAME]);

    remove_file_if_present(&paths.units.join(SERVICE_NAME))?;
    remove_file_if_present(&paths.units.join(BACKUP_SERVICE_NAME))?;
    remove_file_if_present(&paths.units.join(BACKUP_TIMER_NAME))?;
    remove_file_if_present(&paths.binary)?;
    remove_dir_if_present(&paths.app)?;
    systemctl_ok(&["daemon-reload"])?;
    let _ = systemctl(&["reset-failed"]);

    println!("ZMydia was uninstalled.");
    println!("Library data was preserved at {}", paths.data.display());
    println!("Configuration was preserved at {}", paths.config.display());
    Ok(())
}

fn create_directories(paths: &InstallPaths) -> io::Result<()> {
    for path in [
        paths.binary.parent().expect("binary parent"),
        paths.app.as_path(),
        paths.data.as_path(),
        paths.cache.as_path(),
        paths.config.as_path(),
        paths.units.as_path(),
    ] {
        fs::create_dir_all(path)?;
    }
    set_mode(&paths.config, 0o700)?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !source.is_dir() {
        return Err(format!("package asset directory not found: {}", source.display()).into());
    }
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            copy_file(entry.path(), &target, is_executable(entry.path())?)?;
        } else if entry.file_type().is_symlink() {
            return Err(format!(
                "package contains an unsupported symlink: {}",
                entry.path().display()
            )
            .into());
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path, executable: bool) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    set_mode(destination, if executable { 0o755 } else { 0o644 })
}

fn write_units(paths: &InstallPaths) -> io::Result<()> {
    fs::write(paths.units.join(SERVICE_NAME), service_unit())?;
    fs::write(paths.units.join(BACKUP_SERVICE_NAME), backup_service_unit())?;
    fs::write(paths.units.join(BACKUP_TIMER_NAME), backup_timer_unit())?;
    Ok(())
}

fn create_private_env_file(path: &Path) -> io::Result<()> {
    if !path.exists() {
        fs::write(path, "")?;
    }
    set_mode(path, 0o600)
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_dir_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn require_command(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    match Command::new(command).arg("--version").status() {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(format!("required command not found: {command}").into())
        }
        Err(error) => Err(error.into()),
    }
}

fn systemctl(arguments: &[&str]) -> io::Result<ExitStatus> {
    Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .status()
}

fn systemctl_ok(arguments: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = systemctl(arguments)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "systemctl --user {} failed with {status}",
            arguments.join(" ")
        )
        .into())
    }
}

fn forward(command: &str, arguments: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new(command).args(arguments).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command} exited with {status}").into())
    }
}

fn print_help() {
    println!(
        "ZMydia service management\n\nUSAGE:\n  zmydia-server service install\n  zmydia-server service status\n  zmydia-server service restart\n  zmydia-server service logs\n  zmydia-server service uninstall"
    );
}

fn service_unit() -> &'static str {
    "[Unit]\nDescription=ZMydia personal video library\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nExecStart=%h/.local/bin/zmydia-server serve\nRestart=on-failure\nRestartSec=5\nTimeoutStopSec=45\nEnvironment=\"PATH=%h/.local/lib/zmydia/bin:/usr/local/bin:/usr/bin:/bin\"\nEnvironment=\"MYMEDIA_DATA_DIR=%h/.local/share/zmydia\"\nEnvironment=\"MYMEDIA_CACHE_DIR=%h/.cache/zmydia\"\nEnvironment=\"MYMEDIA_FRONTEND_DIR=%h/.local/lib/zmydia/frontend\"\nEnvironment=\"MYMEDIA_EXTENSION_DIR=%h/.local/lib/zmydia/extension\"\nEnvironment=\"MYMEDIA_PORT=4173\"\nEnvironment=\"MYMEDIA_LOG_FORMAT=json\"\nEnvironmentFile=-%h/.config/zmydia/zmydia.env\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=full\nProtectKernelTunables=true\nProtectKernelModules=true\nProtectControlGroups=true\nRestrictRealtime=true\nRestrictSUIDSGID=true\nLockPersonality=true\nMemoryHigh=1500M\nMemoryMax=2500M\nCPUQuota=200%\n\n[Install]\nWantedBy=default.target\n"
}

fn backup_service_unit() -> &'static str {
    "[Unit]\nDescription=Create a ZMydia library backup\nAfter=zmydia.service\n\n[Service]\nType=oneshot\nExecStart=%h/.local/bin/zmydia-server backup\nEnvironment=\"PATH=%h/.local/lib/zmydia/bin:/usr/local/bin:/usr/bin:/bin\"\nEnvironment=\"MYMEDIA_DATA_DIR=%h/.local/share/zmydia\"\nEnvironment=\"MYMEDIA_CACHE_DIR=%h/.cache/zmydia\"\nEnvironment=\"MYMEDIA_FRONTEND_DIR=%h/.local/lib/zmydia/frontend\"\nEnvironment=\"MYMEDIA_EXTENSION_DIR=%h/.local/lib/zmydia/extension\"\nEnvironment=\"MYMEDIA_LOG_FORMAT=json\"\nEnvironmentFile=-%h/.config/zmydia/zmydia.env\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=full\n"
}

fn backup_timer_unit() -> &'static str {
    "[Unit]\nDescription=Create a daily ZMydia library backup\n\n[Timer]\nOnCalendar=daily\nPersistent=true\nRandomizedDelaySec=15m\n\n[Install]\nWantedBy=timers.target\n"
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(path.metadata()?.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> io::Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_paths_are_scoped_to_the_user_home() {
        let paths = InstallPaths::from_home(Path::new("/home/tester"));
        assert_eq!(
            paths.binary,
            Path::new("/home/tester/.local/bin/zmydia-server")
        );
        assert_eq!(paths.data, Path::new("/home/tester/.local/share/zmydia"));
        assert_eq!(paths.units, Path::new("/home/tester/.config/systemd/user"));
    }

    #[test]
    fn package_root_requires_release_assets() {
        let root = env::temp_dir().join(format!("zmydia-package-root-{}", std::process::id()));
        let executable = root.join("bin/zmydia-server");
        fs::create_dir_all(root.join("lib/mymedia/frontend")).unwrap();
        fs::create_dir_all(root.join("lib/mymedia/extension")).unwrap();
        fs::write(root.join("lib/mymedia/frontend/index.html"), "test").unwrap();
        fs::write(root.join("lib/mymedia/extension/manifest.json"), "{}").unwrap();
        assert_eq!(package_root(&executable).unwrap(), root);
    }

    #[test]
    fn generated_units_use_the_installed_binary_and_assets() {
        assert!(service_unit().contains("ExecStart=%h/.local/bin/zmydia-server serve"));
        assert!(service_unit().contains("MYMEDIA_FRONTEND_DIR=%h/.local/lib/zmydia/frontend"));
        assert!(backup_service_unit().contains("zmydia-server backup"));
        assert!(backup_timer_unit().contains("OnCalendar=daily"));
    }
}
