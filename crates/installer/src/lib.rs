use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::GzDecoder;
use fs4::FileExt;
use jolter_runtime::{PackageManagerKind, PackageManagerRequest, RuntimeKind, RuntimeRequest};
use jolter_storage::{InstalledRuntime, InstalledTool, Storage, runtime_executable_in};
use reqwest::{
    Url,
    blocking::Client,
    header::ACCEPT,
    redirect::{Attempt, Policy},
};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use thiserror::Error;

const NODE_INDEX_URL: &str = "https://nodejs.org/dist/index.json";
const GITHUB_API: &str = "https://api.github.com";
const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const METADATA_CACHE_MAX_AGE: Duration = Duration::from_secs(60 * 60);

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
    fn from_sri(value: &str) -> Result<Self, InstallerError> {
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

    fn validate(&self) -> Result<(), InstallerError> {
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

    fn cache_key(&self) -> String {
        format!("{:x}", Sha256::digest(self.to_string().as_bytes()))
    }

    fn verify(&self, path: &Path) -> Result<(), InstallerError> {
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
    const fn cache_extension(self) -> &'static str {
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
pub struct PackageManagerRelease {
    pub kind: PackageManagerKind,
    pub version: Version,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInstallOutcome {
    pub tool: InstalledTool,
    pub downloaded: bool,
}

pub trait HttpClient: Send + Sync {
    fn get_text(&self, url: &str) -> Result<String, InstallerError>;
    fn get_npm_metadata(&self, url: &str) -> Result<String, InstallerError> {
        self.get_text(url)
    }
    fn download(&self, url: &str, destination: &Path) -> Result<(), InstallerError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    client: Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Result<Self, InstallerError> {
        let client = Client::builder()
            .user_agent(concat!("jolter/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(30 * 60))
            .redirect(Policy::custom(https_redirect_policy))
            .build()
            .map_err(InstallerError::HttpClient)?;
        Ok(Self { client })
    }

    fn response(
        &self,
        url: &str,
        accept: Option<&str>,
    ) -> Result<reqwest::blocking::Response, InstallerError> {
        ensure_https(url)?;
        let mut request = self.client.get(url);
        if let Some(accept) = accept {
            request = request.header(ACCEPT, accept);
        }
        let response = request
            .send()
            .map_err(|source| InstallerError::Http {
                url: url.to_owned(),
                source,
            })?
            .error_for_status()
            .map_err(|source| InstallerError::Http {
                url: url.to_owned(),
                source,
            })?;
        ensure_https(response.url().as_str())?;
        Ok(response)
    }
}

impl HttpClient for ReqwestHttpClient {
    fn get_text(&self, url: &str) -> Result<String, InstallerError> {
        read_text_response(self.response(url, None)?, url)
    }

    fn get_npm_metadata(&self, url: &str) -> Result<String, InstallerError> {
        read_text_response(
            self.response(url, Some("application/vnd.npm.install-v1+json"))?,
            url,
        )
    }

    fn download(&self, url: &str, destination: &Path) -> Result<(), InstallerError> {
        let response = self.response(url, None)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
        {
            return Err(InstallerError::ArtifactTooLarge {
                url: url.to_owned(),
            });
        }
        let mut file = File::create(destination).map_err(InstallerError::Io)?;
        let copied = io::copy(&mut response.take(MAX_ARCHIVE_BYTES + 1), &mut file)
            .map_err(InstallerError::Io)?;
        if copied > MAX_ARCHIVE_BYTES {
            return Err(InstallerError::ArtifactTooLarge {
                url: url.to_owned(),
            });
        }
        file.sync_all().map_err(InstallerError::Io)
    }
}

fn read_text_response(
    response: reqwest::blocking::Response,
    url: &str,
) -> Result<String, InstallerError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_METADATA_BYTES)
    {
        return Err(InstallerError::MetadataTooLarge {
            url: url.to_owned(),
        });
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(InstallerError::Io)?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(InstallerError::MetadataTooLarge {
            url: url.to_owned(),
        });
    }
    String::from_utf8(bytes).map_err(|source| InstallerError::InvalidUtf8 {
        url: url.to_owned(),
        source,
    })
}

pub struct Installer {
    storage: Storage,
    platform: Platform,
    http: Arc<dyn HttpClient>,
}

impl Installer {
    pub fn new(storage: Storage) -> Result<Self, InstallerError> {
        Ok(Self {
            storage,
            platform: Platform::current()?,
            http: Arc::new(ReqwestHttpClient::new()?),
        })
    }

    #[must_use]
    pub fn with_client(storage: Storage, platform: Platform, http: Arc<dyn HttpClient>) -> Self {
        Self {
            storage,
            platform,
            http,
        }
    }

    pub fn resolve(&self, request: &RuntimeRequest) -> Result<Release, InstallerError> {
        match request.kind {
            RuntimeKind::Node => self.resolve_node(request),
            RuntimeKind::Bun => self.resolve_github(request, GithubRuntime::Bun),
            RuntimeKind::Deno => self.resolve_github(request, GithubRuntime::Deno),
        }
    }

    pub fn install(&self, request: &RuntimeRequest) -> Result<InstallOutcome, InstallerError> {
        self.install_inner(request, false)
    }

    pub fn repair(&self, request: &RuntimeRequest) -> Result<InstallOutcome, InstallerError> {
        self.install_inner(request, true)
    }

    pub fn resolve_package_manager(
        &self,
        request: &PackageManagerRequest,
    ) -> Result<PackageManagerRelease, InstallerError> {
        let package_name = request.kind.registry_package();
        let encoded_name = package_name.replace('@', "%40").replace('/', "%2F");
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
        .ok_or_else(|| InstallerError::PackageManagerVersionNotFound(request.clone()))?;

        let version = Version::parse(&selected.version)
            .map_err(|_| InstallerError::PackageManagerVersionNotFound(request.clone()))?;
        let artifact_url = Url::parse(&selected.dist.tarball)
            .map_err(|_| InstallerError::InvalidUrl(selected.dist.tarball.clone()))?;
        let file_name = artifact_url
            .path_segments()
            .and_then(Iterator::last)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| InstallerError::InvalidArtifactName(selected.dist.tarball.clone()))?
            .to_owned();

        Ok(PackageManagerRelease {
            kind: request.kind,
            version,
            artifact: Artifact {
                url: selected.dist.tarball.clone(),
                integrity: ArtifactIntegrity::from_sri(&selected.dist.integrity)?,
                file_name,
                format: ArchiveFormat::TarGz,
                strip_components: 1,
            },
        })
    }

    pub fn install_package_manager(
        &self,
        request: &PackageManagerRequest,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.install_package_manager_inner(request, false)
    }

    pub fn repair_package_manager(
        &self,
        request: &PackageManagerRequest,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.install_package_manager_inner(request, true)
    }

    fn install_package_manager_inner(
        &self,
        request: &PackageManagerRequest,
        repair: bool,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let release = self.resolve_package_manager(request)?;
        let destination = self
            .storage
            .tool_version_dir(release.kind, &release.version);
        let lock = self.tool_install_lock(release.kind, &release.version)?;
        FileExt::lock(&lock).map_err(InstallerError::Io)?;

        let command = release.kind.to_string();
        let entrypoint = release
            .kind
            .entrypoint(&command)
            .ok_or(InstallerError::MissingToolEntrypoint(release.kind))?;
        let executable = destination.join(entrypoint);
        if executable.is_file() {
            return Ok(ToolInstallOutcome {
                tool: InstalledTool {
                    kind: release.kind,
                    version: release.version,
                    path: destination,
                },
                downloaded: false,
            });
        }
        if destination.exists() {
            if !repair {
                return Err(InstallerError::CorruptToolInstallation { path: destination });
            }
            let expected_parent = self.storage.tool_dir(release.kind);
            if destination.parent() != Some(expected_parent.as_path()) {
                return Err(InstallerError::UnsafeRemoval { path: destination });
            }
            fs::remove_dir_all(&destination).map_err(|source| InstallerError::RemoveCorrupt {
                path: destination.clone(),
                source,
            })?;
        }

        release.artifact.validate()?;
        let archive = self.obtain_archive(&release.artifact)?;
        let tool_parent = self.storage.tool_dir(release.kind);
        let stage = tempfile::Builder::new()
            .prefix(".jolter-tool-install-")
            .tempdir_in(&tool_parent)
            .map_err(InstallerError::Io)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(InstallerError::Io)?;
        extract_archive(
            &archive,
            &payload,
            release.artifact.format,
            release.artifact.strip_components,
        )?;

        let staged_entrypoint = payload.join(entrypoint);
        if !staged_entrypoint.is_file() {
            return Err(InstallerError::ExecutableMissing {
                path: staged_entrypoint,
            });
        }
        make_executable(&staged_entrypoint)?;
        write_tool_manifest(&payload, &release)?;
        fs::rename(&payload, &destination).map_err(|source| InstallerError::Publish {
            path: destination.clone(),
            source,
        })?;

        Ok(ToolInstallOutcome {
            tool: InstalledTool {
                kind: release.kind,
                version: release.version,
                path: destination,
            },
            downloaded: true,
        })
    }

    fn install_inner(
        &self,
        request: &RuntimeRequest,
        repair: bool,
    ) -> Result<InstallOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let release = self.resolve(request)?;
        let destination = self
            .storage
            .runtime_version_dir(release.kind, &release.version);
        let lock = self.install_lock(release.kind, &release.version)?;
        FileExt::lock(&lock).map_err(InstallerError::Io)?;

        let executable = runtime_executable_in(&destination, release.kind);
        if executable.is_file() {
            return Ok(InstallOutcome {
                runtime: InstalledRuntime {
                    kind: release.kind,
                    version: release.version,
                    path: destination,
                },
                downloaded: false,
            });
        }
        if destination.exists() {
            if !repair {
                return Err(InstallerError::CorruptInstallation { path: destination });
            }
            let expected_parent = self.storage.runtime_dir(release.kind);
            if destination.parent() != Some(expected_parent.as_path()) {
                return Err(InstallerError::UnsafeRemoval { path: destination });
            }
            fs::remove_dir_all(&destination).map_err(|source| InstallerError::RemoveCorrupt {
                path: destination.clone(),
                source,
            })?;
        }

        release.artifact.validate()?;
        let archive = self.obtain_archive(&release.artifact)?;
        let runtime_parent = self.storage.runtime_dir(release.kind);
        let stage = tempfile::Builder::new()
            .prefix(".jolter-install-")
            .tempdir_in(&runtime_parent)
            .map_err(InstallerError::Io)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(InstallerError::Io)?;
        extract_archive(
            &archive,
            &payload,
            release.artifact.format,
            release.artifact.strip_components,
        )?;

        let staged_executable = runtime_executable_in(&payload, release.kind);
        if !staged_executable.is_file() {
            return Err(InstallerError::ExecutableMissing {
                path: staged_executable,
            });
        }
        make_executable(&staged_executable)?;
        write_manifest(&payload, &release)?;
        fs::rename(&payload, &destination).map_err(|source| InstallerError::Publish {
            path: destination.clone(),
            source,
        })?;

        Ok(InstallOutcome {
            runtime: InstalledRuntime {
                kind: release.kind,
                version: release.version,
                path: destination,
            },
            downloaded: true,
        })
    }

    fn resolve_node(&self, request: &RuntimeRequest) -> Result<Release, InstallerError> {
        let contents = self.metadata_text(NODE_INDEX_URL)?;
        let index: Vec<NodeRelease> =
            serde_json::from_str(&contents).map_err(|source| InstallerError::MetadataJson {
                url: NODE_INDEX_URL.to_owned(),
                source,
            })?;
        let target = node_target(self.platform);
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

    fn resolve_github(
        &self,
        request: &RuntimeRequest,
        runtime: GithubRuntime,
    ) -> Result<Release, InstallerError> {
        let asset_name = runtime.asset_name(self.platform)?;
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
                        asset: asset_name.clone(),
                    })?;
                let sha256 = match asset.digest.as_deref().and_then(parse_github_digest) {
                    Some(checksum) => checksum,
                    None => runtime.fallback_checksum(self, &release, asset, &asset_name)?,
                };
                return Ok(Release {
                    kind: runtime.kind(),
                    version,
                    artifact: Artifact {
                        url: asset.browser_download_url.clone(),
                        integrity: ArtifactIntegrity::Sha256(sha256),
                        file_name: asset_name,
                        format: ArchiveFormat::Zip,
                        strip_components: runtime.strip_components(),
                    },
                });
            }
        }
        Err(InstallerError::VersionNotFound(request.clone()))
    }

    fn install_lock(&self, kind: RuntimeKind, version: &Version) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("{kind}-{version}.lock")))
            .map_err(InstallerError::Io)
    }

    fn tool_install_lock(
        &self,
        kind: PackageManagerKind,
        version: &Version,
    ) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("tool-{kind}-{version}.lock")))
            .map_err(InstallerError::Io)
    }

    fn obtain_archive(&self, artifact: &Artifact) -> Result<PathBuf, InstallerError> {
        let directory = self.storage.cache_dir().join("downloads");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        let path = directory.join(format!(
            "{}.{}",
            artifact.integrity.cache_key(),
            artifact.format.cache_extension()
        ));
        if path.is_file() {
            if artifact.integrity.verify(&path).is_ok() {
                return Ok(path);
            }
            fs::remove_file(&path).map_err(InstallerError::Io)?;
        }

        let temporary = tempfile::Builder::new()
            .prefix(".jolter-download-")
            .tempfile_in(&directory)
            .map_err(InstallerError::Io)?;
        self.http.download(&artifact.url, temporary.path())?;
        artifact.integrity.verify(temporary.path())?;
        temporary
            .persist(&path)
            .map_err(|error| InstallerError::Io(error.error))?;
        Ok(path)
    }

    fn metadata_text(&self, url: &str) -> Result<String, InstallerError> {
        self.metadata_text_inner(url, false)
    }

    fn npm_metadata_text(&self, url: &str) -> Result<String, InstallerError> {
        self.metadata_text_inner(url, true)
    }

    fn metadata_text_inner(&self, url: &str, npm_metadata: bool) -> Result<String, InstallerError> {
        ensure_https(url)?;
        let directory = self.storage.cache_dir().join("metadata");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        let cache_source = if npm_metadata {
            format!("npm:{url}")
        } else {
            url.to_owned()
        };
        let cache_key = format!("{:x}", Sha256::digest(cache_source.as_bytes()));
        let cache_path = directory.join(format!("{cache_key}.txt"));
        let cached = fs::read_to_string(&cache_path).ok();
        let fresh = fs::metadata(&cache_path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age <= METADATA_CACHE_MAX_AGE);
        if fresh {
            return cached.ok_or_else(|| InstallerError::MetadataCacheRead {
                path: cache_path.clone(),
            });
        }
        if offline_mode() {
            return cached.ok_or_else(|| InstallerError::OfflineCacheMiss {
                url: url.to_owned(),
            });
        }

        let response = if npm_metadata {
            self.http.get_npm_metadata(url)
        } else {
            self.http.get_text(url)
        };
        match response {
            Ok(contents) => {
                write_cache_file(&cache_path, contents.as_bytes())?;
                Ok(contents)
            }
            Err(error) => cached.ok_or(error),
        }
    }
}

#[derive(Debug, Deserialize)]
struct NodeRelease {
    version: String,
    lts: serde_json::Value,
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NpmPackageMetadata {
    #[serde(rename = "dist-tags")]
    dist_tags: BTreeMap<String, String>,
    versions: BTreeMap<String, NpmVersionMetadata>,
}

#[derive(Debug, Deserialize)]
struct NpmVersionMetadata {
    version: String,
    dist: NpmDistribution,
}

#[derive(Debug, Deserialize)]
struct NpmDistribution {
    tarball: String,
    integrity: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum GithubRuntime {
    Bun,
    Deno,
}

impl GithubRuntime {
    const fn kind(self) -> RuntimeKind {
        match self {
            Self::Bun => RuntimeKind::Bun,
            Self::Deno => RuntimeKind::Deno,
        }
    }

    const fn repository(self) -> &'static str {
        match self {
            Self::Bun => "oven-sh/bun",
            Self::Deno => "denoland/deno",
        }
    }

    fn parse_tag(self, tag: &str) -> Option<Version> {
        let value = match self {
            Self::Bun => tag.strip_prefix("bun-v")?,
            Self::Deno => tag.strip_prefix('v')?,
        };
        Version::parse(value).ok()
    }

    fn asset_name(self, platform: Platform) -> Result<String, InstallerError> {
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

    const fn strip_components(self) -> usize {
        match self {
            Self::Bun => 1,
            Self::Deno => 0,
        }
    }

    fn fallback_checksum(
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
            validate_checksum(&checksum)?;
            ensure_https(&asset.browser_download_url)?;
            Ok(checksum)
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct NodeTarget {
    index_name: &'static str,
    archive_name: &'static str,
    format: ArchiveFormat,
}

fn node_target(platform: Platform) -> NodeTarget {
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

fn detect_bun_cpu(arch: Architecture) -> BunCpu {
    if arch == Architecture::Arm64 {
        return BunCpu::Standard;
    }
    #[cfg(target_arch = "x86_64")]
    {
        if !std::is_x86_feature_detected!("sse4.2") {
            BunCpu::Unsupported
        } else if std::is_x86_feature_detected!("avx2") {
            BunCpu::Standard
        } else {
            BunCpu::Baseline
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        BunCpu::Unsupported
    }
}

fn https_redirect_policy(attempt: Attempt<'_>) -> reqwest::redirect::Action {
    if attempt.previous().len() >= 10 {
        return attempt.error("too many redirects");
    }
    if attempt.url().scheme() != "https" {
        return attempt.error("refusing redirect to a non-HTTPS URL");
    }
    attempt.follow()
}

fn ensure_https(value: &str) -> Result<(), InstallerError> {
    let url = Url::parse(value).map_err(|_| InstallerError::InvalidUrl(value.to_owned()))?;
    if url.scheme() != "https" {
        return Err(InstallerError::InsecureUrl(value.to_owned()));
    }
    Ok(())
}

fn offline_mode() -> bool {
    std::env::var_os("JOLTER_OFFLINE").is_some_and(|value| {
        matches!(
            value.to_string_lossy().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

fn write_cache_file(path: &Path, contents: &[u8]) -> Result<(), InstallerError> {
    let parent = path
        .parent()
        .ok_or_else(|| InstallerError::MetadataCacheRead {
            path: path.to_path_buf(),
        })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(InstallerError::Io)?;
    temporary.write_all(contents).map_err(InstallerError::Io)?;
    temporary
        .persist(path)
        .map_err(|error| InstallerError::Io(error.error))?;
    Ok(())
}

fn validate_checksum(value: &str) -> Result<(), InstallerError> {
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(InstallerError::InvalidChecksum(value.to_owned()));
    }
    Ok(())
}

fn parse_github_digest(value: &str) -> Option<String> {
    let checksum = value.strip_prefix("sha256:")?;
    validate_checksum(checksum).ok()?;
    Some(checksum.to_ascii_lowercase())
}

fn checksum_for(contents: &str, file_name: &str) -> Result<String, InstallerError> {
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(checksum) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if name.trim_start_matches('*') == file_name {
            validate_checksum(checksum)?;
            return Ok(checksum.to_ascii_lowercase());
        }
    }
    Err(InstallerError::ChecksumNotFound {
        file: file_name.to_owned(),
    })
}

fn parse_checksum_value(contents: &str) -> Result<String, InstallerError> {
    let checksum = contents
        .split_whitespace()
        .next()
        .ok_or(InstallerError::EmptyChecksum)?;
    validate_checksum(checksum)?;
    Ok(checksum.to_ascii_lowercase())
}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), InstallerError> {
    validate_checksum(expected)?;
    let mut file = File::open(path).map_err(InstallerError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(InstallerError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(InstallerError::ChecksumMismatch {
            expected: expected.to_owned(),
            actual,
        })
    }
}

fn verify_sha512(path: &Path, expected: &str) -> Result<(), InstallerError> {
    let expected_bytes = BASE64
        .decode(expected)
        .map_err(|_| InstallerError::InvalidIntegrity(format!("sha512-{expected}")))?;
    if expected_bytes.len() != 64 {
        return Err(InstallerError::InvalidIntegrity(format!(
            "sha512-{expected}"
        )));
    }
    let mut file = File::open(path).map_err(InstallerError::Io)?;
    let mut hasher = Sha512::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(InstallerError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual_bytes = hasher.finalize();
    if actual_bytes.as_slice() == expected_bytes {
        Ok(())
    } else {
        Err(InstallerError::ChecksumMismatch {
            expected: format!("sha512-{expected}"),
            actual: format!("sha512-{}", BASE64.encode(actual_bytes)),
        })
    }
}

fn extract_archive(
    archive: &Path,
    destination: &Path,
    format: ArchiveFormat,
    strip_components: usize,
) -> Result<(), InstallerError> {
    match format {
        ArchiveFormat::Zip => extract_zip(archive, destination, strip_components),
        ArchiveFormat::TarGz => extract_tar_gz(archive, destination, strip_components),
    }
}

fn extract_zip(
    archive: &Path,
    destination: &Path,
    strip_components: usize,
) -> Result<(), InstallerError> {
    let file = File::open(archive).map_err(InstallerError::Io)?;
    let mut zip = zip::ZipArchive::new(file).map_err(InstallerError::Zip)?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(InstallerError::ArchiveEntryLimit);
    }
    let mut extracted = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(InstallerError::Zip)?;
        if entry.is_symlink() {
            return Err(InstallerError::UnsupportedArchiveEntry(
                entry.name().to_owned(),
            ));
        }
        extracted = extracted
            .checked_add(entry.size())
            .ok_or(InstallerError::ArchiveSizeLimit)?;
        if extracted > MAX_ARCHIVE_BYTES {
            return Err(InstallerError::ArchiveSizeLimit);
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| InstallerError::UnsafeArchivePath(entry.name().to_owned()))?;
        let Some(relative) = stripped_relative(&enclosed, strip_components)? else {
            continue;
        };
        let output = destination.join(relative);
        ensure_safe_parent(destination, &output)?;
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(InstallerError::Io)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(InstallerError::Io)?;
        }
        let mut output_file = File::create(&output).map_err(InstallerError::Io)?;
        io::copy(&mut entry, &mut output_file).map_err(InstallerError::Io)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            set_mode(&output, mode)?;
        }
    }
    Ok(())
}

fn extract_tar_gz(
    archive: &Path,
    destination: &Path,
    strip_components: usize,
) -> Result<(), InstallerError> {
    let file = File::open(archive).map_err(InstallerError::Io)?;
    let decoder = GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut extracted = 0_u64;
    let mut entries = 0_usize;
    for entry in tar.entries().map_err(InstallerError::Io)? {
        entries += 1;
        if entries > MAX_ARCHIVE_ENTRIES {
            return Err(InstallerError::ArchiveEntryLimit);
        }
        let mut entry = entry.map_err(InstallerError::Io)?;
        let path = entry.path().map_err(InstallerError::Io)?.into_owned();
        let Some(relative) = stripped_relative(&path, strip_components)? else {
            continue;
        };
        let output = destination.join(&relative);
        ensure_safe_parent(destination, &output)?;
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            fs::create_dir_all(&output).map_err(InstallerError::Io)?;
        } else if entry_type.is_file() {
            let size = entry.header().size().map_err(InstallerError::Io)?;
            extracted = extracted
                .checked_add(size)
                .ok_or(InstallerError::ArchiveSizeLimit)?;
            if extracted > MAX_ARCHIVE_BYTES {
                return Err(InstallerError::ArchiveSizeLimit);
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(InstallerError::Io)?;
            }
            let mut output_file = File::create(&output).map_err(InstallerError::Io)?;
            io::copy(&mut entry, &mut output_file).map_err(InstallerError::Io)?;
            #[cfg(unix)]
            set_mode(&output, entry.header().mode().map_err(InstallerError::Io)?)?;
        } else if entry_type.is_symlink() {
            extract_symlink(&entry, destination, &relative, &output)?;
        } else if entry_type.is_hard_link() {
            extract_hard_link(&entry, destination, strip_components, &output)?;
        } else {
            return Err(InstallerError::UnsupportedArchiveEntry(
                path.display().to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn extract_symlink<R: Read>(
    entry: &tar::Entry<'_, R>,
    destination: &Path,
    relative: &Path,
    output: &Path,
) -> Result<(), InstallerError> {
    let target = entry
        .link_name()
        .map_err(InstallerError::Io)?
        .ok_or_else(|| InstallerError::UnsafeArchivePath(relative.display().to_string()))?;
    validate_symlink_target(relative, &target)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(InstallerError::Io)?;
        ensure_safe_parent(destination, output)?;
    }
    std::os::unix::fs::symlink(target, output).map_err(InstallerError::Io)
}

#[cfg(not(unix))]
fn extract_symlink<R: Read>(
    _entry: &tar::Entry<'_, R>,
    _destination: &Path,
    relative: &Path,
    _output: &Path,
) -> Result<(), InstallerError> {
    Err(InstallerError::UnsupportedArchiveEntry(
        relative.display().to_string(),
    ))
}

#[cfg(unix)]
fn extract_hard_link<R: Read>(
    entry: &tar::Entry<'_, R>,
    destination: &Path,
    strip_components: usize,
    output: &Path,
) -> Result<(), InstallerError> {
    let target = entry
        .link_name()
        .map_err(InstallerError::Io)?
        .ok_or_else(|| InstallerError::UnsafeArchivePath(output.display().to_string()))?;
    let target = stripped_relative(&target, strip_components)?
        .ok_or_else(|| InstallerError::UnsafeArchivePath(target.display().to_string()))?;
    let source = destination.join(target);
    ensure_safe_parent(destination, &source)?;
    if !source.is_file() {
        return Err(InstallerError::UnsafeArchivePath(
            source.display().to_string(),
        ));
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(InstallerError::Io)?;
    }
    fs::hard_link(source, output).map_err(InstallerError::Io)
}

#[cfg(not(unix))]
fn extract_hard_link<R: Read>(
    _entry: &tar::Entry<'_, R>,
    _destination: &Path,
    _strip_components: usize,
    output: &Path,
) -> Result<(), InstallerError> {
    Err(InstallerError::UnsupportedArchiveEntry(
        output.display().to_string(),
    ))
}

fn stripped_relative(
    path: &Path,
    strip_components: usize,
) -> Result<Option<PathBuf>, InstallerError> {
    let mut normals = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normals.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(InstallerError::UnsafeArchivePath(
                    path.display().to_string(),
                ));
            }
        }
    }
    if normals.len() <= strip_components {
        return Ok(None);
    }
    Ok(Some(normals.into_iter().skip(strip_components).collect()))
}

fn ensure_safe_parent(root: &Path, output: &Path) -> Result<(), InstallerError> {
    let relative = output
        .strip_prefix(root)
        .map_err(|_| InstallerError::UnsafeArchivePath(output.display().to_string()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err(InstallerError::UnsafeArchivePath(
                output.display().to_string(),
            ));
        };
        current.push(value);
        if current == output {
            break;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(InstallerError::UnsafeArchivePath(
                    output.display().to_string(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(InstallerError::Io(error)),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_symlink_target(relative: &Path, target: &Path) -> Result<(), InstallerError> {
    if target.is_absolute() {
        return Err(InstallerError::UnsafeArchivePath(
            target.display().to_string(),
        ));
    }
    let mut depth = relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(InstallerError::UnsafeArchivePath(
                    target.display().to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), InstallerError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777)).map_err(InstallerError::Io)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), InstallerError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(InstallerError::Io)?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions).map_err(InstallerError::Io)
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn make_executable(_path: &Path) -> Result<(), InstallerError> {
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallManifest<'a> {
    runtime: String,
    version: String,
    artifact_url: &'a str,
    integrity: String,
}

fn write_manifest(destination: &Path, release: &Release) -> Result<(), InstallerError> {
    let manifest = InstallManifest {
        runtime: release.kind.to_string(),
        version: release.version.to_string(),
        artifact_url: &release.artifact.url,
        integrity: release.artifact.integrity.to_string(),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(InstallerError::Manifest)?;
    fs::write(destination.join(".jolter-install.json"), contents).map_err(InstallerError::Io)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolInstallManifest<'a> {
    package_manager: String,
    version: String,
    artifact_url: &'a str,
    integrity: String,
}

fn write_tool_manifest(
    destination: &Path,
    release: &PackageManagerRelease,
) -> Result<(), InstallerError> {
    let manifest = ToolInstallManifest {
        package_manager: release.kind.to_string(),
        version: release.version.to_string(),
        artifact_url: &release.artifact.url,
        integrity: release.artifact.integrity.to_string(),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(InstallerError::Manifest)?;
    fs::write(destination.join(".jolter-tool.json"), contents).map_err(InstallerError::Io)
}

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
    #[error("no stable release satisfies {0}")]
    VersionNotFound(RuntimeRequest),
    #[error("no stable package manager release satisfies {0}")]
    PackageManagerVersionNotFound(PackageManagerRequest),
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
    #[error("existing package manager installation at {path} is incomplete")]
    CorruptToolInstallation { path: PathBuf },
    #[error("no entrypoint is defined for package manager {0}")]
    MissingToolEntrypoint(PackageManagerKind),
    #[error("refusing to remove runtime path outside its expected parent: {path}")]
    UnsafeRemoval { path: PathBuf },
    #[error("failed to remove incomplete runtime installation at {path}: {source}")]
    RemoveCorrupt {
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

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::{
        collections::HashMap,
        io::{Cursor, Write},
        sync::Mutex,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    struct FakeHttpClient {
        text: HashMap<String, String>,
        downloads: HashMap<String, Vec<u8>>,
        text_count: Mutex<usize>,
        download_count: Mutex<usize>,
    }

    impl HttpClient for FakeHttpClient {
        fn get_text(&self, url: &str) -> Result<String, InstallerError> {
            *self.text_count.lock().unwrap() += 1;
            self.text.get(url).cloned().ok_or_else(|| {
                InstallerError::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no fake response for {url}"),
                ))
            })
        }

        fn download(&self, url: &str, destination: &Path) -> Result<(), InstallerError> {
            let bytes = self.downloads.get(url).ok_or_else(|| {
                InstallerError::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no fake download for {url}"),
                ))
            })?;
            fs::write(destination, bytes).map_err(InstallerError::Io)?;
            *self.download_count.lock().unwrap() += 1;
            Ok(())
        }
    }

    #[test]
    fn rejects_insecure_artifact() {
        let artifact = Artifact {
            url: "http://example.test/node.zip".to_owned(),
            integrity: ArtifactIntegrity::Sha256("a".repeat(64)),
            file_name: "node.zip".to_owned(),
            format: ArchiveFormat::Zip,
            strip_components: 0,
        };
        assert!(matches!(
            artifact.validate(),
            Err(InstallerError::InsecureUrl(_))
        ));
    }

    #[test]
    fn selects_checksum_by_exact_file_name() {
        let sums = concat!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  node-a.zip\n",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  node-b.zip\n"
        );
        assert_eq!(checksum_for(sums, "node-b.zip").unwrap(), "b".repeat(64));
    }

    #[test]
    fn rejects_archive_parent_traversal() {
        assert!(matches!(
            stripped_relative(Path::new("../outside"), 0),
            Err(InstallerError::UnsafeArchivePath(_))
        ));
    }

    #[test]
    fn maps_supported_platform_assets() {
        let windows = Platform {
            os: OperatingSystem::Windows,
            arch: Architecture::X64,
            bun_cpu: BunCpu::Standard,
        };
        assert_eq!(
            GithubRuntime::Bun.asset_name(windows).unwrap(),
            "bun-windows-x64.zip"
        );
        assert_eq!(
            GithubRuntime::Deno.asset_name(windows).unwrap(),
            "deno-x86_64-pc-windows-msvc.zip"
        );
        let baseline = Platform {
            bun_cpu: BunCpu::Baseline,
            ..windows
        };
        assert_eq!(
            GithubRuntime::Bun.asset_name(baseline).unwrap(),
            "bun-windows-x64-baseline.zip"
        );
    }

    #[test]
    fn installs_and_repairs_a_verified_runtime_archive() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let platform = Platform::current().unwrap();
        let asset_name = GithubRuntime::Deno.asset_name(platform).unwrap();
        let executable_name = if cfg!(windows) { "deno.exe" } else { "deno" };
        let archive = zip_with_file(executable_name, b"fake deno");
        let checksum = format!("{:x}", Sha256::digest(&archive));
        let download_url = format!("https://example.test/{asset_name}");
        let metadata_url = format!("{GITHUB_API}/repos/denoland/deno/releases?per_page=100&page=1");
        let metadata = serde_json::json!([{
            "tag_name": "v2.8.3",
            "draft": false,
            "prerelease": false,
            "assets": [{
                "name": asset_name.clone(),
                "browser_download_url": download_url.clone(),
                "digest": format!("sha256:{checksum}")
            }]
        }])
        .to_string();
        let client = Arc::new(FakeHttpClient {
            text: HashMap::from([(metadata_url, metadata)]),
            downloads: HashMap::from([(download_url, archive)]),
            text_count: Mutex::new(0),
            download_count: Mutex::new(0),
        });
        let installer = Installer::with_client(storage.clone(), platform, client.clone());

        let outcome = installer.install(&"deno@2".parse().unwrap()).unwrap();
        assert!(outcome.downloaded);
        assert!(
            storage
                .runtime_executable(RuntimeKind::Deno, &Version::new(2, 8, 3))
                .is_file()
        );
        assert!(outcome.runtime.path.join(".jolter-install.json").is_file());

        fs::remove_file(storage.runtime_executable(RuntimeKind::Deno, &Version::new(2, 8, 3)))
            .unwrap();
        let repaired = installer.repair(&"deno@2.8.3".parse().unwrap()).unwrap();
        assert!(repaired.downloaded);
        assert_eq!(*client.text_count.lock().unwrap(), 1);
        assert_eq!(*client.download_count.lock().unwrap(), 1);
    }

    #[test]
    fn installs_a_verified_package_manager_archive() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let archive = tar_gz_with_file("package/bin/pnpm.cjs", b"fake pnpm");
        let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(&archive)));
        let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
        let download_url = "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz".to_owned();
        let metadata = serde_json::json!({
            "dist-tags": { "latest": "10.2.0" },
            "versions": {
                "10.1.0": {
                    "version": "10.1.0",
                    "dist": {
                        "tarball": "https://registry.npmjs.org/pnpm/-/pnpm-10.1.0.tgz",
                        "integrity": integrity
                    }
                },
                "10.2.0": {
                    "version": "10.2.0",
                    "dist": {
                        "tarball": download_url.clone(),
                        "integrity": integrity
                    }
                }
            }
        })
        .to_string();
        let client = Arc::new(FakeHttpClient {
            text: HashMap::from([(metadata_url, metadata)]),
            downloads: HashMap::from([(download_url, archive)]),
            text_count: Mutex::new(0),
            download_count: Mutex::new(0),
        });
        let installer = Installer::with_client(
            storage.clone(),
            Platform::current().unwrap(),
            client.clone(),
        );

        let outcome = installer
            .install_package_manager(&"pnpm@10".parse().unwrap())
            .unwrap();

        assert!(outcome.downloaded);
        assert_eq!(outcome.tool.version, Version::new(10, 2, 0));
        assert!(
            storage
                .tool_entrypoint(PackageManagerKind::Pnpm, &outcome.tool.version, "pnpm")
                .unwrap()
                .is_file()
        );
        assert!(outcome.tool.path.join(".jolter-tool.json").is_file());
        assert_eq!(*client.download_count.lock().unwrap(), 1);
    }

    fn zip_with_file(path: &str, contents: &[u8]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut cursor);
            writer
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn tar_gz_with_file(path: &str, contents: &[u8]) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append_data(&mut header, path, contents).unwrap();
        archive.into_inner().unwrap().finish().unwrap()
    }
}
