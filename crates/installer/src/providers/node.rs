use jolter_runtime::RuntimeKind;
use semver::Version;
use serde::Deserialize;

use crate::{
    NODE_INDEX_URL, RuntimeRequest,
    error::InstallerError,
    http::checksum_for,
    installer::Installer,
    types::{
        Architecture, ArchiveFormat, Artifact, ArtifactIntegrity, OperatingSystem, Platform,
        Release,
    },
};

#[derive(Debug, Deserialize)]
pub(crate) struct NodeRelease {
    pub version: String,
    pub lts: serde_json::Value,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NodeTarget {
    pub index_name: &'static str,
    pub archive_name: &'static str,
    pub format: ArchiveFormat,
}

pub(crate) fn node_target(platform: Platform) -> NodeTarget {
    match (platform.os, platform.arch) {
        (OperatingSystem::Windows, Architecture::X64) => NodeTarget {
            index_name: "win-x64-zip",
            archive_name: "win-x64",
            format: ArchiveFormat::Zip,
        },
        (OperatingSystem::Windows, Architecture::Arm64) => NodeTarget {
            index_name: "win-arm64-zip",
            archive_name: "win-arm64",
            format: ArchiveFormat::Zip,
        },
        (OperatingSystem::Linux, Architecture::X64) => NodeTarget {
            index_name: "linux-x64",
            archive_name: "linux-x64",
            format: ArchiveFormat::TarGz,
        },
        (OperatingSystem::Linux, Architecture::Arm64) => NodeTarget {
            index_name: "linux-arm64",
            archive_name: "linux-arm64",
            format: ArchiveFormat::TarGz,
        },
        (OperatingSystem::MacOs, Architecture::X64) => NodeTarget {
            index_name: "osx-x64-tar",
            archive_name: "darwin-x64",
            format: ArchiveFormat::TarGz,
        },
        (OperatingSystem::MacOs, Architecture::Arm64) => NodeTarget {
            index_name: "osx-arm64-tar",
            archive_name: "darwin-arm64",
            format: ArchiveFormat::TarGz,
        },
    }
}

impl Installer {
    pub(crate) fn resolve_node(&self, request: &RuntimeRequest) -> Result<Release, InstallerError> {
        let target = node_target(self.platform);
        if let Ok(version) = Version::parse(&request.selector) {
            let tagged_version = format!("v{version}");
            let file_name = format!(
                "node-{tagged_version}-{}.{}",
                target.archive_name,
                target.format.cache_extension()
            );
            let sums_url = format!("https://nodejs.org/dist/{tagged_version}/SHASUMS256.txt");
            if let Ok(sums) = self.metadata_text(&sums_url) {
                if let Ok(sha256) = checksum_for(&sums, &file_name) {
                    return Ok(Release {
                        kind: RuntimeKind::Node,
                        version,
                        artifact: Artifact {
                            url: format!("https://nodejs.org/dist/{tagged_version}/{file_name}"),
                            integrity: ArtifactIntegrity::Sha256(sha256),
                            file_name,
                            format: target.format,
                            strip_components: 1,
                        },
                    });
                }
            }
        }

        let contents = self.metadata_text(NODE_INDEX_URL)?;
        let index: Vec<NodeRelease> =
            serde_json::from_str(&contents).map_err(|source| InstallerError::MetadataJson {
                url: NODE_INDEX_URL.to_owned(),
                source,
            })?;
        let mut candidates = index
            .into_iter()
            .filter_map(|release| {
                let version = Version::parse(release.version.trim_start_matches('v')).ok()?;
                let is_lts = release.lts != serde_json::Value::Bool(false);
                (request.matches_release(&version, is_lts)
                    && release.files.iter().any(|file| file == target.index_name))
                .then_some((version, release.version))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.0.cmp(&left.0));
        let (version, tagged_version) = candidates
            .into_iter()
            .next()
            .ok_or_else(|| InstallerError::VersionNotFound(request.clone()))?;
        let file_name = format!(
            "node-{tagged_version}-{}.{}",
            target.archive_name,
            target.format.cache_extension()
        );
        let sums_url = format!("https://nodejs.org/dist/{tagged_version}/SHASUMS256.txt");
        let sums = self.metadata_text(&sums_url)?;
        let sha256 = checksum_for(&sums, &file_name)?;

        Ok(Release {
            kind: RuntimeKind::Node,
            version,
            artifact: Artifact {
                url: format!("https://nodejs.org/dist/{tagged_version}/{file_name}"),
                integrity: ArtifactIntegrity::Sha256(sha256),
                file_name,
                format: target.format,
                strip_components: 1,
            },
        })
    }
}
