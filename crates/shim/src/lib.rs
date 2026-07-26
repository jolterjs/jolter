use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use jolter_resolver::resolve;
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use jolter_storage::{InstalledPluginTool, InstalledRuntime, InstalledTool, Storage};
use semver::Version;
use thiserror::Error;

pub const SHIM_COMMANDS: [&str; 7] = ["node", "npm", "npx", "pnpm", "yarn", "bun", "deno"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimTarget {
    Runtime(RuntimeKind),
    NodeTool(&'static str),
    PluginCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    pub executable: PathBuf,
    pub arguments: Vec<PathBuf>,
    pub runtime_root: Option<PathBuf>,
    pub runtime: Option<InstalledRuntime>,
}

#[must_use]
pub fn target_for_command(command: &str) -> Option<ShimTarget> {
    match command {
        "node" => Some(ShimTarget::Runtime(RuntimeKind::Node)),
        "bun" => Some(ShimTarget::Runtime(RuntimeKind::Bun)),
        "deno" => Some(ShimTarget::Runtime(RuntimeKind::Deno)),
        "npm" => Some(ShimTarget::NodeTool("npm")),
        "npx" => Some(ShimTarget::NodeTool("npx")),
        "pnpm" => Some(ShimTarget::NodeTool("pnpm")),
        "yarn" => Some(ShimTarget::NodeTool("yarn")),
        _ => None,
    }
}

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

#[allow(clippy::similar_names)]
pub fn desired_shim_commands(storage: &Storage) -> Result<BTreeSet<String>, ShimError> {
    let mut commands = BTreeSet::new();

    let runtimes = storage.installed_runtimes()?;
    let has_node = runtimes
        .iter()
        .any(|runtime| runtime.kind == RuntimeKind::Node && runtime.is_complete());
    let has_bun = runtimes
        .iter()
        .any(|runtime| runtime.kind == RuntimeKind::Bun && runtime.is_complete());
    let has_deno = runtimes
        .iter()
        .any(|runtime| runtime.kind == RuntimeKind::Deno && runtime.is_complete());

    if has_node {
        commands.insert("node".to_string());
    }
    if has_bun {
        commands.insert("bun".to_string());
    }
    if has_deno {
        commands.insert("deno".to_string());
    }

    let tools = storage.installed_tools()?;
    let has_npm_tool = tools
        .iter()
        .any(|tool| tool.kind == ToolKind::Npm && tool.is_complete());
    let has_pnpm_tool = tools
        .iter()
        .any(|tool| tool.kind == ToolKind::Pnpm && tool.is_complete());
    let has_yarn_tool = tools
        .iter()
        .any(|tool| tool.kind == ToolKind::Yarn && tool.is_complete());

    if has_node || has_npm_tool {
        commands.insert("npm".to_string());
        commands.insert("npx".to_string());
    }
    if has_pnpm_tool {
        commands.insert("pnpm".to_string());
    }
    if has_yarn_tool {
        commands.insert("yarn".to_string());
    }

    for plugin_tool in storage.installed_plugin_tools()? {
        if plugin_tool.is_complete() {
            for command in &plugin_tool.commands {
                commands.insert(command.clone());
            }
        }
    }

    Ok(commands)
}

pub fn install_shims(
    source_executable: &Path,
    storage: &Storage,
) -> Result<Vec<PathBuf>, ShimError> {
    if !source_executable.is_file() {
        return Err(ShimError::SourceExecutableMissing(
            source_executable.to_path_buf(),
        ));
    }
    storage.ensure_layout()?;
    let desired = desired_shim_commands(storage)?;
    let mut installed = Vec::with_capacity(desired.len());

    for command in &desired {
        let file_name = shim_file_name(command);
        let destination = storage.shims_dir().join(&file_name);
        let temporary = storage
            .shims_dir()
            .join(format!(".{file_name}.{}.tmp", std::process::id()));
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|source| ShimError::Write {
                path: temporary.clone(),
                source,
            })?;
        }
        if fs::hard_link(source_executable, &temporary).is_err() {
            fs::copy(source_executable, &temporary).map_err(|source| ShimError::Write {
                path: temporary.clone(),
                source,
            })?;
        }
        let mut replace_failed = false;
        if destination.exists() && fs::remove_file(&destination).is_err() {
            replace_failed = true;
        }
        if replace_failed {
            let _ = fs::remove_file(&temporary);
        } else if let Err(source) = fs::rename(&temporary, &destination) {
            if destination.exists() {
                let _ = fs::remove_file(&temporary);
            } else {
                return Err(ShimError::Write {
                    path: destination.clone(),
                    source,
                });
            }
        }
        installed.push(destination);
    }

    if let Ok(entries) = fs::read_dir(storage.shims_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.starts_with('.') {
                    continue;
                }
                if !desired.contains(stem) {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }

    Ok(installed)
}

#[must_use]
pub fn invoked_command_name() -> Option<String> {
    env::args_os()
        .next()
        .and_then(|argument| {
            Path::new(&argument)
                .file_stem()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
        })
        .filter(|name| !name.is_empty())
}

pub fn run_invoked_command() -> Result<ExitCode, ShimError> {
    let command = invoked_command_name()
        .filter(|name| name != "jolter-shim")
        .ok_or_else(|| ShimError::UnsupportedCommand("jolter-shim".to_owned()))?;
    run_command(&command)
}

pub fn run_command(command_name: &str) -> Result<ExitCode, ShimError> {
    let storage = Storage::discover()?;
    let current_dir = env::current_dir().map_err(ShimError::CurrentDirectory)?;
    let resolved = resolve_command(command_name, &current_dir, &storage)?;
    let mut command = Command::new(&resolved.executable);
    command.args(&resolved.arguments);
    command.args(env::args_os().skip(1));
    if let (Some(runtime), Some(runtime_root)) = (&resolved.runtime, &resolved.runtime_root) {
        prepend_runtime_path(&mut command, runtime.kind, runtime_root)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        Err(ShimError::Launch {
            path: resolved.executable,
            source: error,
        })
    }
    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|source| ShimError::Launch {
            path: resolved.executable,
            source,
        })?;
        Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from))
    }
}

fn prepend_runtime_path(
    command: &mut Command,
    kind: RuntimeKind,
    runtime_root: &Path,
) -> Result<(), ShimError> {
    let binary_directory = if kind == RuntimeKind::Node && !cfg!(windows) {
        runtime_root.join("bin")
    } else {
        runtime_root.to_path_buf()
    };
    let mut paths = vec![binary_directory];
    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }
    let path = env::join_paths(paths).map_err(ShimError::JoinPath)?;
    command.env("PATH", path);
    command.env("JOLTER_RUNTIME_ROOT", runtime_root);
    Ok(())
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
        commands.extend(
            manifest
                .commands
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

#[derive(Debug, serde::Deserialize)]
struct PluginShimManifest {
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    provides: PluginShimProvides,
}

#[derive(Debug, Default, serde::Deserialize)]
struct PluginShimProvides {
    #[serde(default)]
    tools: std::collections::BTreeMap<String, PluginShimTool>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct PluginShimTool {
    #[serde(default)]
    commands: Vec<String>,
}

fn active_runtime(storage: &Storage, kind: RuntimeKind) -> Result<InstalledRuntime, ShimError> {
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

#[cfg(windows)]
fn shim_file_name(command: &str) -> String {
    format!("{command}.exe")
}

#[cfg(not(windows))]
fn shim_file_name(command: &str) -> String {
    command.to_owned()
}

#[derive(Debug, Error)]
pub enum ShimError {
    #[error("unsupported shim command `{0}`")]
    UnsupportedCommand(String),
    #[error(
        "plugin command `{0}` is known, but its tool artifact is not installed yet; run `jolter sync --yes`"
    )]
    PluginToolNotInstalled(String),
    #[error("runtime required by the project is not installed: {0}")]
    RuntimeNotInstalled(RuntimeRequest),
    #[error("tool required by the project is not installed: {0}")]
    ToolNotInstalled(ToolRequest),
    #[error("no active {0} runtime; run `jolter use {0}@<version>`")]
    NoActiveRuntime(RuntimeKind),
    #[error("active {kind}@{version} runtime is missing from {path}")]
    ActiveRuntimeMissing {
        kind: RuntimeKind,
        version: Version,
        path: PathBuf,
    },
    #[error("active {kind}@{version} tool is missing from {path}")]
    ActiveToolMissing {
        kind: ToolKind,
        version: Version,
        path: PathBuf,
    },
    #[error("command `{command}` was not found at {path}")]
    ExecutableNotFound { command: String, path: PathBuf },
    #[error("Jolter executable was not found at {0}")]
    SourceExecutableMissing(PathBuf),
    #[error("failed to determine the current directory: {0}")]
    CurrentDirectory(#[source] std::io::Error),
    #[error("failed to launch {path}: {source}")]
    Launch {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to construct runtime PATH: {0}")]
    JoinPath(#[source] env::JoinPathsError),
    #[error("failed to write shim at {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Resolver(#[from] jolter_resolver::ResolverError),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
    #[error("failed to read plugin manifest at {path}: {source}")]
    ReadPluginManifest {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse plugin manifest at {path}: {source}")]
    ParsePluginManifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_commands() {
        assert_eq!(
            target_for_command("node"),
            Some(ShimTarget::Runtime(RuntimeKind::Node))
        );
        assert_eq!(target_for_command("npm"), Some(ShimTarget::NodeTool("npm")));
        assert_eq!(target_for_command("ruby"), None);
    }

    #[test]
    fn resolves_project_runtime_before_active_runtime() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        fs::write(
            project.path().join("jolter.json"),
            r#"{"runtime":{"node":"24"}}"#,
        )
        .unwrap();
        let project_executable =
            storage.runtime_executable(RuntimeKind::Node, &Version::new(24, 1, 0));
        fs::create_dir_all(project_executable.parent().unwrap()).unwrap();
        fs::write(project_executable, b"node").unwrap();
        let active_executable =
            storage.runtime_executable(RuntimeKind::Node, &Version::new(22, 1, 0));
        fs::create_dir_all(active_executable.parent().unwrap()).unwrap();
        fs::write(active_executable, b"node").unwrap();
        storage
            .activate(RuntimeKind::Node, &Version::new(22, 1, 0))
            .unwrap();

        let resolved = resolve_command("node", project.path(), &storage).unwrap();

        assert_eq!(resolved.runtime.unwrap().version, Version::new(24, 1, 0));
        assert!(resolved.arguments.is_empty());
    }

    #[test]
    fn resolves_managed_tool_through_project_node() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        fs::write(
            project.path().join("jolter.json"),
            r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
        )
        .unwrap();
        let node_version = Version::new(24, 1, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, b"node").unwrap();
        let pnpm_version = Version::new(10, 2, 0);
        let pnpm = storage
            .tool_entrypoint(jolter_runtime::ToolKind::Pnpm, &pnpm_version, "pnpm")
            .unwrap();
        fs::create_dir_all(pnpm.parent().unwrap()).unwrap();
        fs::write(&pnpm, b"pnpm").unwrap();

        let resolved = resolve_command("pnpm", project.path(), &storage).unwrap();

        assert_eq!(resolved.executable, node);
        assert_eq!(resolved.arguments, vec![pnpm]);
    }

    #[test]
    fn resolves_active_tool_through_active_node() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        let node_version = Version::new(24, 1, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, b"node").unwrap();
        storage.activate(RuntimeKind::Node, &node_version).unwrap();
        let pnpm_version = Version::new(10, 2, 0);
        let pnpm = storage
            .tool_entrypoint(ToolKind::Pnpm, &pnpm_version, "pnpm")
            .unwrap();
        fs::create_dir_all(pnpm.parent().unwrap()).unwrap();
        fs::write(&pnpm, b"pnpm").unwrap();
        storage
            .activate_tool(ToolKind::Pnpm, &pnpm_version)
            .unwrap();

        let resolved = resolve_command("pnpm", project.path(), &storage).unwrap();

        assert_eq!(resolved.executable, node);
        assert_eq!(resolved.arguments, vec![pnpm]);
    }

    #[test]
    fn project_tool_overrides_the_active_version() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        fs::write(
            project.path().join("jolter.json"),
            r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
        )
        .unwrap();
        let node_version = Version::new(24, 1, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(node, b"node").unwrap();
        let active_version = Version::new(9, 1, 0);
        let active = storage
            .tool_entrypoint(ToolKind::Pnpm, &active_version, "pnpm")
            .unwrap();
        fs::create_dir_all(active.parent().unwrap()).unwrap();
        fs::write(active, b"pnpm").unwrap();
        storage
            .activate_tool(ToolKind::Pnpm, &active_version)
            .unwrap();
        let project_version = Version::new(10, 2, 0);
        let project_pnpm = storage
            .tool_entrypoint(ToolKind::Pnpm, &project_version, "pnpm")
            .unwrap();
        fs::create_dir_all(project_pnpm.parent().unwrap()).unwrap();
        fs::write(&project_pnpm, b"pnpm").unwrap();

        let resolved = resolve_command("pnpm", project.path(), &storage).unwrap();

        assert_eq!(resolved.arguments, vec![project_pnpm]);
    }

    #[test]
    fn resolves_active_runtime_and_bundled_node_tools() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        let version = Version::new(22, 4, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &version);
        let npm = storage.node_tool_executable(&version, "npm");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, b"node").unwrap();
        fs::write(&npm, b"npm").unwrap();
        storage.activate(RuntimeKind::Node, &version).unwrap();

        let resolved = resolve_command("npm", project.path(), &storage).unwrap();

        assert_eq!(resolved.executable, npm);
        assert!(resolved.arguments.is_empty());
        assert_eq!(resolved.runtime.unwrap().version, version);
    }

    #[test]
    fn reports_missing_active_runtime_and_project_tool() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();

        assert!(matches!(
            resolve_command("node", project.path(), &storage),
            Err(ShimError::NoActiveRuntime(RuntimeKind::Node))
        ));

        fs::write(
            project.path().join("jolter.json"),
            r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
        )
        .unwrap();
        let version = Version::new(24, 1, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &version);
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(node, b"node").unwrap();
        assert!(matches!(
            resolve_command("pnpm", project.path(), &storage),
            Err(ShimError::ToolNotInstalled(_))
        ));
    }

    #[test]
    fn reports_known_plugin_command_without_runtime_resolution() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        let manifest = storage
            .plugin_version_dir("@local/hello", &Version::new(1, 0, 0))
            .join(".jolter-plugin.json");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, r#"{"commands":["hello"]}"#).unwrap();

        assert!(matches!(
            resolve_command("hello", project.path(), &storage),
            Err(ShimError::PluginToolNotInstalled(command)) if command == "hello"
        ));
    }

    #[test]
    fn resolves_active_plugin_tool_command() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        write_plugin_manifest(&storage, "@local/hello");
        let executable = write_plugin_tool(&storage, "@local/hello", "hello", "hello");
        storage
            .activate_plugin_tool("@local/hello", "hello", &Version::new(1, 0, 0))
            .unwrap();

        let resolved = resolve_command("hello", project.path(), &storage).unwrap();

        assert_eq!(resolved.executable, executable);
        assert!(resolved.arguments.is_empty());
        assert!(resolved.runtime.is_none());
        assert!(resolved.runtime_root.is_none());
    }

    #[test]
    fn resolves_project_plugin_tool_before_active_plugin_tool() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        fs::write(
            project.path().join("jolter.json"),
            r#"{"schemaVersion":2,"tools":{"hello":"1"},"plugins":{"@local/hello":"1"}}"#,
        )
        .unwrap();
        write_plugin_manifest(&storage, "@local/hello");
        let project_executable = write_plugin_tool(&storage, "@local/hello", "hello", "hello");
        write_plugin_manifest(&storage, "@local/other");
        let _active_executable = write_plugin_tool(&storage, "@local/other", "hello", "hello");
        storage
            .activate_plugin_tool("@local/other", "hello", &Version::new(1, 0, 0))
            .unwrap();

        let resolved = resolve_command("hello", project.path(), &storage).unwrap();

        assert_eq!(resolved.executable, project_executable);
    }

    fn write_plugin_manifest(storage: &Storage, provider: &str) {
        let manifest = storage
            .plugin_version_dir(provider, &Version::new(1, 0, 0))
            .join(".jolter-plugin.json");
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(
            manifest,
            r#"{"commands":["hello"],"provides":{"tools":{"hello":{"commands":["hello"]}}}}"#,
        )
        .unwrap();
    }

    fn write_plugin_tool(storage: &Storage, provider: &str, tool: &str, command: &str) -> PathBuf {
        let root = storage.plugin_tool_version_dir(provider, tool, &Version::new(1, 0, 0));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".jolter-plugin-tool.json"),
            format!(r#"{{"commands":["{command}"]}}"#),
        )
        .unwrap();
        let executable = if cfg!(windows) {
            root.join(format!("{command}.exe"))
        } else {
            root.join(command)
        };
        fs::write(&executable, b"tool").unwrap();
        executable
    }

    #[test]
    fn installs_and_refreshes_shims_demand_driven() {
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join(if cfg!(windows) {
            "jolter.exe"
        } else {
            "jolter"
        });
        fs::write(&source, b"first").unwrap();

        // 1. Fresh storage has no installed runtimes or tools -> 0 shims created
        let installed = install_shims(&source, &storage).unwrap();
        assert!(installed.is_empty());

        // 2. Add Node.js runtime -> node, npm, npx shims created
        let node_dir = storage.runtime_version_dir(RuntimeKind::Node, &Version::new(24, 0, 0));
        fs::create_dir_all(&node_dir).unwrap();
        let node_bin = storage.runtime_executable(RuntimeKind::Node, &Version::new(24, 0, 0));
        fs::create_dir_all(node_bin.parent().unwrap()).unwrap();
        fs::write(&node_bin, b"node").unwrap();

        let installed = install_shims(&source, &storage).unwrap();
        let names = installed
            .iter()
            .map(|p| p.file_stem().unwrap().to_str().unwrap().to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from(["node".to_string(), "npm".to_string(), "npx".to_string()])
        );

        // 3. Add Bun runtime -> bun shim also created
        let bun_dir = storage.runtime_version_dir(RuntimeKind::Bun, &Version::new(1, 0, 0));
        fs::create_dir_all(&bun_dir).unwrap();
        let bun_bin = storage.runtime_executable(RuntimeKind::Bun, &Version::new(1, 0, 0));
        fs::create_dir_all(bun_bin.parent().unwrap()).unwrap();
        fs::write(&bun_bin, b"bun").unwrap();

        let installed = install_shims(&source, &storage).unwrap();
        let names = installed
            .iter()
            .map(|p| p.file_stem().unwrap().to_str().unwrap().to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "node".to_string(),
                "npm".to_string(),
                "npx".to_string(),
                "bun".to_string()
            ])
        );

        // 4. Remove Node.js -> node, npm, npx shims are pruned, only bun remains
        fs::remove_dir_all(&node_dir).unwrap();
        let installed = install_shims(&source, &storage).unwrap();
        let names = installed
            .iter()
            .map(|p| p.file_stem().unwrap().to_str().unwrap().to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(names, BTreeSet::from(["bun".to_string()]));
        assert!(!storage.shims_dir().join("node").exists());
        assert!(!storage.shims_dir().join("node.exe").exists());
    }
}
