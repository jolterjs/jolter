use semver::Version;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] jolter_core::CoreError),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
    #[error("no active {0} version; pass an explicit selector such as {0}@latest")]
    NoActiveUpdateTarget(String),
    #[error("pass a plugin name or use `jolter plugin update --all`")]
    PluginUpdateTargetRequired,
    #[error("`jolter use` without a target requires an interactive terminal")]
    InteractiveUseRequiresTty,
    #[error("interactive selection failed: {0}")]
    InteractivePrompt(String),
    #[error("interactive selection produced an invalid target: {0}")]
    InteractiveSelection(String),
    #[error("plugin tool `{0}@{1}` is not installed")]
    PluginToolUninstallTargetMissing(String, Version),
    #[error(
        "plugin tool `{0}@{1}` is installed from multiple providers; remove by pruning or uninstall one provider first"
    )]
    PluginToolUninstallTargetAmbiguous(String, Version),
    #[error("invalid update request: {0}")]
    UpdateRequest(String),
    #[error("failed to determine the current directory: {0}")]
    CurrentDirectory(#[source] std::io::Error),
    #[error("failed to determine the Jolter executable: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("Jolter shim executable was not found at {0}")]
    ShimExecutableMissing(PathBuf),
    #[error("failed to serialize command output: {0}")]
    Json(#[source] serde_json::Error),
    #[error("failed to update {variable} file {path}: {source}")]
    CiEnvironment {
        variable: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
