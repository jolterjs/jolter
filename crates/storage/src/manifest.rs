use std::{fs, path::Path};

use serde::Deserialize;

use crate::{error::StorageError, types::InstalledPluginTool};

#[derive(Debug, Deserialize)]
pub(crate) struct PluginToolInstallManifest {
    #[serde(default)]
    pub commands: Vec<String>,
}

pub(crate) fn read_installed_plugin_tool_versions(
    provider: &str,
    tool: &str,
    root: &Path,
) -> Result<Vec<InstalledPluginTool>, StorageError> {
    let mut installed = Vec::new();
    for entry in fs::read_dir(root).map_err(|source| StorageError::Read {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::Read {
            path: root.to_path_buf(),
            source,
        })?;
        if !entry
            .file_type()
            .map_err(|source| StorageError::Read {
                path: entry.path(),
                source,
            })?
            .is_dir()
        {
            continue;
        }
        let version_name = entry.file_name().to_string_lossy().into_owned();
        let Ok(version) = semver::Version::parse(version_name.trim_start_matches('v')) else {
            continue;
        };
        let path = entry.path();
        let commands = read_plugin_tool_commands(&path).unwrap_or_default();
        installed.push(InstalledPluginTool {
            provider: provider.to_owned(),
            tool: tool.to_owned(),
            version,
            path,
            commands,
        });
    }
    Ok(installed)
}

pub(crate) fn read_plugin_tool_commands(path: &Path) -> Result<Vec<String>, StorageError> {
    let manifest_path = path.join(".jolter-plugin-tool.json");
    let contents = fs::read_to_string(&manifest_path).map_err(|source| StorageError::ReadFile {
        path: manifest_path.clone(),
        source,
    })?;
    let manifest: PluginToolInstallManifest =
        serde_json::from_str(&contents).map_err(|source| StorageError::ParseActive {
            path: manifest_path,
            source,
        })?;
    Ok(manifest.commands)
}
