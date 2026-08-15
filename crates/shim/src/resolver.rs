use std::{collections::BTreeSet, fs, path::Path};

use jolter_resolver::resolve;
use jolter_runtime::{RuntimeKind, ToolKind};
use jolter_storage::{InstalledPluginTool, InstalledRuntime, InstalledTool, Storage};
use serde::Deserialize;

use crate::{
    error::ShimError,
    target::{ResolvedCommand, ShimTarget, target_for_command},
};

pub fn resolve_command(
    command: &str,
    project: &Path,
    storage: &Storage,
) -> Result<ResolvedCommand, ShimError> {
    let target = target_for_command(command)
        .or_else(|| plugin_command_exists(storage, command).then_some(ShimTarget::PluginCommand))
        .ok_or_else(|| ShimError::UnsupportedCommand(command.to_owned()))?;
    let project_resolution = resolve(project)?;
    if target == ShimTarget::PluginCommand {
        let tool = match project_plugin_tool_for_command(storage, command, &project_resolution)? {
            Some(tool) => Some(tool),
            None => active_plugin_tool_for_command(storage, command)?,
        }
        .ok_or_else(|| ShimError::PluginToolNotInstalled(command.to_owned()))?;
        let executable = tool.executable_for_command(command);
        if !executable.is_file() {
            return Err(ShimError::ExecutableNotFound {
                command: command.to_owned(),
                path: executable,
            });
        }
        return Ok(ResolvedCommand {
            executable,
            arguments: Vec::new(),
            runtime_root: None,
            runtime: None,
        });
    }
    let kind = match target {
        ShimTarget::Runtime(kind) => kind,
        ShimTarget::NodeTool(_) => RuntimeKind::Node,
        ShimTarget::PluginCommand => unreachable!("plugin command returned early"),
    };
    let project_request = project_resolution
        .runtime
        .filter(|runtime| runtime.request.kind == kind)
        .map(|runtime| runtime.request);
    let runtime = match project_request {
        Some(request) => storage
            .find_matching(&request)?
            .ok_or(ShimError::RuntimeNotInstalled(request))?,
        None => active_runtime(storage, kind)?,
    };
    let (executable, arguments) = match target {
        ShimTarget::Runtime(_) => (
            storage.runtime_executable(kind, &runtime.version),
            Vec::new(),
        ),
        ShimTarget::NodeTool(command) => {
            let project_tool = project_resolution
                .tools
                .into_iter()
                .find(|resolved| resolved.request.kind.entrypoint(command).is_some());
            let managed_tool = if let Some(resolved) = project_tool {
                Some(
                    storage
                        .find_matching_tool(&resolved.request)?
                        .ok_or_else(|| ShimError::ToolNotInstalled(resolved.request.clone()))?,
                )
            } else {
                active_tool_for_command(storage, command)?
            };
            if let Some(tool) = managed_tool {
                let entrypoint = storage
                    .tool_entrypoint(tool.kind, &tool.version, command)
                    .ok_or_else(|| ShimError::ExecutableNotFound {
                        command: command.to_owned(),
                        path: tool.path.clone(),
                    })?;
                if !entrypoint.is_file() {
                    return Err(ShimError::ExecutableNotFound {
                        command: command.to_owned(),
                        path: entrypoint,
                    });
                }
                (
                    storage.runtime_executable(RuntimeKind::Node, &runtime.version),
                    vec![entrypoint],
                )
            } else {
                (
                    storage.node_tool_executable(&runtime.version, command),
                    Vec::new(),
                )
            }
        }
        ShimTarget::PluginCommand => unreachable!("plugin command returned early"),
    };
    if !executable.is_file() {
        return Err(ShimError::ExecutableNotFound {
            command: command.to_owned(),
            path: executable,
        });
    }

    Ok(ResolvedCommand {
        executable,
        arguments,
        runtime_root: Some(runtime.path.clone()),
        runtime: Some(runtime),
    })
}

fn project_plugin_tool_for_command(
    storage: &Storage,
    command: &str,
    resolution: &jolter_resolver::ProjectResolution,
) -> Result<Option<InstalledPluginTool>, ShimError> {
    let configured_providers = resolution
        .plugins
        .iter()
        .filter(|plugin| plugin.name.starts_with('@'))
        .map(|plugin| plugin.name.as_str())
        .collect::<BTreeSet<_>>();
    for requirement in &resolution.plugin_tools {
        let Some(tool) = storage
            .installed_plugin_tools()?
            .into_iter()
            .rev()
            .find(|tool| {
                tool.tool == requirement.name
                    && (configured_providers.is_empty()
                        || configured_providers.contains(tool.provider.as_str()))
                    && tool.commands.iter().any(|candidate| candidate == command)
                    && tool.is_complete()
            })
        else {
            if plugin_command_matches_tool(storage, command, &requirement.name)? {
                return Err(ShimError::PluginToolNotInstalled(command.to_owned()));
            }
            continue;
        };
        return Ok(Some(tool));
    }
    Ok(None)
}

fn active_plugin_tool_for_command(
    storage: &Storage,
    command: &str,
) -> Result<Option<InstalledPluginTool>, ShimError> {
    Ok(storage.active_plugin_tools()?.into_iter().find(|tool| {
        tool.commands.iter().any(|candidate| candidate == command) && tool.is_complete()
    }))
}

fn active_tool_for_command(
    storage: &Storage,
    command: &str,
) -> Result<Option<InstalledTool>, ShimError> {
    let Some(kind) = ToolKind::ALL
        .into_iter()
        .find(|kind| kind.entrypoint(command).is_some())
    else {
        return Ok(None);
    };
    let version = if let Some(v) = storage.active_tool_version(kind)? {
        Some(v)
    } else {
        let tools = storage.installed_tools()?;
        tools
            .into_iter()
            .filter(|t| t.kind == kind && t.is_complete())
            .max_by(|a, b| a.version.cmp(&b.version))
            .map(|t| t.version)
    };
    let Some(version) = version else {
        return Ok(None);
    };
    let path = storage.tool_version_dir(kind, &version);
    let tool = InstalledTool {
        kind,
        version,
        path,
    };
    if !tool.is_complete() {
        return Err(ShimError::ActiveToolMissing {
            kind: tool.kind,
            version: tool.version,
            path: tool.path,
        });
    }
    Ok(Some(tool))
}

pub(crate) fn active_runtime(
    storage: &Storage,
    kind: RuntimeKind,
) -> Result<InstalledRuntime, ShimError> {
    let version = if let Some(v) = storage.active_version(kind)? {
        v
    } else {
        let installed = storage.installed_runtimes()?;
        let highest = installed
            .into_iter()
            .filter(|r| r.kind == kind && r.is_complete())
            .max_by(|a, b| a.version.cmp(&b.version));
        match highest {
            Some(r) => r.version,
            None => return Err(ShimError::NoActiveRuntime(kind)),
        }
    };
    let path = storage.runtime_version_dir(kind, &version);
    let executable = storage.runtime_executable(kind, &version);
    if !executable.is_file() {
        return Err(ShimError::ActiveRuntimeMissing {
            kind,
            version,
            path,
        });
    }
    Ok(InstalledRuntime {
        kind,
        version,
        path,
    })
}

fn plugin_shim_commands(storage: &Storage) -> Result<Vec<String>, ShimError> {
    if !storage.plugins_dir().exists() {
        return Ok(Vec::new());
    }
    let installed = storage.installed_plugins()?;
    if installed.is_empty() {
        return Ok(Vec::new());
    }
    let mut commands = Vec::new();
    for plugin in installed {
        let path = plugin.path.join(".jolter-plugin.json");
        if !path.is_file() {
            continue;
        }
        let contents =
            fs::read_to_string(&path).map_err(|source| ShimError::ReadPluginManifest {
                path: path.clone(),
                source,
            })?;
        let manifest: PluginShimManifest =
            serde_json::from_str(&contents).map_err(|source| ShimError::ParsePluginManifest {
                path: path.clone(),
                source,
            })?;
        let mut plugin_commands = manifest.commands;
        for tool in manifest.provides.tools.values() {
            plugin_commands.extend(tool.commands.clone());
        }
        commands.extend(
            plugin_commands
                .into_iter()
                .filter(|command| valid_plugin_command(command)),
        );
    }
    Ok(commands)
}

fn plugin_command_exists(storage: &Storage, command: &str) -> bool {
    plugin_shim_commands(storage)
        .is_ok_and(|commands| commands.iter().any(|candidate| candidate == command))
}

fn plugin_command_matches_tool(
    storage: &Storage,
    command: &str,
    tool: &str,
) -> Result<bool, ShimError> {
    if !storage.plugins_dir().exists() {
        return Ok(false);
    }
    let installed = storage.installed_plugins()?;
    if installed.is_empty() {
        return Ok(false);
    }
    for plugin in installed {
        let path = plugin.path.join(".jolter-plugin.json");
        if !path.is_file() {
            continue;
        }
        let contents =
            fs::read_to_string(&path).map_err(|source| ShimError::ReadPluginManifest {
                path: path.clone(),
                source,
            })?;
        let manifest: PluginShimManifest =
            serde_json::from_str(&contents).map_err(|source| ShimError::ParsePluginManifest {
                path: path.clone(),
                source,
            })?;
        if manifest.provides.tools.get(tool).is_some_and(|definition| {
            definition
                .commands
                .iter()
                .any(|candidate| candidate == command)
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn valid_plugin_command(command: &str) -> bool {
    !command.is_empty()
        && command.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
}

#[derive(Debug, Deserialize)]
struct PluginShimManifest {
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    provides: PluginShimProvides,
}

#[derive(Debug, Default, Deserialize)]
struct PluginShimProvides {
    #[serde(default)]
    tools: std::collections::BTreeMap<String, PluginShimTool>,
}

#[derive(Debug, Default, Deserialize)]
struct PluginShimTool {
    #[serde(default)]
    commands: Vec<String>,
}
