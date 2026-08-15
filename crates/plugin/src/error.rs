use semver::Version;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("invalid plugin name `{0}`")]
    InvalidName(String),
    #[error("invalid plugin version selector `{0}`")]
    InvalidSelector(String),
    #[error("plugin version not found for {name}@{selector}")]
    VersionNotFound { name: String, selector: String },
    #[error("invalid plugin version `{value}`: {source}")]
    InvalidVersion {
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("invalid plugin tool version `{value}`: {source}")]
    InvalidPluginToolVersion {
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("plugin version {name}@{version} is yanked")]
    YankedVersion {
        name: String,
        version: Version,
        message: Option<String>,
    },
    #[error("plugin manifest schema version {0} is not supported")]
    UnsupportedSchema(u32),
    #[error(
        "plugin release manifest $schema `{found}` does not match schemaVersion; expected `{expected}`"
    )]
    SchemaUrlMismatch { found: String, expected: String },
    #[error("plugin release manifest identity mismatch: expected {expected}, got {actual}")]
    ManifestIdentity { expected: String, actual: String },
    #[error("plugin release manifest version mismatch: expected {expected}, got {actual}")]
    ManifestVersion { expected: String, actual: String },
    #[error("plugin release manifest has an invalid WASM entrypoint")]
    InvalidEntrypoint,
    #[error("invalid plugin WASM sha256 `{0}`")]
    InvalidSha256(String),
    #[error("plugin command execution permissions are not supported")]
    CommandExecutionUnsupported,
    #[error("insecure plugin URL `{0}`")]
    InsecureUrl(String),
    #[error("invalid plugin URL `{0}`")]
    InvalidUrl(String),
    #[error("plugin manifest from {url} is too large")]
    ManifestTooLarge { url: String },
    #[error("plugin WASM from {url} is too large")]
    WasmTooLarge { url: String },
    #[error("plugin WASM size mismatch: expected {expected} bytes, got {actual}")]
    WasmSizeMismatch { expected: u64, actual: u64 },
    #[error("plugin WASM checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("failed to create plugin directory {path}: {source}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to remove plugin directory {path}: {source}")]
    Remove {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to publish plugin file at {path}: {source}")]
    Persist {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create HTTP client: {0}")]
    HttpClient(#[source] reqwest::Error),
    #[error("failed to execute plugin component: {0}")]
    Wasm(#[source] wasmtime::Error),
    #[error("HTTP request failed for {url}: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("invalid UTF-8 response from {url}: {source}")]
    InvalidUtf8 {
        url: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("invalid plugin JSON from {url}: {source}")]
    ManifestJson {
        url: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
}
