use jolter_runtime::{RuntimeRequestError, ToolRequestError};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolverError {
    #[error(transparent)]
    Config(#[from] jolter_config::ConfigError),
    #[error(transparent)]
    Runtime(#[from] RuntimeRequestError),
    #[error(transparent)]
    Tool(#[from] ToolRequestError),
    #[error("failed to resolve path {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid package.json at {path}: {source}")]
    PackageJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid packageManager value `{0}`; expected a value such as pnpm@10.0.0")]
    InvalidPackageManager(String),
}
