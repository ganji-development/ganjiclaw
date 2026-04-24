//! Windows Service Control Manager (SCM) integration for the ZeroClaw daemon.
//!
//! Provides system-scope service install/uninstall/start/stop via the SCM, plus
//! a service dispatcher entry point (`run_as_service`) that the hidden
//! `__windows-service-run` subcommand invokes when started by SCM.
//!
//! User-scope install remains in `mod.rs` (Task Scheduler via `schtasks`).

use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use std::os::windows::process::CommandExt;
use super::is_root;
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
    Error as WinServiceError,
};
use winreg::enums::*;
use winreg::RegKey;

pub const SERVICE_NAME: &str = "ZeroClawDaemon";
pub const SERVICE_DISPLAY_NAME: &str = "ZeroClaw Daemon";
pub const SERVICE_DESCRIPTION: &str = "ZeroClaw autonomous agent runtime daemon";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const REGISTRY_KEY: &str = r"SOFTWARE\ZeroClaw";
const REGISTRY_VALUE_CONFIG_DIR: &str = "ConfigDir";

/// Read the config directory from the registry
fn get_stored_config_dir() -> Option<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey_with_flags(REGISTRY_KEY, KEY_READ) {
        if let Ok(config_dir) = key.get_value::<String, _>(REGISTRY_VALUE_CONFIG_DIR) {
            return Some(PathBuf::from(config_dir));
        }
    }
    None
}

/// Store the config directory in the registry
fn set_stored_config_dir(config_dir: &PathBuf) -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (key, _) = hklm.create_subkey(REGISTRY_KEY)
        .context("Failed to create registry key")?;
    key.set_value(REGISTRY_VALUE_CONFIG_DIR, &config_dir.to_string_lossy().to_string())
        .context("Failed to set registry value")?;
    Ok(())
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_args: Vec<OsString>) {
    if let Err(err) = run_service() {
        let _ = write_fatal_log(&format!("service_main failed: {err:#}"));
    }
}

/// Called by `main` when the hidden `__windows-service-run` subcommand is
/// invoked by SCM. Blocks until the service stops.
pub fn run_as_service() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .context("Failed to start Windows service dispatcher")?;
    Ok(())
}

fn run_service() -> Result<()> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)
        .context("Failed to register service control handler")?;

    set_status(
        &status_handle,
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
        Duration::from_secs(10),
    )?;

    let mut child = spawn_daemon_child().context("Failed to spawn daemon child process")?;

    set_status(
        &status_handle,
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        Duration::default(),
    )?;

    loop {
        if let Ok(Some(_status)) = child.try_wait() {
            break;
        }
        if shutdown_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
            break;
        }
    }

    set_status(
        &status_handle,
        ServiceState::StopPending,
        ServiceControlAccept::empty(),
        Duration::from_secs(30),
    )?;

    let _ = child.kill();
    let _ = child.wait();

    set_status(
        &status_handle,
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        Duration::default(),
    )?;
    Ok(())
}

fn set_status(
    handle: &service_control_handler::ServiceStatusHandle,
    state: ServiceState,
    controls: ServiceControlAccept,
    wait_hint: Duration,
) -> Result<()> {
    handle
        .set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: state,
            controls_accepted: controls,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint,
            process_id: None,
        })
        .context("Failed to set service status")?;
    Ok(())
}

fn spawn_daemon_child() -> Result<Child> {
    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let config_dir = system_config_dir();
    let logs_dir = system_logs_dir();

    // Ensure directories exist
    fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create config directory: {}", config_dir.display()))?;
    fs::create_dir_all(&logs_dir)
        .with_context(|| format!("Failed to create logs directory: {}", logs_dir.display()))?;

    let stdout_log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("daemon.stdout.log"))
        .context("Failed to open daemon stdout log")?;
    let stderr_log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("daemon.stderr.log"))
        .context("Failed to open daemon stderr log")?;

    Command::new(&exe)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log))
        .creation_flags(0x08000000) // CREATE_NO_WINDOW: no console window
        .spawn()
        .context("Failed to spawn zeroclaw daemon child")
}

fn write_fatal_log(msg: &str) -> std::io::Result<()> {
    use std::io::Write;
    let logs_dir = system_logs_dir();
    fs::create_dir_all(&logs_dir)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("service.fatal.log"))?;
    writeln!(f, "{msg}")
}

/// Config directory for the SCM-scoped daemon.
/// Uses stored config directory from registry, falls back to system-wide location.
pub fn system_config_dir() -> PathBuf {
    // Check for stored config directory in registry
    if let Some(config_dir) = get_stored_config_dir() {
        return config_dir;
    }
    // Fall back to system-wide location
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        return PathBuf::from(programdata).join("ZeroClaw");
    }
    PathBuf::from(r"C:\ProgramData\ZeroClaw")
}

/// Logs directory for the SCM-scoped daemon.
/// Prefers `%PROGRAMDATA%\ZeroClaw\logs`, falls back to `C:\ProgramData\ZeroClaw\logs`,
/// then current directory.
pub fn system_logs_dir() -> PathBuf {
    system_config_dir().join("logs")
}

/// Install as a proper Windows SCM service (auto-start, LocalSystem).
///
/// `config_dir` is the resolved config directory the daemon should use. It is
/// persisted to `HKLM\SOFTWARE\ZeroClaw\ConfigDir` so the SCM-spawned daemon
/// passes it via `--config-dir`. Pass the caller's `Config::config_path.parent()`
/// so an explicit `--config-dir` / `ZEROCLAW_CONFIG_DIR` wins over the
/// system-wide fallback.
///
/// Requires Administrator privileges.
pub fn install_system(config_dir: &Path) -> Result<()> {
    // Pre-elevate with the resolved config_dir baked into args. UAC strips env
    // vars AND the elevated process runs as Administrator (whose
    // active_workspace.toml / HOME is different), so without an explicit
    // --config-dir the elevated child resolves to %PROGRAMDATA%\ZeroClaw.
    if !is_root() {
        relaunch_elevated_with_config_dir(Some(config_dir))?;
        // relaunch_elevated_with_config_dir exits the process on success.
        bail!("Administrator privileges required.");
    }

    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let manager = open_manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec![OsString::from("__windows-service-run")],
        dependencies: vec![],
        account_name: None,   // LocalSystem
        account_password: None,
    };

    let service = manager
        .create_service(&service_info, ServiceAccess::CHANGE_CONFIG)
        .context("Failed to create service (already installed?)")?;
    service
        .set_description(SERVICE_DESCRIPTION)
        .context("Failed to set service description")?;

    let config_dir = config_dir.to_path_buf();
    let logs_dir = config_dir.join("logs");
    fs::create_dir_all(&config_dir).ok();
    fs::create_dir_all(&logs_dir).ok();

    // Store config directory in registry so the SCM-spawned daemon picks it up
    set_stored_config_dir(&config_dir)?;

    println!("✅ Installed Windows service: {SERVICE_DISPLAY_NAME}");
    println!("   Service name: {SERVICE_NAME}");
    println!("   Start type:   Automatic (runs at boot as LocalSystem)");
    println!("   Config dir:   {}", config_dir.display());
    println!("   Logs:         {}", logs_dir.display());
    println!("   Start with:   zeroclaw service start --scope system");
    Ok(())
}

pub fn uninstall_system() -> Result<()> {
    let manager = open_manager(ServiceManagerAccess::CONNECT)?;
    let service = open_service(
        &manager,
        ServiceAccess::DELETE | ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
    )?;

    if let Ok(status) = service.query_status()
        && status.current_state != ServiceState::Stopped
    {
        let _ = service.stop();
        // Wait briefly for stop to take effect
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(500));
            if let Ok(s) = service.query_status()
                && s.current_state == ServiceState::Stopped
            {
                break;
            }
        }
    }

    service.delete().context("Failed to delete service")?;
    let _ = clear_stored_config_dir();
    println!("✅ Windows service uninstalled: {SERVICE_NAME}");
    Ok(())
}

/// Remove the persisted config directory value so a future install doesn't
/// inherit a stale path from a previous install.
fn clear_stored_config_dir() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey_with_flags(REGISTRY_KEY, KEY_WRITE) {
        let _ = key.delete_value(REGISTRY_VALUE_CONFIG_DIR);
    }
    Ok(())
}

pub fn start_system() -> Result<()> {
    let manager = open_manager(ServiceManagerAccess::CONNECT)?;
    let service = open_service(&manager, ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
    service
        .start::<&str>(&[])
        .context("Failed to start service")?;
    println!("✅ Service started: {SERVICE_NAME}");
    Ok(())
}

pub fn stop_system() -> Result<()> {
    let manager = open_manager(ServiceManagerAccess::CONNECT)?;
    let service = open_service(&manager, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?;
    service.stop().context("Failed to stop service")?;
    println!("✅ Service stopped: {SERVICE_NAME}");
    Ok(())
}

pub fn is_system_running() -> bool {
    let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
    else {
        return false;
    };
    let Ok(service) = manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) else {
        return false;
    };
    matches!(
        service.query_status().map(|s| s.current_state),
        Ok(ServiceState::Running)
    )
}

pub fn is_system_installed() -> bool {
    let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
    else {
        return false;
    };
    manager
        .open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)
        .is_ok()
}

/// Update the config directory for the system service.
/// Requires Administrator privileges (writes to HKLM).
pub fn update_config_dir(config_dir: &PathBuf) -> Result<()> {
    if !is_system_installed() {
        bail!("Service is not installed. Run 'zeroclaw service install --scope system' first.");
    }

    if !is_root() {
        relaunch_elevated_with_config_dir(Some(config_dir))?;
        bail!("Administrator privileges required.");
    }

    // Store config directory in registry
    set_stored_config_dir(config_dir)?;

    println!("✅ Updated service config directory: {}", config_dir.display());
    println!("   Restart the service for changes to take effect:");
    println!("   zeroclaw service restart --scope system");
    Ok(())
}

fn open_manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
    match ServiceManager::local_computer(None::<&str>, access) {
        Ok(m) => Ok(m),
        Err(e) => {
            if !is_root() {
                request_elevation()?;
                bail!("Access denied. Please run as Administrator.");
            }
            Err(anyhow::Error::new(e).context("Failed to open Service Control Manager"))
        }
    }
}

pub fn status_system() -> Result<()> {
    if !is_system_installed() {
        println!("Service: ❌ not installed (system scope)");
        return Ok(());
    }
    let running = is_system_running();
    println!(
        "Service: {} (system scope)",
        if running { "✅ running" } else { "❌ not running" }
    );
    println!("Service name: {SERVICE_NAME}");
    println!("Config dir:   {}", system_config_dir().display());
    println!("Logs:         {}", system_logs_dir().display());
    Ok(())
}

fn open_service(
    manager: &ServiceManager,
    access: ServiceAccess,
) -> Result<windows_service::service::Service> {
    match manager.open_service(SERVICE_NAME, access) {
        Ok(service) => Ok(service),
        Err(e) => {
            if matches!(&e, WinServiceError::Winapi(io_err) if io_err.raw_os_error() == Some(5))
                && !is_root()
            {
                // Attempt to relaunch with elevation
                request_elevation()?;
                // If relaunch returns (unlikely as it calls exit), return the error
                bail!("Access denied. Please run as Administrator.");
            }
            Err(anyhow::Error::new(e).context("Service not installed"))
        }
    }
}

fn request_elevation() -> Result<()> {
    relaunch_elevated_with_config_dir(None)
}

/// Relaunch the current process with UAC elevation, optionally injecting an
/// explicit `--config-dir` so the elevated child resolves the same workspace
/// as the unelevated parent.
///
/// UAC elevation via ShellExecuteEx ("runas") does NOT inherit the parent
/// process's environment, and the elevated child runs as the elevated user
/// (typically Administrator) — so neither `ZEROCLAW_CONFIG_DIR` nor the
/// per-user `active_workspace.toml` marker survives. Callers that already know
/// the resolved config dir should pass it via `override_config_dir`; otherwise
/// we fall back to forwarding `ZEROCLAW_CONFIG_DIR` if it happens to be set.
fn relaunch_elevated_with_config_dir(override_config_dir: Option<&Path>) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to resolve current executable path")?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    let already_has_flag = args
        .iter()
        .any(|a| a == "--config-dir" || a.starts_with("--config-dir="));
    if !already_has_flag {
        let injected = override_config_dir
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| std::env::var("ZEROCLAW_CONFIG_DIR").ok())
            .filter(|s| !s.trim().is_empty());
        if let Some(cfg) = injected {
            args.insert(0, cfg);
            args.insert(0, "--config-dir".to_string());
        }
    }

    println!("⚠️  Administrative privileges required. Requesting elevation...");

    // UAC-elevated processes started via ShellExecuteEx ("runas") cannot have
    // their stdout/stderr redirected from the launcher (UseShellExecute must be
    // true, which is incompatible with -RedirectStandardOutput). Without
    // redirection the elevated child runs in a separate console window that
    // closes immediately — the user sees nothing and the parent has no idea
    // whether the install actually succeeded.
    //
    // Workaround: write a temp .cmd that runs the binary with `> log 2>&1`,
    // launch *that* elevated. The redirection happens inside the elevated cmd
    // process where it's allowed. After cmd exits we read the log file and
    // tee it back to the parent's stderr so the user sees what happened.
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();
    let script_file = tmp_dir.join(format!("zeroclaw-elevate-{pid}.cmd"));
    let log_file = tmp_dir.join(format!("zeroclaw-elevate-{pid}.log"));
    let _ = fs::remove_file(&log_file);
    let _ = fs::remove_file(&script_file);

    let mut script = String::from("@echo off\r\n");
    script.push('"');
    script.push_str(&exe.to_string_lossy());
    script.push('"');
    for a in &args {
        script.push(' ');
        script.push('"');
        script.push_str(&a.replace('"', "\"\""));
        script.push('"');
    }
    script.push_str(&format!(" > \"{}\" 2>&1\r\n", log_file.display()));
    script.push_str("exit /b %ERRORLEVEL%\r\n");
    fs::write(&script_file, script).context("Failed to write elevation script")?;

    // Launch the .cmd elevated. Single-quoted PowerShell strings are literal,
    // so the only escaping needed is doubling embedded single quotes.
    let script_path_for_ps = script_file.display().to_string().replace('\'', "''");
    let ps_cmd = format!(
        "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath '{script_path_for_ps}' \
         -Verb RunAs -Wait -PassThru; exit $p.ExitCode"
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .status();

    // Always tee the elevated child's output back to the parent — even on
    // failure — so the user sees install messages and any error that caused
    // the failure.
    if let Ok(content) = fs::read_to_string(&log_file) {
        if !content.is_empty() {
            eprint!("{content}");
        }
    }
    let _ = fs::remove_file(&log_file);
    let _ = fs::remove_file(&script_file);

    let status = status.context("Failed to launch elevated process")?;
    if status.success() {
        std::process::exit(0);
    } else {
        bail!(
            "Elevated process exited with code {}",
            status.code().unwrap_or(-1)
        )
    }
}
