use std::process::ExitCode;

use jolter_core::Jolter;

use super::{
    inventory::{print_plugins, print_plugins_json},
    setup::install_shims,
};
use crate::{
    args::PluginCommand,
    error::CliError,
    output::{TerminalUi, format_bytes},
};

pub fn run_plugin(
    jolter: &Jolter,
    command: PluginCommand,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    match command {
        PluginCommand::Install { target } => {
            let action = jolter.install_plugin(&target)?;
            install_shims(jolter)?;
            ui.success(format!(
                "Installed plugin {}@{} at {}",
                action.plugin.canonical_name,
                action.plugin.version,
                action.plugin.path.display()
            ));
        }
        PluginCommand::List { json } => {
            if json {
                ui.finish_progress();
                print_plugins_json(jolter)?;
            } else {
                print_plugins(jolter, ui)?;
            }
        }
        PluginCommand::Update { target, all } => {
            if all {
                let mut names = jolter
                    .list_plugins()?
                    .into_iter()
                    .map(|plugin| plugin.canonical_name)
                    .collect::<Vec<_>>();
                names.sort();
                names.dedup();
                if names.is_empty() {
                    ui.info("No plugins installed.");
                    return Ok(ExitCode::SUCCESS);
                }
                for name in names {
                    let action = jolter.update_plugin(&name)?;
                    ui.success(format!(
                        "Updated plugin {}@{}",
                        action.plugin.canonical_name, action.plugin.version
                    ));
                }
                install_shims(jolter)?;
            } else {
                let target = target.ok_or(CliError::PluginUpdateTargetRequired)?;
                let action = jolter.update_plugin(&target)?;
                install_shims(jolter)?;
                ui.success(format!(
                    "Updated plugin {}@{}",
                    action.plugin.canonical_name, action.plugin.version
                ));
            }
        }
        PluginCommand::Uninstall { name, force } => {
            let outcome = jolter.uninstall_plugin(&name, force)?;
            install_shims(jolter)?;
            ui.success(format!(
                "Uninstalled plugin from {} ({})",
                outcome.path.display(),
                format_bytes(outcome.reclaimed_bytes)
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}
