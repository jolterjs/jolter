use std::{
    fs,
    path::{Path, PathBuf},
};

use jolter_resolver::resolve;
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use jolter_storage::{InstalledRuntime, InstalledTool, Storage};
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
    pub runtime_root: PathBuf,
    pub runtime: InstalledRuntime,
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
    let kind = match target {
        ShimTarget::Runtime(kind) => kind,
        ShimTarget::NodeTool(_) => RuntimeKind::Node,
        ShimTarget::PluginCommand => {
            return Err(ShimError::PluginToolNotInstalled(command.to_owned()));
        }
    };
    let project_resolution = resolve(project)?;
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
        ShimTarget::PluginCommand => {
            return Err(ShimError::PluginToolNotInstalled(command.to_owned()));
        }
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
        runtime_root: runtime.path.clone(),
        runtime,
    })
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
    let Some(version) = storage.active_tool_version(kind)? else {
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
    let plugin_commands = plugin_shim_commands(storage)?;
    let mut commands = SHIM_COMMANDS
        .iter()
        .map(|command| (*command).to_owned())
        .chain(plugin_commands)
        .collect::<Vec<_>>();
    commands.sort();
    commands.dedup();
    let mut installed = Vec::with_capacity(commands.len());
    for command in commands {
        let command = command.as_str();
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
        if destination.exists() {
            fs::remove_file(&destination).map_err(|source| ShimError::Write {
                path: destination.clone(),
                source,
            })?;
        }
        fs::rename(&temporary, &destination).map_err(|source| ShimError::Write {
            path: destination.clone(),
            source,
        })?;
        installed.push(destination);
    }
    Ok(installed)
}

fn plugin_shim_commands(storage: &Storage) -> Result<Vec<String>, ShimError> {
    let mut commands = Vec::new();
    for plugin in storage.installed_plugins()? {
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
}

fn active_runtime(storage: &Storage, kind: RuntimeKind) -> Result<InstalledRuntime, ShimError> {
    let version = storage
        .active_version(kind)?
        .ok_or(ShimError::NoActiveRuntime(kind))?;
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

        assert_eq!(resolved.runtime.version, Version::new(24, 1, 0));
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
        assert_eq!(resolved.runtime.version, version);
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
    fn installs_and_refreshes_every_shim() {
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join(if cfg!(windows) {
            "jolter.exe"
        } else {
            "jolter"
        });
        fs::write(&source, b"first").unwrap();

        let installed = install_shims(&source, &storage).unwrap();
        assert_eq!(installed.len(), SHIM_COMMANDS.len());
        assert!(installed.iter().all(|path| path.is_file()));

        fs::remove_file(&source).unwrap();
        fs::write(&source, b"second").unwrap();
        install_shims(&source, &storage).unwrap();
        assert_eq!(fs::read(&installed[0]).unwrap(), b"second");
        assert!(matches!(
            install_shims(&source_dir.path().join("missing"), &storage),
            Err(ShimError::SourceExecutableMissing(_))
        ));
    }
}
