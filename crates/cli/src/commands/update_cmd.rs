use std::{path::Path, process::ExitCode};

use jolter_core::Jolter;
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use semver::Version;

use super::setup::install_shims;
use crate::{args::UpdateTarget, error::CliError, output::TerminalUi};

pub fn run_update(
    jolter: &Jolter,
    target: Option<UpdateTarget>,
    all: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let targets = if all {
        active_update_targets(jolter)?
    } else {
        target.into_iter().collect()
    };
    if targets.is_empty() {
        ui.info("No active runtimes or tools to update.");
        return Ok(ExitCode::SUCCESS);
    }

    for target in targets {
        match target {
            UpdateTarget::Runtime(kind, request) => {
                let previous = jolter.storage().active_version(kind)?;
                let request = update_runtime_request(kind, request, previous.as_ref())?;
                let action = jolter.update_runtime(&request)?;
                print_update_result(
                    &kind.to_string(),
                    previous.as_ref(),
                    &action.runtime.version,
                    &action.runtime.path,
                    ui,
                );
            }
            UpdateTarget::Tool(kind, request) => {
                let previous = jolter.storage().active_tool_version(kind)?;
                let request = update_tool_request(kind, request, previous.as_ref())?;
                let action = jolter.update_tool(&request)?;
                print_update_result(
                    &kind.to_string(),
                    previous.as_ref(),
                    &action.tool.version,
                    &action.tool.path,
                    ui,
                );
            }
            UpdateTarget::PluginTool { name, selector } => {
                let previous = jolter.storage().active_plugin_tool(&name)?;
                let action = jolter.update_plugin_tool(&name, selector.as_deref())?;
                print_update_result(
                    &name,
                    previous.as_ref().map(|tool| &tool.version),
                    &action.tool.version,
                    &action.tool.path,
                    ui,
                );
            }
        }
    }
    install_shims(jolter)?;
    Ok(ExitCode::SUCCESS)
}

pub fn active_update_targets(jolter: &Jolter) -> Result<Vec<UpdateTarget>, CliError> {
    let mut targets = Vec::new();
    for kind in RuntimeKind::ALL {
        if jolter.storage().active_version(kind)?.is_some() {
            targets.push(UpdateTarget::Runtime(kind, None));
        }
    }
    for kind in ToolKind::ALL {
        if jolter.storage().active_tool_version(kind)?.is_some() {
            targets.push(UpdateTarget::Tool(kind, None));
        }
    }
    for tool in jolter.storage().active_plugin_tools()? {
        targets.push(UpdateTarget::PluginTool {
            name: tool.tool,
            selector: None,
        });
    }
    Ok(targets)
}

pub fn update_runtime_request(
    kind: RuntimeKind,
    request: Option<RuntimeRequest>,
    active: Option<&Version>,
) -> Result<RuntimeRequest, CliError> {
    request.map_or_else(
        || {
            let active = active.ok_or_else(|| CliError::NoActiveUpdateTarget(kind.to_string()))?;
            RuntimeRequest::new(kind, active.major.to_string())
                .map_err(|error| CliError::UpdateRequest(error.to_string()))
        },
        Ok,
    )
}

pub fn update_tool_request(
    kind: ToolKind,
    request: Option<ToolRequest>,
    active: Option<&Version>,
) -> Result<ToolRequest, CliError> {
    request.map_or_else(
        || {
            let active = active.ok_or_else(|| CliError::NoActiveUpdateTarget(kind.to_string()))?;
            ToolRequest::new(kind, active.major.to_string())
                .map_err(|error| CliError::UpdateRequest(error.to_string()))
        },
        Ok,
    )
}

pub fn print_update_result(
    name: &str,
    previous: Option<&Version>,
    current: &Version,
    path: &Path,
    ui: &TerminalUi,
) {
    match previous {
        Some(previous) if previous == current => {
            ui.info(format!(
                "{name}@{current} is already current at {}",
                path.display()
            ));
        }
        Some(previous) => {
            ui.success(format!(
                "Updated {name} from {previous} to {current} at {}",
                path.display()
            ));
        }
        None => {
            ui.success(format!(
                "Installed and activated {name}@{current} at {}",
                path.display()
            ));
        }
    }
}
