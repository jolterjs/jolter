use std::{io, path::PathBuf};

use jolter_runtime::{RuntimeRequest, ToolHashAlgorithm, ToolKind, ToolRequest};
use semver::Version;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallerError {
    #[error("refusing non-HTTPS URL `{0}`")]
    InsecureUrl(String),
    #[error("invalid URL `{0}`")]
    InvalidUrl(String),
    #[error("invalid artifact file name `{0}`")]
    InvalidArtifactName(String),
    #[error("SHA256 checksum `{0}` must contain exactly 64 hexadecimal characters")]
    InvalidChecksum(String),
    #[error("invalid artifact integrity value `{0}`")]
    InvalidIntegrity(String),
    #[error("unsupported artifact integrity `{0}`; expected SHA-512 SRI")]
    UnsupportedIntegrity(String),
    #[error("artifact checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("{algorithm} tool hash mismatch: expected {expected}, got {actual}")]
    ToolHashMismatch {
        algorithm: ToolHashAlgorithm,
        expected: String,
        actual: String,
    },
    #[error("checksum metadata did not contain an entry for `{file}`")]
    ChecksumNotFound { file: String },
    #[error("release did not contain checksum metadata for `{asset}`")]
    ChecksumAssetNotFound { asset: String },
    #[error("checksum metadata was empty")]
    EmptyChecksum,
    #[error("HTTP client initialization failed: {0}")]
    HttpClient(#[source] reqwest::Error),
    #[error("request to {url} failed: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("metadata response from {url} exceeded the size limit")]
    MetadataTooLarge { url: String },
    #[error("metadata cache at {path} could not be read")]
    MetadataCacheRead { path: PathBuf },
    #[error("offline mode is enabled and no cached metadata exists for {url}")]
    OfflineCacheMiss { url: String },
    #[error("artifact response from {url} exceeded the size limit")]
    ArtifactTooLarge { url: String },
    #[error("metadata response from {url} was not UTF-8: {source}")]
    InvalidUtf8 {
        url: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("invalid release metadata from {url}: {source}")]
    MetadataJson {
        url: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to read installed package metadata at {path}: {source}")]
    InstalledPackageMetadataRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid installed package metadata at {path}: {source}")]
    InstalledPackageMetadataParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{tool}@{version} has invalid Node.js engine range `{requirement}`: {details}")]
    InvalidNodeEngineRange {
        tool: ToolKind,
        version: Version,
        requirement: String,
        details: String,
    },
    #[error(
        "{tool}@{version} requires Node.js `{requirement}`, but node@{node_version} was selected"
    )]
    IncompatibleNodeVersion {
        tool: ToolKind,
        version: Version,
        requirement: String,
        node_version: Version,
    },
    #[error("no stable release satisfies {0}")]
    VersionNotFound(RuntimeRequest),
    #[error("no stable tool release satisfies {0}")]
    ToolVersionNotFound(ToolRequest),
    #[error("release {version} does not provide required asset `{asset}`")]
    AssetNotFound { version: Version, asset: String },
    #[error("unsupported operating system `{0}`")]
    UnsupportedOperatingSystem(String),
    #[error("unsupported architecture `{0}`")]
    UnsupportedArchitecture(String),
    #[error("Bun x64 requires SSE4.2; this CPU is not supported by Bun baseline builds")]
    UnsupportedBunCpu,
    #[error("existing runtime installation at {path} is incomplete")]
    CorruptInstallation { path: PathBuf },
    #[error("existing tool installation at {path} is incomplete")]
    CorruptToolInstallation { path: PathBuf },
    #[error("existing plugin tool installation at {path} is incomplete")]
    CorruptPluginToolInstallation { path: PathBuf },
    #[error("runtime installation was not found at {path}")]
    RuntimeNotInstalled { path: PathBuf },
    #[error("tool installation was not found at {path}")]
    ToolNotInstalled { path: PathBuf },
    #[error("no entrypoint is defined for tool {0}")]
    MissingToolEntrypoint(ToolKind),
    #[error("refusing to remove runtime path outside its expected parent: {path}")]
    UnsafeRemoval { path: PathBuf },
    #[error("failed to remove incomplete runtime installation at {path}: {source}")]
    RemoveCorrupt {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to remove installation at {path}: {source}")]
    RemoveInstallation {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to clean cache directory {path}: {source}")]
    CacheCleanup {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("installed archive did not contain expected executable {path}")]
    ExecutableMissing { path: PathBuf },
    #[error("refusing unsafe archive path `{0}`")]
    UnsafeArchivePath(String),
    #[error("unsupported archive entry `{0}`")]
    UnsupportedArchiveEntry(String),
    #[error("archive exceeded the extracted size limit")]
    ArchiveSizeLimit,
    #[error("archive exceeded the entry count limit")]
    ArchiveEntryLimit,
    #[error("failed to publish runtime installation at {path}: {source}")]
    Publish {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("installer I/O error: {0}")]
    Io(#[source] io::Error),
    #[error("invalid ZIP archive: {0}")]
    Zip(#[source] zip::result::ZipError),
    #[error("failed to serialize installation manifest: {0}")]
    Manifest(#[source] serde_json::Error),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
}
