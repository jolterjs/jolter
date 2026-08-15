use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use jolter_runtime::{RuntimeKind, ToolKind};
use jolter_storage::Storage;

use crate::error::ShimError;

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

#[cfg(windows)]
fn shim_file_name(command: &str) -> String {
    format!("{command}.exe")
}

#[cfg(not(windows))]
fn shim_file_name(command: &str) -> String {
    command.to_owned()
}
