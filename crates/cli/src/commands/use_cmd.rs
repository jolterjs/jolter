use std::{io::IsTerminal, process::ExitCode};

use jolter_core::Jolter;

use super::setup::install_shims;
use crate::{
    args::{UseTarget, parse_use_target},
    error::CliError,
    output::TerminalUi,
};

pub fn run_install(
    jolter: &Jolter,
    targets: Vec<UseTarget>,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let targets = if targets.is_empty() {
        vec![interactive_use_target(jolter, ui)?]
    } else {
        targets
    };
    for target in targets
        .iter()
        .filter(|target| matches!(target, UseTarget::Runtime(_)))
        .chain(
            targets
                .iter()
                .filter(|target| !matches!(target, UseTarget::Runtime(_))),
        )
    {
        match target.clone() {
            UseTarget::Runtime(request) => {
                let action = jolter.install_runtime(&request)?;
                let verb = if action.downloaded {
                    "Installed"
                } else {
                    "Already installed"
                };
                ui.success(format!(
                    "{verb} {}@{} at {}",
                    action.runtime.kind,
                    action.runtime.version,
                    action.runtime.path.display()
                ));
            }
            UseTarget::Tool(request) => {
                let action = jolter.install_tool(&request)?;
                let verb = if action.downloaded {
                    "Installed"
                } else {
                    "Already installed"
                };
                ui.success(format!(
                    "{verb} tool {}@{} at {}",
                    action.tool.kind,
                    action.tool.version,
                    action.tool.path.display()
                ));
            }
            UseTarget::PluginTool { name, selector } => {
                let action = jolter.install_plugin_tool(&name, &selector)?;
                let verb = if action.downloaded {
                    "Installed"
                } else {
                    "Already installed"
                };
                ui.success(format!(
                    "{verb} plugin tool {}@{} via {} at {}",
                    action.tool.tool,
                    action.tool.version,
                    action.provider,
                    action.tool.path.display()
                ));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn run_use(
    jolter: &Jolter,
    targets: Vec<UseTarget>,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let targets = if targets.is_empty() {
        vec![interactive_use_target(jolter, ui)?]
    } else {
        targets
    };
    for target in targets
        .iter()
        .filter(|target| matches!(target, UseTarget::Runtime(_)))
        .chain(
            targets
                .iter()
                .filter(|target| !matches!(target, UseTarget::Runtime(_))),
        )
    {
        match target.clone() {
            UseTarget::Runtime(request) => {
                let action = jolter.use_runtime(&request)?;
                install_shims(jolter)?;
                let verb = if action.downloaded {
                    "Installed and activated"
                } else {
                    "Activated"
                };
                ui.success(format!(
                    "{verb} {}@{} at {}",
                    action.runtime.kind,
                    action.runtime.version,
                    action.runtime.path.display()
                ));
            }
            UseTarget::Tool(request) => {
                let action = jolter.use_tool(&request)?;
                install_shims(jolter)?;
                let verb = if action.downloaded {
                    "Installed and activated"
                } else {
                    "Activated"
                };
                ui.success(format!(
                    "{verb} tool {}@{} at {}",
                    action.tool.kind,
                    action.tool.version,
                    action.tool.path.display()
                ));
            }
            UseTarget::PluginTool { name, selector } => {
                let action = jolter.use_plugin_tool(&name, &selector)?;
                install_shims(jolter)?;
                let verb = if action.downloaded {
                    "Installed and activated"
                } else {
                    "Activated"
                };
                ui.success(format!(
                    "{verb} plugin tool {}@{} via {} at {}",
                    action.tool.tool,
                    action.tool.version,
                    action.provider,
                    action.tool.path.display()
                ));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn interactive_use_target(jolter: &Jolter, ui: &TerminalUi) -> Result<UseTarget, CliError> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(CliError::InteractiveUseRequiresTty);
    }
    ui.finish_progress();
    let mut choices = vec![
        "node@latest".to_owned(),
        "bun@latest".to_owned(),
        "deno@latest".to_owned(),
        "npm@latest".to_owned(),
        "pnpm@latest".to_owned(),
        "yarn@latest".to_owned(),
    ];
    for plugin_tool in jolter.list_plugin_tools()? {
        choices.push(format!("{}@latest", plugin_tool.tool));
    }
    choices.sort();
    choices.dedup();
    let selected = inquire::Select::new("Use runtime or tool", choices)
        .with_help_message("Type to filter, then press Enter")
        .prompt()
        .map_err(|error| CliError::InteractivePrompt(error.to_string()))?;
    parse_use_target(&selected).map_err(CliError::InteractiveSelection)
}
