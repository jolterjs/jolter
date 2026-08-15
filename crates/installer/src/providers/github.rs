use jolter_runtime::RuntimeKind;
use semver::Version;
use serde::Deserialize;

use crate::{
    GITHUB_API, RuntimeRequest,
    error::InstallerError,
    http::{checksum_for, ensure_https, parse_checksum_value, parse_github_digest},
    installer::Installer,
    types::{
        Architecture, ArchiveFormat, Artifact, ArtifactIntegrity, BunCpu, OperatingSystem,
        Platform, Release,
    },
};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GithubRelease {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum GithubRuntime {
    Bun,
    Deno,
}

impl GithubRuntime {
    pub(crate) const fn kind(self) -> RuntimeKind {
        match self {
            Self::Bun => RuntimeKind::Bun,
            Self::Deno => RuntimeKind::Deno,
        }
    }

    pub(crate) const fn repository(self) -> &'static str {
        match self {
            Self::Bun => "oven-sh/bun",
            Self::Deno => "denoland/deno",
        }
    }

    pub(crate) fn parse_tag(self, tag: &str) -> Option<Version> {
        let value = match self {
            Self::Bun => tag.strip_prefix("bun-v")?,
            Self::Deno => tag.strip_prefix('v')?,
        };
        Version::parse(value).ok()
    }

    pub(crate) fn asset_name(self, platform: Platform) -> Result<String, InstallerError> {
        match self {
            Self::Bun => {
                let os = match platform.os {
                    OperatingSystem::Windows => "windows",
                    OperatingSystem::Linux => "linux",
                    OperatingSystem::MacOs => "darwin",
                };
                let arch = match platform.arch {
                    Architecture::X64 => "x64",
                    Architecture::Arm64 => "aarch64",
                };
                let variant = match (platform.arch, platform.bun_cpu) {
                    (Architecture::X64, BunCpu::Baseline) => "-baseline",
                    (Architecture::X64, BunCpu::Unsupported) => {
                        return Err(InstallerError::UnsupportedBunCpu);
                    }
                    _ => "",
                };
                Ok(format!("bun-{os}-{arch}{variant}.zip"))
            }
            Self::Deno => {
                let target = match (platform.os, platform.arch) {
                    (OperatingSystem::Windows, Architecture::X64) => "x86_64-pc-windows-msvc",
                    (OperatingSystem::Windows, Architecture::Arm64) => "aarch64-pc-windows-msvc",
                    (OperatingSystem::Linux, Architecture::X64) => "x86_64-unknown-linux-gnu",
                    (OperatingSystem::Linux, Architecture::Arm64) => "aarch64-unknown-linux-gnu",
                    (OperatingSystem::MacOs, Architecture::X64) => "x86_64-apple-darwin",
                    (OperatingSystem::MacOs, Architecture::Arm64) => "aarch64-apple-darwin",
                };
                Ok(format!("deno-{target}.zip"))
            }
        }
    }

    pub(crate) const fn strip_components(self) -> usize {
        match self {
            Self::Bun => 1,
            Self::Deno => 0,
        }
    }

    pub(crate) fn fallback_checksum(
        self,
        installer: &Installer,
        release: &GithubRelease,
        asset: &GithubAsset,
        asset_name: &str,
    ) -> Result<String, InstallerError> {
        match self {
            Self::Bun => {
                let sums = release
                    .assets
                    .iter()
                    .find(|candidate| candidate.name == "SHASUMS256.txt")
                    .ok_or_else(|| InstallerError::ChecksumAssetNotFound {
                        asset: asset_name.to_owned(),
                    })?;
                checksum_for(
                    &installer.metadata_text(&sums.browser_download_url)?,
                    asset_name,
                )
            }
            Self::Deno => {
                let checksum_name = format!("{asset_name}.sha256sum");
                let checksum_asset = release
                    .assets
                    .iter()
                    .find(|candidate| candidate.name == checksum_name)
                    .ok_or_else(|| InstallerError::ChecksumAssetNotFound {
                        asset: asset_name.to_owned(),
                    })?;
                let contents = installer.metadata_text(&checksum_asset.browser_download_url)?;
                parse_checksum_value(&contents)
            }
        }
        .and_then(|checksum| {
            crate::http::validate_checksum(&checksum)?;
            ensure_https(&asset.browser_download_url)?;
            Ok(checksum)
        })
    }
}

impl Installer {
    pub(crate) fn resolve_github(
        &self,
        request: &RuntimeRequest,
        runtime: GithubRuntime,
    ) -> Result<Release, InstallerError> {
        let asset_name = runtime.asset_name(self.platform)?;

        if let Some(release) = self.resolve_github_direct(request, runtime, &asset_name) {
            return Ok(release);
        }

        self.resolve_github_paged(request, runtime, &asset_name)
    }

    fn resolve_github_direct(
        &self,
        request: &RuntimeRequest,
        runtime: GithubRuntime,
        asset_name: &str,
    ) -> Option<Release> {
        let direct_url = if request.selector.eq_ignore_ascii_case("latest") {
            Some(format!(
                "{GITHUB_API}/repos/{}/releases/latest",
                runtime.repository()
            ))
        } else if let Ok(parsed_ver) = Version::parse(&request.selector) {
            let tag = match runtime {
                GithubRuntime::Bun => format!("bun-v{parsed_ver}"),
                GithubRuntime::Deno => format!("v{parsed_ver}"),
            };
            Some(format!(
                "{GITHUB_API}/repos/{}/releases/tags/{tag}",
                runtime.repository()
            ))
        } else {
            None
        };

        let url = direct_url?;
        let contents = self.metadata_text(&url).ok()?;
        let release: GithubRelease = serde_json::from_str(&contents).ok()?;
        if release.draft || release.prerelease {
            return None;
        }
        let version = runtime.parse_tag(&release.tag_name)?;
        if !request.matches_release(&version, false) {
            return None;
        }
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)?;
        let sha256 = match asset.digest.as_deref().and_then(parse_github_digest) {
            Some(checksum) => checksum,
            None => runtime
                .fallback_checksum(self, &release, asset, asset_name)
                .ok()?,
        };

        Some(Release {
            kind: runtime.kind(),
            version,
            artifact: Artifact {
                url: asset.browser_download_url.clone(),
                integrity: ArtifactIntegrity::Sha256(sha256),
                file_name: asset_name.to_string(),
                format: ArchiveFormat::Zip,
                strip_components: runtime.strip_components(),
            },
        })
    }

    fn resolve_github_paged(
        &self,
        request: &RuntimeRequest,
        runtime: GithubRuntime,
        asset_name: &str,
    ) -> Result<Release, InstallerError> {
        for page in 1..=10 {
            let url = format!(
                "{GITHUB_API}/repos/{}/releases?per_page=100&page={page}",
                runtime.repository()
            );
            let contents = self.metadata_text(&url)?;
            let releases: Vec<GithubRelease> =
                serde_json::from_str(&contents).map_err(|source| InstallerError::MetadataJson {
                    url: url.clone(),
                    source,
                })?;
            if releases.is_empty() {
                break;
            }

            let mut candidates = releases
                .into_iter()
                .filter(|release| !release.draft && !release.prerelease)
                .filter_map(|release| {
                    let version = runtime.parse_tag(&release.tag_name)?;
                    request
                        .matches_release(&version, false)
                        .then_some((version, release))
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| right.0.cmp(&left.0));
            if let Some((version, release)) = candidates.into_iter().next() {
                let asset = release
                    .assets
                    .iter()
                    .find(|asset| asset.name == asset_name)
                    .ok_or_else(|| InstallerError::AssetNotFound {
                        version: version.clone(),
                        asset: asset_name.to_string(),
                    })?;
                let sha256 = match asset.digest.as_deref().and_then(parse_github_digest) {
                    Some(checksum) => checksum,
                    None => runtime.fallback_checksum(self, &release, asset, asset_name)?,
                };
                return Ok(Release {
                    kind: runtime.kind(),
                    version,
                    artifact: Artifact {
                        url: asset.browser_download_url.clone(),
                        integrity: ArtifactIntegrity::Sha256(sha256),
                        file_name: asset_name.to_string(),
                        format: ArchiveFormat::Zip,
                        strip_components: runtime.strip_components(),
                    },
                });
            }
        }
        Err(InstallerError::VersionNotFound(request.clone()))
    }
}
