use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read configuration at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid configuration at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize configuration: {source}")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write configuration at {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("only one runtime may be configured per project")]
    MultipleRuntimes,
    #[error(
        "unsupported jolter.json schema version {found}; this Jolter release supports version {supported}"
    )]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error("jolter.json $schema `{found}` does not match schemaVersion; expected `{expected}`")]
    SchemaUrlMismatch { found: String, expected: String },
    #[error("selector for `{0}` cannot be empty")]
    EmptySelector(String),
    #[error("invalid plugin name `{0}`")]
    InvalidPluginName(String),
    #[error("invalid plugin tool name `{0}`")]
    InvalidPluginToolName(String),
    #[error("invalid plugin selector `{selector}` for `{name}`")]
    InvalidPluginSelector { name: String, selector: String },
    #[error("invalid plugin tool selector `{selector}` for `{name}`")]
    InvalidPluginToolSelector { name: String, selector: String },
    #[error(transparent)]
    Tool(#[from] jolter_runtime::ToolRequestError),
    #[error(transparent)]
    Runtime(#[from] jolter_runtime::RuntimeRequestError),
    #[error("invalid configuration path {path}")]
    InvalidPath { path: PathBuf },
}
