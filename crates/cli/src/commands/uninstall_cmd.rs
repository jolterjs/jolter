use std::process::ExitCode;

use jolter_core::Jolter;

use super::setup::install_shims;
use crate::{
    args::UninstallTarget,
    error::CliError,
    output::{TerminalUi, format_bytes},
};

pub fn run_uninstall(
    jolter: &Jolter,
    target: UninstallTarget,
    force: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let (name, version, outcome) = match target {
        UninstallTarget::Runtime(kind, version) => {
            let outcome = jolter.uninstall_runtime(kind, &version, force)?;
            (kind.to_string(), version, outcome)
        }
        UninstallTarget::Tool(kind, version) => {
            let outcome = jolter.uninstall_tool(kind, &version, force)?;
            (kind.to_string(), version, outcome)
        }
        UninstallTarget::PluginTool { name, version } => {
            let matches = jolter
                .list_plugin_tools()?
                .into_iter()
                .filter(|tool| tool.tool == name && tool.version == version)
                .collect::<Vec<_>>();
            let tool = match matches.as_slice() {
                [tool] => tool,
                [] => return Err(CliError::PluginToolUninstallTargetMissing(name, version)),
                _ => return Err(CliError::PluginToolUninstallTargetAmbiguous(name, version)),
            };
            let provider = tool.provider.clone();
            let outcome = jolter.uninstall_plugin_tool(&provider, &name, &version, force)?;
            (format!("{name} via {provider}"), version, outcome)
        }
    };
    install_shims(jolter)?;
    ui.success(format!(
        "Uninstalled {name}@{version} from {} ({})",
        outcome.path.display(),
        format_bytes(outcome.reclaimed_bytes)
    ));
    Ok(ExitCode::SUCCESS)
}
