use std::time::Duration;

pub mod archive;
pub mod error;
pub mod http;
pub mod installer;
pub mod progress;
pub mod providers;
pub mod types;

#[cfg(test)]
mod tests;

pub use error::InstallerError;
pub use http::{HttpClient, ReqwestHttpClient, verify_sha256};
pub use installer::Installer;
pub use progress::{NoProgressReporter, ProgressAction, ProgressEvent, ProgressReporter};
pub use providers::github::GithubRuntime;
pub use types::{
    Architecture, ArchiveFormat, Artifact, ArtifactIntegrity, BunCpu, CacheCleanOutcome,
    InstallOutcome, OperatingSystem, Platform, PluginToolArchive, PluginToolInstallOutcome,
    Release, ReleaseChannel, RemovalOutcome, SelfUpgradeOutcome, ToolInstallOutcome, ToolRelease,
};

pub(crate) use jolter_runtime::{RuntimeRequest, ToolRequest};

pub(crate) const NODE_INDEX_URL: &str = "https://nodejs.org/dist/index.json";
pub(crate) const GITHUB_API: &str = "https://api.github.com";
pub(crate) const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_ENTRIES: usize = 100_000;
pub(crate) const MAX_METADATA_CACHE_AGE_SECS: Duration = Duration::from_secs(60 * 60);
pub(crate) const MAX_HTTP_ATTEMPTS: usize = 4;
pub(crate) const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);
