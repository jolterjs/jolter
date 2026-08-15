use std::{env, path::PathBuf};

use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use semver::Version;
use thiserror::Error;

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
