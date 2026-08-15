use std::{collections::BTreeMap, fs, path::Path};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::error::PluginError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginReleaseManifest {
    #[serde(default, rename = "$schema")]
    pub schema_url: Option<String>,
    pub schema_version: u32,
    pub name: String,
    pub version: String,
    pub jolter: JolterApiRequirement,
    pub entrypoint: PluginEntrypoint,
    pub provides: PluginProvides,
    #[serde(default)]
    pub permissions: PluginPermissions,
    pub artifacts: PluginArtifacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JolterApiRequirement {
    pub minimum_version: String,
    pub api_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEntrypoint {
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginProvides {
    #[serde(default)]
    pub tools: BTreeMap<String, PluginToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolDefinition {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub network: Option<NetworkPermissions>,
    #[serde(default)]
    pub filesystem: Option<FilesystemPermissions>,
    #[serde(default)]
    pub commands: Option<CommandPermissions>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPermissions {
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemPermissions {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandPermissions {
    #[serde(default)]
    pub execute: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginArtifacts {
    pub wasm: WasmArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmArtifact {
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPluginManifest {
    pub canonical_name: String,
    pub requested_name: String,
    pub version: String,
    pub registry_url: String,
    pub wasm_sha256: String,
    pub commands: Vec<String>,
    pub provides: PluginProvides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTool {
    pub name: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPlatform {
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginToolRelease {
    pub version: Version,
    pub url: String,
    pub sha256: String,
    pub archive_format: String,
    pub strip_components: usize,
    pub commands: Vec<String>,
}

pub fn read_installed_manifest(path: &Path) -> Result<InstalledPluginManifest, PluginError> {
    let contents = fs::read_to_string(path.join(".jolter-plugin.json")).map_err(PluginError::Io)?;
    serde_json::from_str(&contents).map_err(PluginError::Json)
}

#[must_use]
pub fn commands_from_provides(provides: &PluginProvides) -> Vec<String> {
    let mut commands = provides
        .tools
        .values()
        .flat_map(|tool| tool.commands.iter().cloned())
        .collect::<Vec<_>>();
    commands.sort();
    commands.dedup();
    commands
}
