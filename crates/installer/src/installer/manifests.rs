use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::{
    error::InstallerError,
    types::{ArchiveFormat, ArtifactIntegrity, PluginToolArchive, Release, ToolRelease},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallManifest<'a> {
    runtime: String,
    version: String,
    artifact_url: &'a str,
    integrity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolInstallManifest<'a> {
    tool: String,
    version: String,
    artifact_url: &'a str,
    integrity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_engine: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_hash: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginToolInstallManifest<'a> {
    provider: &'a str,
    tool: &'a str,
    version: String,
    url: &'a str,
    sha256: &'a str,
    archive_format: &'static str,
    strip_components: usize,
    commands: &'a [String],
}

pub fn write_manifest(destination: &Path, release: &Release) -> Result<(), InstallerError> {
    let manifest = InstallManifest {
        runtime: release.kind.to_string(),
        version: release.version.to_string(),
        artifact_url: &release.artifact.url,
        integrity: release.artifact.integrity.to_string(),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(InstallerError::Manifest)?;
    fs::write(destination.join(".jolter-install.json"), contents).map_err(InstallerError::Io)
}

pub fn write_tool_manifest(
    destination: &Path,
    release: &ToolRelease,
) -> Result<(), InstallerError> {
    let manifest = ToolInstallManifest {
        tool: release.kind.to_string(),
        version: release.version.to_string(),
        artifact_url: &release.artifact.url,
        integrity: release.artifact.integrity.to_string(),
        node_engine: release.node_engine.as_deref(),
        tool_hash: release.expected_hash.as_ref().map(ToString::to_string),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(InstallerError::Manifest)?;
    fs::write(destination.join(".jolter-tool.json"), contents).map_err(InstallerError::Io)
}

pub fn write_plugin_tool_manifest(
    destination: &Path,
    release: &PluginToolArchive,
) -> Result<(), InstallerError> {
    let ArtifactIntegrity::Sha256(sha256) = &release.artifact.integrity else {
        return Err(InstallerError::UnsupportedIntegrity(
            release.artifact.integrity.to_string(),
        ));
    };
    let manifest = PluginToolInstallManifest {
        provider: &release.provider,
        tool: &release.tool,
        version: release.version.to_string(),
        url: &release.artifact.url,
        sha256,
        archive_format: match release.artifact.format {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::TarGz => "tar.gz",
        },
        strip_components: release.artifact.strip_components,
        commands: &release.commands,
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(InstallerError::Manifest)?;
    fs::write(destination.join(".jolter-plugin-tool.json"), contents).map_err(InstallerError::Io)
}
