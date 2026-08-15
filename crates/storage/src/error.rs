use std::path::PathBuf;

use jolter_runtime::{RuntimeKind, ToolKind};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("could not determine the user home directory; set JOLTER_HOME explicitly")]
    HomeDirectoryUnavailable,
    #[error("failed to create storage directory {path}: {source}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read storage directory {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read storage file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write storage file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid active toolchain configuration at {path}: {source}")]
    ParseActive {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize active toolchain configuration: {0}")]
    SerializeActive(#[source] serde_json::Error),
    #[error("active {kind} version `{value}` is invalid: {source}")]
    InvalidActiveVersion {
        kind: RuntimeKind,
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("active {kind} tool version `{value}` is invalid: {source}")]
    InvalidActiveToolVersion {
        kind: ToolKind,
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("active plugin tool {tool} version `{value}` is invalid: {source}")]
    InvalidActivePluginToolVersion {
        tool: String,
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("invalid storage path {path}")]
    InvalidPath { path: PathBuf },
}
