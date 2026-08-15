use std::collections::BTreeMap;

use reqwest::Url;
use semver::Version;
use serde::Deserialize;

use crate::{
    ToolRequest,
    error::InstallerError,
    installer::Installer,
    types::{ArchiveFormat, Artifact, ArtifactIntegrity, ToolRelease},
};

#[derive(Debug, Deserialize)]
pub(crate) struct NpmPackageMetadata {
    #[serde(rename = "dist-tags")]
    pub dist_tags: BTreeMap<String, String>,
    pub versions: BTreeMap<String, NpmVersionMetadata>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NpmVersionMetadata {
    pub version: String,
    pub dist: NpmDistribution,
    #[serde(default)]
    pub engines: NpmEngines,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct NpmEngines {
    pub node: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NpmDistribution {
    pub tarball: String,
    pub integrity: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InstalledPackageMetadata {
    #[serde(default)]
    pub engines: NpmEngines,
}

impl Installer {
    pub fn resolve_tool(&self, request: &ToolRequest) -> Result<ToolRelease, InstallerError> {
        let package_name = request.kind.registry_package();
        let encoded_name = package_name.replace('@', "%40").replace('/', "%2F");

        let single_spec = if request.selector.eq_ignore_ascii_case("latest") {
            Some("latest")
        } else if Version::parse(&request.selector).is_ok() {
            Some(request.selector.as_str())
        } else {
            None
        };

        if let Some(spec) = single_spec {
            let single_url = format!("https://registry.npmjs.org/{encoded_name}/{spec}");
            if let Ok(contents) = self.npm_metadata_text(&single_url) {
                if let Ok(selected) = serde_json::from_str::<NpmVersionMetadata>(&contents) {
                    if let Ok(version) = Version::parse(&selected.version) {
                        if let Ok(artifact_url) = Url::parse(&selected.dist.tarball) {
                            if let Some(file_name) = artifact_url
                                .path_segments()
                                .and_then(Iterator::last)
                                .filter(|name| !name.is_empty())
                            {
                                if let Ok(integrity) =
                                    ArtifactIntegrity::from_sri(&selected.dist.integrity)
                                {
                                    return Ok(ToolRelease {
                                        kind: request.kind,
                                        version,
                                        artifact: Artifact {
                                            url: selected.dist.tarball,
                                            integrity,
                                            file_name: file_name.to_owned(),
                                            format: ArchiveFormat::TarGz,
                                            strip_components: 1,
                                        },
                                        node_engine: selected.engines.node,
                                        expected_hash: request.hash.clone(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        let url = format!("https://registry.npmjs.org/{encoded_name}");
        let contents = self.npm_metadata_text(&url)?;
        let metadata: NpmPackageMetadata =
            serde_json::from_str(&contents).map_err(|source| InstallerError::MetadataJson {
                url: url.clone(),
                source,
            })?;

        let selected = if request.selector.eq_ignore_ascii_case("latest") {
            metadata
                .dist_tags
                .get("latest")
                .and_then(|version| metadata.versions.get(version))
        } else {
            metadata
                .versions
                .values()
                .filter_map(|release| {
                    let version = Version::parse(&release.version).ok()?;
                    (version.pre.is_empty() && request.matches_version(&version))
                        .then_some((version, release))
                })
                .max_by(|left, right| left.0.cmp(&right.0))
                .map(|(_, release)| release)
        }
        .ok_or_else(|| InstallerError::ToolVersionNotFound(request.clone()))?;

        let version = Version::parse(&selected.version)
            .map_err(|_| InstallerError::ToolVersionNotFound(request.clone()))?;
        let artifact_url = Url::parse(&selected.dist.tarball)
            .map_err(|_| InstallerError::InvalidUrl(selected.dist.tarball.clone()))?;
        let file_name = artifact_url
            .path_segments()
            .and_then(Iterator::last)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| InstallerError::InvalidArtifactName(selected.dist.tarball.clone()))?
            .to_owned();

        Ok(ToolRelease {
            kind: request.kind,
            version,
            artifact: Artifact {
                url: selected.dist.tarball.clone(),
                integrity: ArtifactIntegrity::from_sri(&selected.dist.integrity)?,
                file_name,
                format: ArchiveFormat::TarGz,
                strip_components: 1,
            },
            node_engine: selected.engines.node.clone(),
            expected_hash: request.hash.clone(),
        })
    }
}
