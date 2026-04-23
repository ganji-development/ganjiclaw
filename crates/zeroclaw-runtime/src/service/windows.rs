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
use std::path::PathBuf;
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

pub const SERVICE_NAME: &str = "ZeroClawDaemon";
pub const SERVICE_DISPLAY_NAME: &str = "ZeroClaw Daemon";
pub const SERVICE_DESCRIPTION: &str = "ZeroClaw autonomous agent runtime daemon";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

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
    let logs_dir = system_logs_dir();
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

/// Logs directory for the SCM-scoped daemon.
/// Prefers `%PROGRAMDATA%\ZeroClaw\logs`, falls back to `C:\ProgramData\ZeroClaw\logs`,
/// then current directory.
pub fn system_logs_dir() -> PathBuf {
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        return PathBuf::from(programdata).join("ZeroClaw").join("logs");
    }
    PathBuf::from(r"C:\ProgramData\ZeroClaw\logs")
}

/// Install as a proper Windows SCM service (auto-start, LocalSystem).
/// Requires Administrator privileges.
pub fn install_system() -> Result<()> {
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

    let logs_dir = system_logs_dir();
    fs::create_dir_all(&logs_dir).ok();

    println!("✅ Installed Windows service: {SERVICE_DISPLAY_NAME}");
    println!("   Service name: {SERVICE_NAME}");
    println!("   Start type:   Automatic (runs at boot as LocalSystem)");
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
    println!("✅ Windows service uninstalled: {SERVICE_NAME}");
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
    let exe = std::env::current_exe().context("Failed to resolve current executable path")?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    println!("⚠️  Administrative privileges required. Requesting elevation...");

    // Construct powershell command:
    // $ErrorActionPreference = 'Stop'; Start-Process -FilePath "..." -ArgumentList "...", "..." -Verb RunAs -Wait
    let mut ps_cmd = format!(
        "$ErrorActionPreference = 'Stop'; Start-Process -FilePath '{}'",
        exe.display()
    );
    if !args.is_empty() {
        let escaped_args: Vec<String> = args
            .iter()
            .map(|a| format!("'{}'", a.replace("'", "''")))
            .collect();
        ps_cmd.push_str(&format!(" -ArgumentList {}", escaped_args.join(", ")));
    }
    ps_cmd.push_str(" -Verb RunAs -Wait");

    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_cmd])
        .status()
        .context("Failed to launch elevated process")?;

    if status.success() {
        // If the elevated process finished successfully, we should exit.
        // The work has already been done by the child.
        std::process::exit(0);
    } else {
        bail!("Elevated process failed or was cancelled.")
    }
}
