use std::path::PathBuf;

use jolter_runtime::{RuntimeKind, ToolKind, ToolRequest};
use semver::Version;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("no runtime requirement was found from {0}")]
    NoRuntimeRequirement(PathBuf),
    #[error("tool {0} requires a Node.js runtime")]
    ToolRequiresNode(ToolRequest),
    #[error("tool {0} requires an active Node.js runtime; run `jolter use node@<version>` first")]
    ToolRequiresActiveNode(ToolRequest),
    #[error("active node@{version} runtime is missing from {path}")]
    ActiveNodeRuntimeMissing { version: Version, path: PathBuf },
    #[error(
        "refusing to uninstall active {kind}@{version}; activate another version or pass --force"
    )]
    ActiveRuntimeRemoval { kind: RuntimeKind, version: Version },
    #[error(
        "refusing to uninstall active {kind}@{version}; activate another version or pass --force"
    )]
    ActiveToolRemoval { kind: ToolKind, version: Version },
    #[error(
        "refusing to uninstall active plugin tool {tool}@{version} via {provider}; activate another version or pass --force"
    )]
    ActivePluginToolRemoval {
        provider: String,
        tool: String,
        version: Version,
    },
    #[error("no active plugin tool `{0}`; pass an explicit selector such as {0}@latest")]
    NoActivePluginTool(String),
    #[error("project requires plugin {name}@{selector}; rerun with `--yes` to install it")]
    MissingProjectPlugin { name: String, selector: String },
    #[error("no installed plugin provides tool `{0}`")]
    PluginToolProviderMissing(String),
    #[error(
        "multiple installed plugins provide tool `{tool}` ({providers}); declare one provider in jolter.json"
    )]
    AmbiguousPluginToolProvider { tool: String, providers: String },
    #[error("plugin tool archive format `{0}` is not supported")]
    UnsupportedPluginToolArchive(String),
    #[error("plugin `{provider}` reported invalid installation for `{tool}@{version}` at {path}")]
    InvalidPluginToolInstallation {
        provider: String,
        tool: String,
        version: Version,
        path: PathBuf,
    },
    #[error(
        "direct use of plugin-provided tool `{0}` is not available yet; add it to jolter.json and run `jolter sync --yes`"
    )]
    DirectPluginToolUseUnsupported(String),
    #[error("plugin `{0}` is installed with active shim commands; pass --force to remove it")]
    ActivePluginRemoval(String),
    #[error("plugin `{0}` is not installed")]
    PluginNotInstalled(String),
    #[error("failed to remove plugin at {path}: {source}")]
    PluginRemoval {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Config(#[from] jolter_config::ConfigError),
    #[error(transparent)]
    Doctor(#[from] jolter_doctor::DoctorError),
    #[error(transparent)]
    Installer(#[from] jolter_installer::InstallerError),
    #[error(transparent)]
    Plugin(#[from] jolter_plugin::PluginError),
    #[error(transparent)]
    Resolver(#[from] jolter_resolver::ResolverError),
    #[error(transparent)]
    Shim(#[from] jolter_shim::ShimError),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
}
