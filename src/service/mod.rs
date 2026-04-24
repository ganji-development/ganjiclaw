pub use zeroclaw_runtime::service::*;

use crate::config::Config;
use anyhow::Result;
use std::path::PathBuf;

#[allow(dead_code)]
pub fn handle_command(
    command: &crate::ServiceCommands,
    config: &Config,
    init_system: InitSystem,
) -> Result<()> {
    let installed_scope = read_installed_scope(config);

    match command {
        crate::ServiceCommands::Install { scope } => {
            let scope = scope
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or(ServiceScope::User);
            install(config, init_system, scope)
        }
        crate::ServiceCommands::Start { scope } => {
            let scope = scope
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(|| installed_scope.unwrap_or(ServiceScope::User));
            start(config, init_system, scope)
        }
        crate::ServiceCommands::Stop { scope } => {
            let scope = scope
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(|| installed_scope.unwrap_or(ServiceScope::User));
            stop(config, init_system, scope)
        }
        crate::ServiceCommands::Restart { scope } => {
            let scope = scope
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(|| installed_scope.unwrap_or(ServiceScope::User));
            restart(config, init_system, scope)
        }
        crate::ServiceCommands::Status { scope } => {
            let scope = scope.as_deref().map(str::parse).transpose()?;
            status(config, init_system, scope)
        }
        crate::ServiceCommands::Uninstall { scope } => {
            let scope = scope
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(|| installed_scope.unwrap_or(ServiceScope::User));
            uninstall(config, init_system, scope)
        }
        crate::ServiceCommands::Logs {
            lines,
            follow,
            scope,
        } => {
            let scope = scope
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(|| installed_scope.unwrap_or(ServiceScope::User));
            logs(config, init_system, scope, *lines, *follow)
        }
        #[cfg(target_os = "windows")]
        crate::ServiceCommands::SetConfigDir { path } => {
            let config_dir = PathBuf::from(&path);
            windows::update_config_dir(&config_dir)
        }
    }
}
