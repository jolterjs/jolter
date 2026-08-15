use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use jolter_runtime::{RuntimeKind, ToolHash, ToolKind};
use jolter_storage::InstalledRuntime;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    error::InstallerError,
    http::{ensure_https, validate_checksum, verify_sha256, verify_sha512},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub url: String,
    pub integrity: ArtifactIntegrity,
    pub file_name: String,
    pub format: ArchiveFormat,
    pub strip_components: usize,
}

impl Artifact {
    pub fn validate(&self) -> Result<(), InstallerError> {
        ensure_https(&self.url)?;
        self.integrity.validate()?;
        if self.file_name.is_empty()
            || Path::new(&self.file_name)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(InstallerError::InvalidArtifactName(self.file_name.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactIntegrity {
    Sha256(String),
    Sha512(String),
}

impl ArtifactIntegrity {
    pub(crate) fn from_sri(value: &str) -> Result<Self, InstallerError> {
        let Some(encoded) = value.strip_prefix("sha512-") else {
            return Err(InstallerError::UnsupportedIntegrity(value.to_owned()));
        };
        let decoded = BASE64
            .decode(encoded)
            .map_err(|_| InstallerError::InvalidIntegrity(value.to_owned()))?;
        if decoded.len() != 64 {
            return Err(InstallerError::InvalidIntegrity(value.to_owned()));
        }
        Ok(Self::Sha512(encoded.to_owned()))
    }

    pub(crate) fn validate(&self) -> Result<(), InstallerError> {
        match self {
            Self::Sha256(value) => validate_checksum(value),
            Self::Sha512(value) => {
                let decoded = BASE64
                    .decode(value)
                    .map_err(|_| InstallerError::InvalidIntegrity(format!("sha512-{value}")))?;
                if decoded.len() != 64 {
                    return Err(InstallerError::InvalidIntegrity(format!("sha512-{value}")));
                }
                Ok(())
            }
        }
    }

    pub(crate) fn cache_key(&self) -> String {
        format!("{:x}", Sha256::digest(self.to_string().as_bytes()))
    }

    pub(crate) fn verify(&self, path: &Path) -> Result<(), InstallerError> {
        match self {
            Self::Sha256(expected) => verify_sha256(path, expected),
            Self::Sha512(expected) => verify_sha512(path, expected),
        }
    }
}

impl fmt::Display for ArtifactIntegrity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sha256(value) => write!(formatter, "sha256-{value}"),
            Self::Sha512(value) => write!(formatter, "sha512-{value}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    TarGz,
}

impl ArchiveFormat {
    pub(crate) const fn cache_extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: OperatingSystem,
    pub arch: Architecture,
    pub bun_cpu: BunCpu,
}

impl Platform {
    pub fn current() -> Result<Self, InstallerError> {
        let os = match std::env::consts::OS {
            "windows" => OperatingSystem::Windows,
            "linux" => OperatingSystem::Linux,
            "macos" => OperatingSystem::MacOs,
            value => return Err(InstallerError::UnsupportedOperatingSystem(value.to_owned())),
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => Architecture::X64,
            "aarch64" => Architecture::Arm64,
            value => return Err(InstallerError::UnsupportedArchitecture(value.to_owned())),
        };
        Ok(Self {
            os,
            arch,
            bun_cpu: detect_bun_cpu(arch),
        })
    }
}

fn detect_bun_cpu(arch: Architecture) -> BunCpu {
    if arch == Architecture::Arm64 {
        return BunCpu::Standard;
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse4.2") {
            BunCpu::Standard
        } else if std::is_x86_feature_detected!("sse2") {
            BunCpu::Baseline
        } else {
            BunCpu::Unsupported
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        BunCpu::Standard
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingSystem {
    Windows,
    Linux,
    MacOs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X64,
    Arm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BunCpu {
    Standard,
    Baseline,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub kind: RuntimeKind,
    pub version: Version,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    pub runtime: InstalledRuntime,
    pub downloaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRelease {
    pub kind: ToolKind,
    pub version: Version,
    pub artifact: Artifact,
    pub node_engine: Option<String>,
    pub expected_hash: Option<ToolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInstallOutcome {
    pub tool: jolter_storage::InstalledTool,
    pub downloaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginToolArchive {
    pub provider: String,
    pub tool: String,
    pub version: Version,
    pub artifact: Artifact,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginToolInstallOutcome {
    pub provider: String,
    pub tool: String,
    pub version: Version,
    pub path: PathBuf,
    pub commands: Vec<String>,
    pub downloaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalOutcome {
    pub path: PathBuf,
    pub reclaimed_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    #[default]
    Stable,
    Nightly,
}

impl fmt::Display for ReleaseChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable => write!(formatter, "stable"),
            Self::Nightly => write!(formatter, "nightly"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpgradeOutcome {
    pub channel: ReleaseChannel,
    pub version: Version,
    pub updated: bool,
    pub executable_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheCleanOutcome {
    pub reclaimed_bytes: u64,
    pub removed_files: usize,
}
