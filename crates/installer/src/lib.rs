use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::GzDecoder;
use fs4::FileExt;
use jolter_runtime::{
    RuntimeKind, RuntimeRequest, ToolHash, ToolHashAlgorithm, ToolKind, ToolRequest,
};
use jolter_storage::{CacheStats, InstalledRuntime, InstalledTool, Storage, runtime_executable_in};
use nodejs_semver::{Range as NodeRange, Version as NodeVersion};
use reqwest::{
    StatusCode, Url,
    blocking::Client,
    header::{ACCEPT, RETRY_AFTER},
    redirect::{Attempt, Policy},
};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};
use thiserror::Error;

const NODE_INDEX_URL: &str = "https://nodejs.org/dist/index.json";
const GITHUB_API: &str = "https://api.github.com";
const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const METADATA_CACHE_MAX_AGE: Duration = Duration::from_secs(60 * 60);
const MAX_HTTP_ATTEMPTS: usize = 3;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(5);

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
pub struct ToolRelease {
    pub kind: ToolKind,
    pub version: Version,
    pub artifact: Artifact,
    pub node_engine: Option<String>,
    pub expected_hash: Option<ToolHash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInstallOutcome {
    pub tool: InstalledTool,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheCleanOutcome {
    pub removed_files: u64,
    pub reclaimed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressAction {
    Select,
    Resolve,
    Reuse,
    Connect,
    Download,
    Verify,
    Extract,
    Publish,
    Activate,
    Remove,
    Clean,
    Diagnose,
    Configure,
    Shims,
}

impl ProgressAction {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Resolve => "resolve",
            Self::Reuse => "reuse",
            Self::Connect => "connect",
            Self::Download => "fetch",
            Self::Verify => "verify",
            Self::Extract => "unpack",
            Self::Publish => "install",
            Self::Activate => "activate",
            Self::Remove => "remove",
            Self::Clean => "clean",
            Self::Diagnose => "doctor",
            Self::Configure => "config",
            Self::Shims => "shims",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEvent<'a> {
    Stage {
        action: ProgressAction,
        target: &'a str,
    },
    DownloadStarted {
        name: &'a str,
        total: Option<u64>,
    },
    DownloadAdvanced {
        name: &'a str,
        downloaded: u64,
        total: Option<u64>,
    },
    DownloadFinished {
        name: &'a str,
        downloaded: u64,
        total: Option<u64>,
    },
    CacheHit {
        name: &'a str,
    },
}

pub trait ProgressReporter: Send + Sync {
    fn report(&self, event: ProgressEvent<'_>);
}

#[derive(Debug, Default)]
pub struct NoProgressReporter;

impl ProgressReporter for NoProgressReporter {
    fn report(&self, _event: ProgressEvent<'_>) {}
}

pub trait HttpClient: Send + Sync {
    fn get_text(&self, url: &str) -> Result<String, InstallerError>;
    fn get_npm_metadata(&self, url: &str) -> Result<String, InstallerError> {
        self.get_text(url)
    }
    fn download(
        &self,
        url: &str,
        destination: &Path,
        name: &str,
        reporter: &dyn ProgressReporter,
    ) -> Result<(), InstallerError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    client: Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Result<Self, InstallerError> {
        let client = Client::builder()
            .user_agent(concat!("jolter/", env!("CARGO_PKG_VERSION")))
            .tcp_nodelay(true)
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
        for attempt in 0..MAX_HTTP_ATTEMPTS {
            let mut request = self.client.get(url);
            if let Some(accept) = accept {
                request = request.header(ACCEPT, accept);
            }
            match request.send() {
                Ok(response)
                    if retryable_status(response.status()) && attempt + 1 < MAX_HTTP_ATTEMPTS =>
                {
                    thread::sleep(retry_delay(attempt, response.headers().get(RETRY_AFTER)));
                }
                Ok(response) => {
                    let response =
                        response
                            .error_for_status()
                            .map_err(|source| InstallerError::Http {
                                url: url.to_owned(),
                                source,
                            })?;
                    ensure_https(response.url().as_str())?;
                    return Ok(response);
                }
                Err(source)
                    if retryable_request_error(&source) && attempt + 1 < MAX_HTTP_ATTEMPTS =>
                {
                    thread::sleep(retry_delay(attempt, None));
                }
                Err(source) => {
                    return Err(InstallerError::Http {
                        url: url.to_owned(),
                        source,
                    });
                }
            }
        }
        unreachable!("the bounded HTTP attempt loop always returns on its final attempt")
    }
}

fn retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn retryable_request_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request()
}

fn retry_delay(attempt: usize, retry_after: Option<&reqwest::header::HeaderValue>) -> Duration {
    if let Some(seconds) = retry_after
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_secs(seconds).min(MAX_RETRY_AFTER);
    }
    Duration::from_millis(250 * (1_u64 << attempt.min(4)))
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

    fn download(
        &self,
        url: &str,
        destination: &Path,
        name: &str,
        reporter: &dyn ProgressReporter,
    ) -> Result<(), InstallerError> {
        reporter.report(ProgressEvent::Stage {
            action: ProgressAction::Connect,
            target: name,
        });
        let mut response = self.response(url, None)?;
        let total = response.content_length();
        if total.is_some_and(|length| length > MAX_ARCHIVE_BYTES) {
            return Err(InstallerError::ArtifactTooLarge {
                url: url.to_owned(),
            });
        }
        reporter.report(ProgressEvent::DownloadStarted { name, total });
        let file = File::create(destination).map_err(InstallerError::Io)?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);
        let mut downloaded = 0_u64;
        let mut buffer = vec![0_u8; 256 * 1024];
        loop {
            let read = response.read(&mut buffer).map_err(InstallerError::Io)?;
            if read == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(read as u64);
            if downloaded > MAX_ARCHIVE_BYTES {
                return Err(InstallerError::ArtifactTooLarge {
                    url: url.to_owned(),
                });
            }
            writer
                .write_all(&buffer[..read])
                .map_err(InstallerError::Io)?;
            reporter.report(ProgressEvent::DownloadAdvanced {
                name,
                downloaded,
                total,
            });
        }
        writer.flush().map_err(InstallerError::Io)?;
        reporter.report(ProgressEvent::DownloadFinished {
            name,
            downloaded,
            total,
        });
        Ok(())
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
    http: HttpClientSource,
    reporter: Arc<dyn ProgressReporter>,
}

enum HttpClientSource {
    Lazy(Mutex<Option<Arc<dyn HttpClient>>>),
    Ready(Arc<dyn HttpClient>),
}

impl HttpClientSource {
    fn get(&self) -> Result<Arc<dyn HttpClient>, InstallerError> {
        match self {
            Self::Lazy(client) => {
                let mut client = client.lock().unwrap();
                if client.is_none() {
                    *client = Some(Arc::new(ReqwestHttpClient::new()?));
                }
                Ok(client.as_ref().expect("client initialized above").clone())
            }
            Self::Ready(client) => Ok(client.clone()),
        }
    }
}

impl Installer {
    pub fn new(storage: Storage) -> Result<Self, InstallerError> {
        Self::new_with_reporter(storage, Arc::new(NoProgressReporter))
    }

    pub fn new_with_reporter(
        storage: Storage,
        reporter: Arc<dyn ProgressReporter>,
    ) -> Result<Self, InstallerError> {
        Ok(Self {
            storage,
            platform: Platform::current()?,
            http: HttpClientSource::Lazy(Mutex::new(None)),
            reporter,
        })
    }

    #[must_use]
    pub fn with_client(storage: Storage, platform: Platform, http: Arc<dyn HttpClient>) -> Self {
        Self::with_client_and_reporter(storage, platform, http, Arc::new(NoProgressReporter))
    }

    #[must_use]
    pub fn with_client_and_reporter(
        storage: Storage,
        platform: Platform,
        http: Arc<dyn HttpClient>,
        reporter: Arc<dyn ProgressReporter>,
    ) -> Self {
        Self {
            storage,
            platform,
            http: HttpClientSource::Ready(http),
            reporter,
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

    pub fn install_tool(
        &self,
        request: &ToolRequest,
        node_version: &Version,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.install_tool_inner(request, node_version, false)
    }

    pub fn repair_tool(
        &self,
        request: &ToolRequest,
        node_version: &Version,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.install_tool_inner(request, node_version, true)
    }

    pub fn install_plugin_tool(
        &self,
        release: &PluginToolArchive,
        repair: bool,
    ) -> Result<PluginToolInstallOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let target = format!(
            "{}@{} via {}",
            release.tool, release.version, release.provider
        );
        self.report_stage(ProgressAction::Resolve, &target);
        let destination = self.storage.plugin_tool_version_dir(
            &release.provider,
            &release.tool,
            &release.version,
        );
        let lock =
            self.plugin_tool_install_lock(&release.provider, &release.tool, &release.version)?;
        FileExt::lock(&lock).map_err(InstallerError::Io)?;

        if plugin_tool_commands_exist(&destination, &release.commands) {
            self.report_stage(ProgressAction::Reuse, &target);
            return Ok(PluginToolInstallOutcome {
                provider: release.provider.clone(),
                tool: release.tool.clone(),
                version: release.version.clone(),
                path: destination,
                commands: release.commands.clone(),
                downloaded: false,
            });
        }
        if destination.exists() {
            if !repair {
                return Err(InstallerError::CorruptPluginToolInstallation { path: destination });
            }
            let expected_parent = self
                .storage
                .plugin_tool_dir(&release.provider, &release.tool);
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
        let tool_parent = self
            .storage
            .plugin_tool_dir(&release.provider, &release.tool);
        fs::create_dir_all(&tool_parent).map_err(InstallerError::Io)?;
        let stage = tempfile::Builder::new()
            .prefix(".jolter-plugin-tool-install-")
            .tempdir_in(&tool_parent)
            .map_err(InstallerError::Io)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(InstallerError::Io)?;
        self.report_stage(ProgressAction::Extract, &target);
        extract_archive(
            &archive,
            &payload,
            release.artifact.format,
            release.artifact.strip_components,
        )?;

        for command in &release.commands {
            let executable = plugin_tool_executable(&payload, command);
            if !executable.is_file() {
                return Err(InstallerError::ExecutableMissing { path: executable });
            }
            make_executable(&executable)?;
        }
        write_plugin_tool_manifest(&payload, release)?;
        self.report_stage(ProgressAction::Publish, &target);
        fs::rename(&payload, &destination).map_err(|source| InstallerError::Publish {
            path: destination.clone(),
            source,
        })?;

        Ok(PluginToolInstallOutcome {
            provider: release.provider.clone(),
            tool: release.tool.clone(),
            version: release.version.clone(),
            path: destination,
            commands: release.commands.clone(),
            downloaded: true,
        })
    }

    pub fn validate_installed_tool(
        &self,
        tool: &InstalledTool,
        node_version: &Version,
    ) -> Result<(), InstallerError> {
        let path = tool.path.join("package.json");
        if !path.is_file() {
            return Ok(());
        }
        let contents = fs::read_to_string(&path).map_err(|source| {
            InstallerError::InstalledPackageMetadataRead {
                path: path.clone(),
                source,
            }
        })?;
        let metadata: InstalledPackageMetadata =
            serde_json::from_str(&contents).map_err(|source| {
                InstallerError::InstalledPackageMetadataParse {
                    path: path.clone(),
                    source,
                }
            })?;
        validate_node_engine(
            tool.kind,
            &tool.version,
            metadata.engines.node.as_deref(),
            node_version,
        )
    }

    pub fn uninstall_runtime(
        &self,
        kind: RuntimeKind,
        version: &Version,
    ) -> Result<RemovalOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let install = self.install_lock(kind, version)?;
        FileExt::lock(&install).map_err(InstallerError::Io)?;
        let path = self.storage.runtime_version_dir(kind, version);
        remove_installation(
            &self.storage,
            path,
            &self.storage.runtime_dir(kind),
            InstallationType::Runtime,
        )
    }

    pub fn uninstall_tool(
        &self,
        kind: ToolKind,
        version: &Version,
    ) -> Result<RemovalOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let install = self.tool_install_lock(kind, version)?;
        FileExt::lock(&install).map_err(InstallerError::Io)?;
        let path = self.storage.tool_version_dir(kind, version);
        remove_installation(
            &self.storage,
            path,
            &self.storage.tool_dir(kind),
            InstallationType::Tool,
        )
    }

    pub fn uninstall_plugin_tool(
        &self,
        provider: &str,
        tool: &str,
        version: &Version,
    ) -> Result<RemovalOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let install = self.plugin_tool_install_lock(provider, tool, version)?;
        FileExt::lock(&install).map_err(InstallerError::Io)?;
        let path = self
            .storage
            .plugin_tool_version_dir(provider, tool, version);
        remove_installation(
            &self.storage,
            path,
            &self.storage.plugin_tool_dir(provider, tool),
            InstallationType::Tool,
        )
    }

    pub fn clean_cache(&self) -> Result<CacheCleanOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock(&maintenance).map_err(InstallerError::Io)?;
        let mut outcome = CacheCleanOutcome::default();
        for name in ["downloads", "metadata"] {
            let path = self.storage.cache_dir().join(name);
            let stats = self.storage.path_stats(&path)?;
            outcome.removed_files = outcome.removed_files.saturating_add(stats.files);
            outcome.reclaimed_bytes = outcome.reclaimed_bytes.saturating_add(stats.bytes);
            if path.exists() {
                fs::remove_dir_all(&path).map_err(|source| InstallerError::CacheCleanup {
                    path: path.clone(),
                    source,
                })?;
            }
            fs::create_dir_all(&path).map_err(InstallerError::Io)?;
        }
        Ok(outcome)
    }

    fn install_tool_inner(
        &self,
        request: &ToolRequest,
        node_version: &Version,
        repair: bool,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let requested = request.to_string();
        self.report_stage(ProgressAction::Resolve, &requested);
        let release = self.resolve_tool(request)?;
        let target = format!("{}@{}", release.kind, release.version);
        validate_node_engine(
            release.kind,
            &release.version,
            release.node_engine.as_deref(),
            node_version,
        )?;
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
        let verified_archive = if let Some(hash) = &release.expected_hash {
            release.artifact.validate()?;
            let archive = self.obtain_archive(&release.artifact)?;
            self.report_stage(ProgressAction::Verify, &release.artifact.file_name);
            verify_tool_hash(&archive, hash)?;
            Some(archive)
        } else {
            None
        };
        if executable.is_file() {
            self.report_stage(ProgressAction::Reuse, &target);
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

        let archive = if let Some(archive) = verified_archive {
            archive
        } else {
            release.artifact.validate()?;
            self.obtain_archive(&release.artifact)?
        };
        let tool_parent = self.storage.tool_dir(release.kind);
        let stage = tempfile::Builder::new()
            .prefix(".jolter-tool-install-")
            .tempdir_in(&tool_parent)
            .map_err(InstallerError::Io)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(InstallerError::Io)?;
        self.report_stage(ProgressAction::Extract, &target);
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
        self.report_stage(ProgressAction::Publish, &target);
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
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let requested = request.to_string();
        self.report_stage(ProgressAction::Resolve, &requested);
        let release = self.resolve(request)?;
        let target = format!("{}@{}", release.kind, release.version);
        let destination = self
            .storage
            .runtime_version_dir(release.kind, &release.version);
        let lock = self.install_lock(release.kind, &release.version)?;
        FileExt::lock(&lock).map_err(InstallerError::Io)?;

        let executable = runtime_executable_in(&destination, release.kind);
        if executable.is_file() {
            self.report_stage(ProgressAction::Reuse, &target);
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
        self.report_stage(ProgressAction::Extract, &target);
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
        self.report_stage(ProgressAction::Publish, &target);
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

    fn resolve_github(
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

    fn tool_install_lock(&self, kind: ToolKind, version: &Version) -> Result<File, InstallerError> {
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

    fn plugin_tool_install_lock(
        &self,
        provider: &str,
        tool: &str,
        version: &Version,
    ) -> Result<File, InstallerError> {
        let safe_provider = provider.replace(['@', '/'], "_");
        let path = self
            .storage
            .cache_dir()
            .join("locks")
            .join(format!("plugin-tool-{safe_provider}-{tool}-{version}.lock"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(InstallerError::Io)?;
        }
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(InstallerError::Io)
    }

    fn maintenance_lock(&self) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("maintenance.lock"))
            .map_err(InstallerError::Io)
    }

    fn metadata_lock(&self, cache_key: &str) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("metadata-{cache_key}.lock")))
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
            self.report_stage(ProgressAction::Verify, &artifact.file_name);
            if artifact.integrity.verify(&path).is_ok() {
                self.reporter.report(ProgressEvent::CacheHit {
                    name: &artifact.file_name,
                });
                return Ok(path);
            }
            fs::remove_file(&path).map_err(InstallerError::Io)?;
        }

        let temporary = tempfile::Builder::new()
            .prefix(".jolter-download-")
            .tempfile_in(&directory)
            .map_err(InstallerError::Io)?;
        self.http.get()?.download(
            &artifact.url,
            temporary.path(),
            &artifact.file_name,
            self.reporter.as_ref(),
        )?;
        self.report_stage(ProgressAction::Verify, &artifact.file_name);
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
        let metadata_lock = self.metadata_lock(&cache_key)?;
        FileExt::lock(&metadata_lock).map_err(InstallerError::Io)?;
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
            self.http.get()?.get_npm_metadata(url)
        } else {
            self.http.get()?.get_text(url)
        };
        match response {
            Ok(contents) => {
                write_cache_file(&cache_path, contents.as_bytes())?;
                Ok(contents)
            }
            Err(error) => cached.ok_or(error),
        }
    }

    fn report_stage(&self, action: ProgressAction, target: &str) {
        self.reporter
            .report(ProgressEvent::Stage { action, target });
    }
}

#[derive(Debug, Clone, Copy)]
enum InstallationType {
    Runtime,
    Tool,
}

fn remove_installation(
    storage: &Storage,
    path: PathBuf,
    expected_parent: &Path,
    installation_type: InstallationType,
) -> Result<RemovalOutcome, InstallerError> {
    if path.parent() != Some(expected_parent) {
        return Err(InstallerError::UnsafeRemoval { path });
    }
    if !path.exists() {
        return Err(match installation_type {
            InstallationType::Runtime => InstallerError::RuntimeNotInstalled { path },
            InstallationType::Tool => InstallerError::ToolNotInstalled { path },
        });
    }
    let CacheStats { bytes, .. } = storage.path_stats(&path)?;
    fs::remove_dir_all(&path).map_err(|source| InstallerError::RemoveInstallation {
        path: path.clone(),
        source,
    })?;
    Ok(RemovalOutcome {
        path,
        reclaimed_bytes: bytes,
    })
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
    #[serde(default)]
    engines: NpmEngines,
}

#[derive(Debug, Default, Deserialize)]
struct NpmEngines {
    node: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NpmDistribution {
    tarball: String,
    integrity: String,
}

#[derive(Debug, Deserialize)]
struct InstalledPackageMetadata {
    #[serde(default)]
    engines: NpmEngines,
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
    let mut buffer = vec![0_u8; 256 * 1024];
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
    let mut buffer = vec![0_u8; 256 * 1024];
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

fn verify_tool_hash(path: &Path, expected: &ToolHash) -> Result<(), InstallerError> {
    let actual = match expected.algorithm {
        ToolHashAlgorithm::Sha1 => digest_hex::<Sha1>(path)?,
        ToolHashAlgorithm::Sha224 => digest_hex::<Sha224>(path)?,
        ToolHashAlgorithm::Sha256 => digest_hex::<Sha256>(path)?,
        ToolHashAlgorithm::Sha384 => digest_hex::<Sha384>(path)?,
        ToolHashAlgorithm::Sha512 => digest_hex::<Sha512>(path)?,
    };
    if actual.eq_ignore_ascii_case(&expected.value) {
        Ok(())
    } else {
        Err(InstallerError::ToolHashMismatch {
            algorithm: expected.algorithm,
            expected: expected.value.clone(),
            actual,
        })
    }
}

fn digest_hex<D: Digest>(path: &Path) -> Result<String, InstallerError> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut file = File::open(path).map_err(InstallerError::Io)?;
    let mut hasher = D::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(InstallerError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let output = hasher.finalize();
    let mut hex = String::with_capacity(output.len() * 2);
    for byte in output {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(hex)
}

fn validate_node_engine(
    kind: ToolKind,
    tool_version: &Version,
    requirement: Option<&str>,
    node_version: &Version,
) -> Result<(), InstallerError> {
    let Some(requirement) = requirement.filter(|value| !value.trim().is_empty()) else {
        return Ok(());
    };
    let range =
        NodeRange::parse(requirement).map_err(|source| InstallerError::InvalidNodeEngineRange {
            tool: kind,
            version: tool_version.clone(),
            requirement: requirement.to_owned(),
            details: source.to_string(),
        })?;
    let node = NodeVersion::from((node_version.major, node_version.minor, node_version.patch));
    if range.satisfies(&node) {
        Ok(())
    } else {
        Err(InstallerError::IncompatibleNodeVersion {
            tool: kind,
            version: tool_version.clone(),
            requirement: requirement.to_owned(),
            node_version: node_version.clone(),
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
    let reader = BufReader::with_capacity(128 * 1024, file);
    let mut zip = zip::ZipArchive::new(reader).map_err(InstallerError::Zip)?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(InstallerError::ArchiveEntryLimit);
    }
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
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
            if created_dirs.insert(output.clone()) {
                fs::create_dir_all(&output).map_err(InstallerError::Io)?;
            }
            continue;
        }
        if let Some(parent) = output.parent() {
            if created_dirs.insert(parent.to_path_buf()) {
                fs::create_dir_all(parent).map_err(InstallerError::Io)?;
            }
        }
        let output_file = File::create(&output).map_err(InstallerError::Io)?;
        let mut writer = BufWriter::with_capacity(64 * 1024, output_file);
        io::copy(&mut entry, &mut writer).map_err(InstallerError::Io)?;
        writer.flush().map_err(InstallerError::Io)?;
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
    let reader = BufReader::with_capacity(128 * 1024, file);
    let decoder = GzDecoder::new(reader);
    let mut tar = tar::Archive::new(decoder);
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
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
            if created_dirs.insert(output.clone()) {
                fs::create_dir_all(&output).map_err(InstallerError::Io)?;
            }
        } else if entry_type.is_file() {
            let size = entry.header().size().map_err(InstallerError::Io)?;
            extracted = extracted
                .checked_add(size)
                .ok_or(InstallerError::ArchiveSizeLimit)?;
            if extracted > MAX_ARCHIVE_BYTES {
                return Err(InstallerError::ArchiveSizeLimit);
            }
            if let Some(parent) = output.parent() {
                if created_dirs.insert(parent.to_path_buf()) {
                    fs::create_dir_all(parent).map_err(InstallerError::Io)?;
                }
            }
            let output_file = File::create(&output).map_err(InstallerError::Io)?;
            let mut writer = BufWriter::with_capacity(64 * 1024, output_file);
            io::copy(&mut entry, &mut writer).map_err(InstallerError::Io)?;
            writer.flush().map_err(InstallerError::Io)?;
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

fn plugin_tool_executable(root: &Path, command: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let exe = root.join(format!("{command}.exe"));
        if exe.is_file() {
            return exe;
        }
    }
    root.join(command)
}

fn plugin_tool_commands_exist(root: &Path, commands: &[String]) -> bool {
    !commands.is_empty()
        && commands
            .iter()
            .all(|command| plugin_tool_executable(root, command).is_file())
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
    tool: String,
    version: String,
    artifact_url: &'a str,
    integrity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_engine: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_hash: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginToolInstallManifest<'a> {
    provider: &'a str,
    tool: &'a str,
    version: String,
    url: &'a str,
    sha256: &'a str,
    archive_format: &'static str,
    strip_components: usize,
    commands: &'a [String],
}

fn write_tool_manifest(destination: &Path, release: &ToolRelease) -> Result<(), InstallerError> {
    let manifest = ToolInstallManifest {
        tool: release.kind.to_string(),
        version: release.version.to_string(),
        artifact_url: &release.artifact.url,
        integrity: release.artifact.integrity.to_string(),
        node_engine: release.node_engine.as_deref(),
        tool_hash: release.expected_hash.as_ref().map(ToString::to_string),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(InstallerError::Manifest)?;
    fs::write(destination.join(".jolter-tool.json"), contents).map_err(InstallerError::Io)
}

fn write_plugin_tool_manifest(
    destination: &Path,
    release: &PluginToolArchive,
) -> Result<(), InstallerError> {
    let ArtifactIntegrity::Sha256(sha256) = &release.artifact.integrity else {
        return Err(InstallerError::UnsupportedIntegrity(
            release.artifact.integrity.to_string(),
        ));
    };
    let manifest = PluginToolInstallManifest {
        provider: &release.provider,
        tool: &release.tool,
        version: release.version.to_string(),
        url: &release.artifact.url,
        sha256,
        archive_format: match release.artifact.format {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::TarGz => "tar.gz",
        },
        strip_components: release.artifact.strip_components,
        commands: &release.commands,
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(InstallerError::Manifest)?;
    fs::write(destination.join(".jolter-plugin-tool.json"), contents).map_err(InstallerError::Io)
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

    #[derive(Default)]
    struct RecordingReporter {
        events: Mutex<Vec<String>>,
    }

    impl ProgressReporter for RecordingReporter {
        fn report(&self, event: ProgressEvent<'_>) {
            let value = match event {
                ProgressEvent::Stage { action, target } => {
                    format!("stage:{}:{target}", action.label())
                }
                ProgressEvent::DownloadStarted { name, total } => {
                    format!("start:{name}:{total:?}")
                }
                ProgressEvent::DownloadAdvanced {
                    name,
                    downloaded,
                    total,
                } => format!("advance:{name}:{downloaded}:{total:?}"),
                ProgressEvent::DownloadFinished {
                    name,
                    downloaded,
                    total,
                } => format!("finish:{name}:{downloaded}:{total:?}"),
                ProgressEvent::CacheHit { name } => format!("cache:{name}"),
            };
            self.events.lock().unwrap().push(value);
        }
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

        fn download(
            &self,
            url: &str,
            destination: &Path,
            name: &str,
            reporter: &dyn ProgressReporter,
        ) -> Result<(), InstallerError> {
            let bytes = self.downloads.get(url).ok_or_else(|| {
                InstallerError::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no fake download for {url}"),
                ))
            })?;
            let total = Some(bytes.len() as u64);
            reporter.report(ProgressEvent::DownloadStarted { name, total });
            fs::write(destination, bytes).map_err(InstallerError::Io)?;
            *self.download_count.lock().unwrap() += 1;
            reporter.report(ProgressEvent::DownloadAdvanced {
                name,
                downloaded: bytes.len() as u64,
                total,
            });
            reporter.report(ProgressEvent::DownloadFinished {
                name,
                downloaded: bytes.len() as u64,
                total,
            });
            Ok(())
        }
    }

    #[test]
    fn reports_download_bytes_verification_and_cache_reuse() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        let bytes = b"verified archive".to_vec();
        let checksum = format!("{:x}", Sha256::digest(&bytes));
        let url = "https://example.test/archive.zip".to_owned();
        let client = Arc::new(FakeHttpClient {
            text: HashMap::new(),
            downloads: HashMap::from([(url.clone(), bytes.clone())]),
            text_count: Mutex::new(0),
            download_count: Mutex::new(0),
        });
        let reporter = Arc::new(RecordingReporter::default());
        let installer = Installer::with_client_and_reporter(
            storage,
            Platform::current().unwrap(),
            client,
            reporter.clone(),
        );
        let artifact = Artifact {
            url,
            integrity: ArtifactIntegrity::Sha256(checksum),
            file_name: "archive.zip".to_owned(),
            format: ArchiveFormat::Zip,
            strip_components: 0,
        };

        let first = installer.obtain_archive(&artifact).unwrap();
        let second = installer.obtain_archive(&artifact).unwrap();

        assert_eq!(first, second);
        let events = reporter.events.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|event| event == "start:archive.zip:Some(16)")
        );
        assert!(
            events
                .iter()
                .any(|event| event == "advance:archive.zip:16:Some(16)")
        );
        assert!(
            events
                .iter()
                .any(|event| event == "finish:archive.zip:16:Some(16)")
        );
        assert!(
            events
                .iter()
                .any(|event| event == "stage:verify:archive.zip")
        );
        assert!(events.iter().any(|event| event == "cache:archive.zip"));
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
        let linux_arm = Platform {
            os: OperatingSystem::Linux,
            arch: Architecture::Arm64,
            bun_cpu: BunCpu::Standard,
        };
        assert_eq!(
            GithubRuntime::Bun.asset_name(linux_arm).unwrap(),
            "bun-linux-aarch64.zip"
        );
        assert_eq!(
            GithubRuntime::Deno.asset_name(linux_arm).unwrap(),
            "deno-aarch64-unknown-linux-gnu.zip"
        );
        let mac_x64 = Platform {
            os: OperatingSystem::MacOs,
            arch: Architecture::X64,
            bun_cpu: BunCpu::Standard,
        };
        assert_eq!(
            GithubRuntime::Deno.asset_name(mac_x64).unwrap(),
            "deno-x86_64-apple-darwin.zip"
        );
        let unsupported = Platform {
            bun_cpu: BunCpu::Unsupported,
            ..windows
        };
        assert!(matches!(
            GithubRuntime::Bun.asset_name(unsupported),
            Err(InstallerError::UnsupportedBunCpu)
        ));

        for (platform, expected) in [
            (
                Platform {
                    os: OperatingSystem::Windows,
                    arch: Architecture::Arm64,
                    bun_cpu: BunCpu::Standard,
                },
                "win-arm64-zip",
            ),
            (
                Platform {
                    os: OperatingSystem::Linux,
                    arch: Architecture::X64,
                    bun_cpu: BunCpu::Standard,
                },
                "linux-x64",
            ),
            (linux_arm, "linux-arm64"),
            (mac_x64, "osx-x64-tar"),
            (
                Platform {
                    os: OperatingSystem::MacOs,
                    arch: Architecture::Arm64,
                    bun_cpu: BunCpu::Standard,
                },
                "osx-arm64-tar",
            ),
        ] {
            assert_eq!(node_target(platform).index_name, expected);
        }
    }

    #[test]
    fn resolves_node_lts_from_the_official_index_shape() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let platform = Platform::current().unwrap();
        let target = node_target(platform);
        let tagged_version = "v24.3.0";
        let file_name = format!(
            "node-{tagged_version}-{}.{}",
            target.archive_name,
            target.format.cache_extension()
        );
        let sums_url = format!("https://nodejs.org/dist/{tagged_version}/SHASUMS256.txt");
        let checksum = "b".repeat(64);
        let metadata = serde_json::json!([
            {
                "version": "v25.0.0",
                "lts": false,
                "files": [target.index_name]
            },
            {
                "version": tagged_version,
                "lts": "Krypton",
                "files": [target.index_name]
            }
        ])
        .to_string();
        let client = Arc::new(FakeHttpClient {
            text: HashMap::from([
                (NODE_INDEX_URL.to_owned(), metadata),
                (
                    sums_url,
                    format!("{checksum}  {file_name}\n{}  other.zip", "a".repeat(64)),
                ),
            ]),
            downloads: HashMap::new(),
            text_count: Mutex::new(0),
            download_count: Mutex::new(0),
        });
        let installer = Installer::with_client(storage, platform, client);

        let release = installer.resolve(&"node@lts".parse().unwrap()).unwrap();

        assert_eq!(release.kind, RuntimeKind::Node);
        assert_eq!(release.version, Version::new(24, 3, 0));
        assert_eq!(release.artifact.file_name, file_name);
        assert_eq!(
            release.artifact.integrity,
            ArtifactIntegrity::Sha256(checksum)
        );
    }

    #[test]
    fn resolves_bun_and_deno_fallback_checksums() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let platform = Platform {
            os: OperatingSystem::Windows,
            arch: Architecture::X64,
            bun_cpu: BunCpu::Standard,
        };
        let bun_asset = GithubRuntime::Bun.asset_name(platform).unwrap();
        let deno_asset = GithubRuntime::Deno.asset_name(platform).unwrap();
        let bun_download = format!("https://example.test/{bun_asset}");
        let deno_download = format!("https://example.test/{deno_asset}");
        let bun_sums = "https://example.test/SHASUMS256.txt".to_owned();
        let deno_sum = format!("https://example.test/{deno_asset}.sha256sum");
        let bun_checksum = "b".repeat(64);
        let deno_checksum = "d".repeat(64);
        let bun_metadata_url =
            format!("{GITHUB_API}/repos/oven-sh/bun/releases?per_page=100&page=1");
        let deno_metadata_url =
            format!("{GITHUB_API}/repos/denoland/deno/releases?per_page=100&page=1");
        let bun_metadata = serde_json::json!([{
            "tag_name": "bun-v1.3.2",
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": bun_asset.clone(),
                    "browser_download_url": bun_download,
                    "digest": null
                },
                {
                    "name": "SHASUMS256.txt",
                    "browser_download_url": bun_sums,
                    "digest": null
                }
            ]
        }])
        .to_string();
        let deno_metadata = serde_json::json!([{
            "tag_name": "v2.4.1",
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": deno_asset.clone(),
                    "browser_download_url": deno_download,
                    "digest": null
                },
                {
                    "name": format!("{deno_asset}.sha256sum"),
                    "browser_download_url": deno_sum,
                    "digest": null
                }
            ]
        }])
        .to_string();
        let client = Arc::new(FakeHttpClient {
            text: HashMap::from([
                (bun_metadata_url, bun_metadata),
                (
                    "https://example.test/SHASUMS256.txt".to_owned(),
                    format!("{bun_checksum} *{bun_asset}"),
                ),
                (deno_metadata_url, deno_metadata),
                (
                    format!("https://example.test/{deno_asset}.sha256sum"),
                    format!("{deno_checksum}  {deno_asset}"),
                ),
            ]),
            downloads: HashMap::new(),
            text_count: Mutex::new(0),
            download_count: Mutex::new(0),
        });
        let installer = Installer::with_client(storage, platform, client);

        let bun = installer.resolve(&"bun@1".parse().unwrap()).unwrap();
        let deno = installer.resolve(&"deno@2".parse().unwrap()).unwrap();

        assert_eq!(bun.version, Version::new(1, 3, 2));
        assert_eq!(
            bun.artifact.integrity,
            ArtifactIntegrity::Sha256(bun_checksum)
        );
        assert_eq!(deno.version, Version::new(2, 4, 1));
        assert_eq!(
            deno.artifact.integrity,
            ArtifactIntegrity::Sha256(deno_checksum)
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
        let tag_url = format!("{GITHUB_API}/repos/denoland/deno/releases/tags/v2.8.3");
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
        let single_metadata = serde_json::json!({
            "tag_name": "v2.8.3",
            "draft": false,
            "prerelease": false,
            "assets": [{
                "name": asset_name.clone(),
                "browser_download_url": download_url.clone(),
                "digest": format!("sha256:{checksum}")
            }]
        })
        .to_string();
        let client = Arc::new(FakeHttpClient {
            text: HashMap::from([(metadata_url, metadata), (tag_url, single_metadata)]),
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
        assert_eq!(*client.text_count.lock().unwrap(), 2);
        assert_eq!(*client.download_count.lock().unwrap(), 1);
    }

    #[test]
    fn installs_a_verified_tool_archive() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let archive = tar_gz_with_file("package/bin/pnpm.cjs", b"fake pnpm");
        let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(&archive)));
        let corepack_hash = format!("{:x}", Sha224::digest(&archive));
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
        let request: ToolRequest = format!("pnpm@10.2.0+sha224.{corepack_hash}")
            .parse()
            .unwrap();

        let outcome = installer
            .install_tool(&request, &Version::new(20, 0, 0))
            .unwrap();

        assert!(outcome.downloaded);
        assert_eq!(outcome.tool.version, Version::new(10, 2, 0));
        assert!(
            storage
                .tool_entrypoint(ToolKind::Pnpm, &outcome.tool.version, "pnpm")
                .unwrap()
                .is_file()
        );
        assert!(outcome.tool.path.join(".jolter-tool.json").is_file());
        let manifest = fs::read_to_string(outcome.tool.path.join(".jolter-tool.json")).unwrap();
        assert!(manifest.contains(&format!("sha224.{corepack_hash}")));
        assert_eq!(*client.download_count.lock().unwrap(), 1);

        let reused = installer
            .install_tool(&"pnpm@latest".parse().unwrap(), &Version::new(20, 0, 0))
            .unwrap();
        assert!(!reused.downloaded);
    }

    #[test]
    fn repairs_and_uninstalls_tools_and_cleans_cache() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        let archive = tar_gz_with_file("package/bin/pnpm.cjs", b"fake pnpm");
        let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(&archive)));
        let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
        let download_url = "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz".to_owned();
        let metadata = serde_json::json!({
            "dist-tags": { "latest": "10.2.0" },
            "versions": {
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
        let installer =
            Installer::with_client(storage.clone(), Platform::current().unwrap(), client);
        let version = Version::new(10, 2, 0);
        fs::create_dir_all(storage.tool_version_dir(ToolKind::Pnpm, &version)).unwrap();

        assert!(matches!(
            installer.install_tool(&"pnpm@10.2.0".parse().unwrap(), &Version::new(20, 0, 0)),
            Err(InstallerError::CorruptToolInstallation { .. })
        ));
        let repaired = installer
            .repair_tool(&"pnpm@10.2.0".parse().unwrap(), &Version::new(20, 0, 0))
            .unwrap();
        assert!(repaired.downloaded);
        let removed = installer.uninstall_tool(ToolKind::Pnpm, &version).unwrap();
        assert!(removed.reclaimed_bytes > 0);
        assert!(matches!(
            installer.uninstall_tool(ToolKind::Pnpm, &version),
            Err(InstallerError::ToolNotInstalled { .. })
        ));

        let metadata_cache = storage.cache_dir().join("metadata").join("orphan.txt");
        fs::create_dir_all(metadata_cache.parent().unwrap()).unwrap();
        fs::write(&metadata_cache, b"metadata").unwrap();
        let cleaned = installer.clean_cache().unwrap();
        assert!(cleaned.removed_files >= 2);
        assert!(!metadata_cache.exists());
    }

    #[test]
    fn concurrent_runtime_installation_downloads_once() {
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
                "name": asset_name,
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
        let first = Installer::with_client(storage.clone(), platform, client.clone());
        let second = Installer::with_client(storage, platform, client.clone());

        let first = thread::spawn(move || first.install(&"deno@2".parse().unwrap()).unwrap());
        let second = thread::spawn(move || second.install(&"deno@2".parse().unwrap()).unwrap());
        let outcomes = [first.join().unwrap(), second.join().unwrap()];

        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.downloaded).count(),
            1
        );
        assert_eq!(*client.download_count.lock().unwrap(), 1);
    }

    #[test]
    fn rejects_a_mismatched_corepack_hash_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let archive = tar_gz_with_file("package/bin/pnpm.cjs", b"fake pnpm");
        let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(&archive)));
        let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
        let download_url = "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz".to_owned();
        let metadata = serde_json::json!({
            "dist-tags": { "latest": "10.2.0" },
            "versions": {
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
        let installer =
            Installer::with_client(storage.clone(), Platform::current().unwrap(), client);
        let request: ToolRequest = format!("pnpm@10.2.0+sha224.{}", "0".repeat(56))
            .parse()
            .unwrap();

        let error = installer
            .install_tool(&request, &Version::new(20, 0, 0))
            .unwrap_err();

        assert!(matches!(error, InstallerError::ToolHashMismatch { .. }));
        assert!(
            !storage
                .tool_version_dir(ToolKind::Pnpm, &Version::new(10, 2, 0))
                .exists()
        );
    }

    #[test]
    fn rejects_a_tool_incompatible_with_selected_node() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
        let metadata = serde_json::json!({
            "dist-tags": { "latest": "11.6.0" },
            "versions": {
                "11.6.0": {
                    "version": "11.6.0",
                    "dist": {
                        "tarball": "https://registry.npmjs.org/pnpm/-/pnpm-11.6.0.tgz",
                        "integrity": format!("sha512-{}", BASE64.encode([0_u8; 64]))
                    },
                    "engines": {
                        "node": ">=22.13"
                    }
                }
            }
        })
        .to_string();
        let client = Arc::new(FakeHttpClient {
            text: HashMap::from([(metadata_url, metadata)]),
            downloads: HashMap::new(),
            text_count: Mutex::new(0),
            download_count: Mutex::new(0),
        });
        let installer =
            Installer::with_client(storage, Platform::current().unwrap(), client.clone());

        let error = installer
            .install_tool(&"pnpm@11.6.0".parse().unwrap(), &Version::new(20, 19, 0))
            .unwrap_err();

        assert!(matches!(
            error,
            InstallerError::IncompatibleNodeVersion { .. }
        ));
        assert_eq!(*client.download_count.lock().unwrap(), 0);
    }

    #[test]
    fn evaluates_npm_style_node_engine_ranges() {
        validate_node_engine(
            ToolKind::Pnpm,
            &Version::new(10, 2, 0),
            Some("^18.18.0 || >=20.9.0"),
            &Version::new(20, 10, 0),
        )
        .unwrap();

        let error = validate_node_engine(
            ToolKind::Pnpm,
            &Version::new(10, 2, 0),
            Some("^18.18.0 || >=20.9.0"),
            &Version::new(19, 0, 0),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            InstallerError::IncompatibleNodeVersion { .. }
        ));
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

    #[test]
    fn retries_only_transient_http_statuses_with_bounded_delays() {
        assert!(retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!retryable_status(StatusCode::NOT_FOUND));
        assert_eq!(retry_delay(0, None), Duration::from_millis(250));
        let retry_after = reqwest::header::HeaderValue::from_static("60");
        assert_eq!(retry_delay(0, Some(&retry_after)), Duration::from_secs(5));
    }

    #[test]
    fn validates_integrity_and_checksum_error_paths() {
        assert!(matches!(
            ArtifactIntegrity::from_sri("sha256-deadbeef"),
            Err(InstallerError::UnsupportedIntegrity(_))
        ));
        assert!(matches!(
            ArtifactIntegrity::from_sri("sha512-not-base64"),
            Err(InstallerError::InvalidIntegrity(_))
        ));
        assert!(matches!(
            Artifact {
                url: "https://example.test/archive.zip".to_owned(),
                integrity: ArtifactIntegrity::Sha256("a".repeat(64)),
                file_name: "../archive.zip".to_owned(),
                format: ArchiveFormat::Zip,
                strip_components: 0,
            }
            .validate(),
            Err(InstallerError::InvalidArtifactName(_))
        ));
        assert!(matches!(
            checksum_for("", "missing.zip"),
            Err(InstallerError::ChecksumNotFound { .. })
        ));
        assert!(matches!(
            parse_checksum_value(""),
            Err(InstallerError::EmptyChecksum)
        ));

        let temp = tempfile::NamedTempFile::new().unwrap();
        fs::write(temp.path(), b"contents").unwrap();
        assert!(matches!(
            verify_sha256(temp.path(), &"0".repeat(64)),
            Err(InstallerError::ChecksumMismatch { .. })
        ));
        let wrong_sha512 = BASE64.encode([0_u8; 64]);
        assert!(matches!(
            verify_sha512(temp.path(), &wrong_sha512),
            Err(InstallerError::ChecksumMismatch { .. })
        ));
        for algorithm in [
            ToolHashAlgorithm::Sha1,
            ToolHashAlgorithm::Sha256,
            ToolHashAlgorithm::Sha384,
            ToolHashAlgorithm::Sha512,
        ] {
            let expected = ToolHash {
                algorithm,
                value: "0".repeat(algorithm.hex_length()),
            };
            assert!(matches!(
                verify_tool_hash(temp.path(), &expected),
                Err(InstallerError::ToolHashMismatch { .. })
            ));
        }
    }

    #[test]
    fn tests_manifest_writing_and_error_display() {
        let temp = tempfile::tempdir().unwrap();
        let release = Release {
            kind: RuntimeKind::Node,
            version: Version::new(20, 0, 0),
            artifact: Artifact {
                url: "https://nodejs.org/dist/v20.0.0/node-v20.0.0-win-x64.zip".to_owned(),
                integrity: ArtifactIntegrity::Sha256("a".repeat(64)),
                file_name: "node.zip".to_owned(),
                format: ArchiveFormat::Zip,
                strip_components: 1,
            },
        };

        write_manifest(temp.path(), &release).unwrap();
        assert!(temp.path().join(".jolter-install.json").is_file());

        let tool_release = ToolRelease {
            kind: ToolKind::Pnpm,
            version: Version::new(10, 0, 0),
            artifact: Artifact {
                url: "https://registry.npmjs.org/pnpm/-/pnpm-10.0.0.tgz".to_owned(),
                integrity: ArtifactIntegrity::Sha256("b".repeat(64)),
                file_name: "pnpm.tgz".to_owned(),
                format: ArchiveFormat::TarGz,
                strip_components: 1,
            },
            node_engine: Some(">=18".to_owned()),
            expected_hash: None,
        };

        write_tool_manifest(temp.path(), &tool_release).unwrap();
        assert!(temp.path().join(".jolter-tool.json").is_file());

        let plugin_tool_archive = PluginToolArchive {
            provider: "my-provider".to_owned(),
            tool: "my-tool".to_owned(),
            version: Version::new(1, 0, 0),
            artifact: Artifact {
                url: "https://example.test/tool.zip".to_owned(),
                integrity: ArtifactIntegrity::Sha256("c".repeat(64)),
                file_name: "tool.zip".to_owned(),
                format: ArchiveFormat::Zip,
                strip_components: 0,
            },
            commands: vec!["my-tool".to_owned()],
        };

        write_plugin_tool_manifest(temp.path(), &plugin_tool_archive).unwrap();
        assert!(temp.path().join(".jolter-plugin-tool.json").is_file());

        let err = InstallerError::InsecureUrl("http://insecure.test".to_owned());
        assert_eq!(
            err.to_string(),
            "refusing non-HTTPS URL `http://insecure.test`"
        );

        let err = InstallerError::ArchiveSizeLimit;
        assert_eq!(err.to_string(), "archive exceeded the extracted size limit");
    }

    #[test]
    fn tests_unsafe_archive_paths_and_urls() {
        assert!(matches!(
            stripped_relative(Path::new("../outside.txt"), 0),
            Err(InstallerError::UnsafeArchivePath(_))
        ));
        assert!(matches!(
            stripped_relative(Path::new("C:\\windows\\system32"), 0),
            Err(InstallerError::UnsafeArchivePath(_))
        ));
        assert!(stripped_relative(Path::new("valid/sub/path.txt"), 0).is_ok());

        assert!(ensure_https("https://secure.test").is_ok());
        assert!(matches!(
            ensure_https("http://insecure.test"),
            Err(InstallerError::InsecureUrl(_))
        ));
    }

    #[test]
    fn tests_retry_helpers() {
        assert!(retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(retryable_status(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert!(retryable_status(reqwest::StatusCode::GATEWAY_TIMEOUT));
        assert!(retryable_status(reqwest::StatusCode::REQUEST_TIMEOUT));
        assert!(retryable_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!retryable_status(reqwest::StatusCode::OK));
        assert!(!retryable_status(reqwest::StatusCode::NOT_FOUND));

        // Exponential backoff: 250ms * 2^attempt
        let delay0 = retry_delay(0, None);
        assert_eq!(delay0, std::time::Duration::from_millis(250));
        let delay1 = retry_delay(1, None);
        assert_eq!(delay1, std::time::Duration::from_millis(500));
        let delay2 = retry_delay(2, None);
        assert_eq!(delay2, std::time::Duration::from_secs(1));

        // Retry-After header is respected (clamped to MAX_RETRY_AFTER = 5s)
        let header = reqwest::header::HeaderValue::from_static("3");
        let delay_header = retry_delay(0, Some(&header));
        assert_eq!(delay_header, std::time::Duration::from_secs(3));

        let header_large = reqwest::header::HeaderValue::from_static("60");
        let delay_clamped = retry_delay(0, Some(&header_large));
        assert_eq!(delay_clamped, MAX_RETRY_AFTER);
    }
}
