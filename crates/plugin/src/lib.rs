use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use jolter_storage::{InstalledPlugin, Storage};
use reqwest::{blocking::Client, redirect::Policy};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, bindgen},
};

pub const DEFAULT_REGISTRY_URL: &str = "https://registry.jolter.dev";
pub const PLUGIN_RELEASE_SCHEMA_URL: &str =
    "https://schemas.jolter.dev/plugin-release/v1/schema.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_WASM_BYTES: u64 = 128 * 1024 * 1024;

bindgen!({
    path: "wit",
    world: "jolter-plugin",
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRequest {
    pub name: String,
    pub selector: String,
}

impl PluginRequest {
    pub fn new(name: impl Into<String>, selector: impl Into<String>) -> Result<Self, PluginError> {
        let name = name.into();
        let selector = selector.into();
        validate_plugin_name(&name)?;
        validate_selector(&selector)?;
        Ok(Self { name, selector })
    }
}

impl std::str::FromStr for PluginRequest {
    type Err = PluginError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some((name, selector)) = value.rsplit_once('@').filter(|(name, _)| !name.is_empty())
        {
            Self::new(name, selector)
        } else {
            Self::new(value, "latest")
        }
    }
}

impl std::fmt::Display for PluginRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}@{}", self.name, self.selector)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginReleaseManifest {
    #[serde(default, rename = "$schema")]
    pub schema_url: Option<String>,
    pub schema_version: u32,
    pub name: String,
    pub version: String,
    pub jolter: JolterApiRequirement,
    pub entrypoint: PluginEntrypoint,
    pub provides: PluginProvides,
    #[serde(default)]
    pub permissions: PluginPermissions,
    pub artifacts: PluginArtifacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JolterApiRequirement {
    pub minimum_version: String,
    pub api_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEntrypoint {
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginProvides {
    #[serde(default)]
    pub tools: BTreeMap<String, PluginToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolDefinition {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub network: Option<NetworkPermissions>,
    #[serde(default)]
    pub filesystem: Option<FilesystemPermissions>,
    #[serde(default)]
    pub commands: Option<CommandPermissions>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPermissions {
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemPermissions {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandPermissions {
    #[serde(default)]
    pub execute: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginArtifacts {
    pub wasm: WasmArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmArtifact {
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPluginManifest {
    pub canonical_name: String,
    pub requested_name: String,
    pub version: String,
    pub registry_url: String,
    pub wasm_sha256: String,
    pub commands: Vec<String>,
    pub provides: PluginProvides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTool {
    pub name: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPlatform {
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginToolRelease {
    pub version: Version,
    pub url: String,
    pub sha256: String,
    pub archive_format: String,
    pub strip_components: usize,
    pub commands: Vec<String>,
}

pub struct PluginExecutor {
    engine: Engine,
}

impl PluginExecutor {
    pub fn new() -> Result<Self, PluginError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        Ok(Self {
            engine: Engine::new(&config).map_err(PluginError::Wasm)?,
        })
    }

    pub fn list_tools(&self, plugin_path: &Path) -> Result<Vec<PluginTool>, PluginError> {
        let (instance, mut store) = self.instantiate(plugin_path)?;
        let tools = instance
            .call_list_tools(&mut store)
            .map_err(PluginError::Wasm)?;
        Ok(tools
            .into_iter()
            .map(|tool| PluginTool {
                name: tool.name,
                commands: tool.commands,
            })
            .collect())
    }

    pub fn resolve_tool(
        &self,
        plugin_path: &Path,
        tool: &str,
        selector: &str,
        platform: PluginPlatform,
    ) -> Result<PluginToolRelease, PluginError> {
        let (instance, mut store) = self.instantiate(plugin_path)?;
        let release = instance
            .call_resolve_tool(
                &mut store,
                tool,
                selector,
                &Platform {
                    os: platform.os,
                    arch: platform.arch,
                },
            )
            .map_err(PluginError::Wasm)?;
        let version =
            Version::parse(release.version.trim_start_matches('v')).map_err(|source| {
                PluginError::InvalidPluginToolVersion {
                    value: release.version.clone(),
                    source,
                }
            })?;
        Ok(PluginToolRelease {
            version,
            url: release.url,
            sha256: release.sha256,
            archive_format: release.archive_format,
            strip_components: usize::try_from(release.strip_components).unwrap_or(usize::MAX),
            commands: release.commands,
        })
    }

    pub fn validate_installed(
        &self,
        plugin_path: &Path,
        tool: &str,
        version: &Version,
        root: &Path,
    ) -> Result<bool, PluginError> {
        let (instance, mut store) = self.instantiate(plugin_path)?;
        instance
            .call_validate_installed(
                &mut store,
                tool,
                &version.to_string(),
                &root.to_string_lossy(),
            )
            .map_err(PluginError::Wasm)
    }

    fn instantiate(&self, plugin_path: &Path) -> Result<(JolterPlugin, Store<()>), PluginError> {
        let component =
            Component::from_file(&self.engine, plugin_path).map_err(PluginError::Wasm)?;
        let linker = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());
        let instance = JolterPlugin::instantiate(&mut store, &component, &linker)
            .map_err(PluginError::Wasm)?;
        Ok((instance, store))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionsResponse {
    latest: Option<String>,
    versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AliasResponse {
    canonical: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseUrls {
    wasm_url: String,
    manifest_url: String,
    yanked: bool,
    deprecation_message: Option<String>,
}

pub trait RegistryHttp: Send + Sync {
    fn get_text(&self, url: &str) -> Result<String, PluginError>;
    fn download(&self, url: &str, destination: &Path) -> Result<(), PluginError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestRegistryHttp {
    client: Client,
}

impl ReqwestRegistryHttp {
    pub fn new() -> Result<Self, PluginError> {
        let client = Client::builder()
            .user_agent(concat!("jolter/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(30 * 60))
            .redirect(Policy::limited(5))
            .build()
            .map_err(PluginError::HttpClient)?;
        Ok(Self { client })
    }
}

impl RegistryHttp for ReqwestRegistryHttp {
    fn get_text(&self, url: &str) -> Result<String, PluginError> {
        ensure_https(url)?;
        let response = self
            .client
            .get(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|source| PluginError::Http {
                url: url.to_owned(),
                source,
            })?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MANIFEST_BYTES)
        {
            return Err(PluginError::ManifestTooLarge {
                url: url.to_owned(),
            });
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(PluginError::Io)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(PluginError::ManifestTooLarge {
                url: url.to_owned(),
            });
        }
        String::from_utf8(bytes).map_err(|source| PluginError::InvalidUtf8 {
            url: url.to_owned(),
            source,
        })
    }

    fn download(&self, url: &str, destination: &Path) -> Result<(), PluginError> {
        ensure_https(url)?;
        let response = self
            .client
            .get(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|source| PluginError::Http {
                url: url.to_owned(),
                source,
            })?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_WASM_BYTES)
        {
            return Err(PluginError::WasmTooLarge {
                url: url.to_owned(),
            });
        }
        let mut file = fs::File::create(destination).map_err(PluginError::Io)?;
        let copied = std::io::copy(&mut response.take(MAX_WASM_BYTES + 1), &mut file)
            .map_err(PluginError::Io)?;
        if copied > MAX_WASM_BYTES {
            return Err(PluginError::WasmTooLarge {
                url: url.to_owned(),
            });
        }
        Ok(())
    }
}

pub struct PluginManager<H = ReqwestRegistryHttp> {
    storage: Storage,
    registry_url: String,
    http: H,
}

impl PluginManager<ReqwestRegistryHttp> {
    pub fn new(storage: Storage) -> Result<Self, PluginError> {
        let registry_url = registry_url_from_env();
        Ok(Self::with_registry(
            storage,
            &registry_url,
            ReqwestRegistryHttp::new()?,
        ))
    }
}

impl<H: RegistryHttp> PluginManager<H> {
    #[must_use]
    pub fn with_registry(storage: Storage, registry_url: &str, http: H) -> Self {
        Self {
            storage,
            registry_url: registry_url.trim_end_matches('/').to_owned(),
            http,
        }
    }

    pub fn install(&self, request: &PluginRequest) -> Result<InstalledPlugin, PluginError> {
        self.storage.ensure_layout()?;
        let canonical = self.resolve_name(&request.name)?;
        let root_metadata = self.plugin_metadata(&canonical).ok();
        let version = self.select_version(&canonical, &request.selector)?;
        let release = self.release_urls(&canonical, &version)?;
        if release.yanked {
            return Err(PluginError::YankedVersion {
                name: canonical,
                version,
                message: release.deprecation_message,
            });
        }
        let manifest_text = self.absolute_get_text(&release.manifest_url)?;
        let manifest: PluginReleaseManifest =
            serde_json::from_str(&manifest_text).map_err(|source| PluginError::ManifestJson {
                url: release.manifest_url.clone(),
                source,
            })?;
        validate_release_manifest(&manifest, &canonical, &version)?;

        let destination = self.storage.plugin_version_dir(&canonical, &version);
        let wasm = destination.join(&manifest.entrypoint.path);
        if wasm.is_file() && verify_sha256(&wasm, &manifest.artifacts.wasm.sha256).is_ok() {
            return Ok(InstalledPlugin {
                canonical_name: canonical,
                version,
                path: destination,
            });
        }
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|source| PluginError::Remove {
                path: destination.clone(),
                source,
            })?;
        }
        fs::create_dir_all(&destination).map_err(|source| PluginError::Create {
            path: destination.clone(),
            source,
        })?;
        let temporary = tempfile::NamedTempFile::new_in(&destination).map_err(PluginError::Io)?;
        let temp_path = temporary.into_temp_path();
        self.absolute_download(&release.wasm_url, &temp_path)?;
        let metadata = fs::metadata(&temp_path).map_err(PluginError::Io)?;
        if metadata.len() != manifest.artifacts.wasm.size {
            return Err(PluginError::WasmSizeMismatch {
                expected: manifest.artifacts.wasm.size,
                actual: metadata.len(),
            });
        }
        verify_sha256(&temp_path, &manifest.artifacts.wasm.sha256)?;
        temp_path
            .persist(&wasm)
            .map_err(|error| PluginError::Persist {
                path: wasm.clone(),
                source: error.error,
            })?;
        fs::write(destination.join("plugin.release.json"), manifest_text)
            .map_err(PluginError::Io)?;
        if let Some(root_metadata) = root_metadata {
            fs::write(destination.join("plugin.registry.json"), root_metadata)
                .map_err(PluginError::Io)?;
        }
        let installed = InstalledPluginManifest {
            canonical_name: canonical.clone(),
            requested_name: request.name.clone(),
            version: version.to_string(),
            registry_url: self.registry_url.clone(),
            wasm_sha256: manifest.artifacts.wasm.sha256.clone(),
            commands: commands_from_provides(&manifest.provides),
            provides: manifest.provides,
        };
        let contents = serde_json::to_vec_pretty(&installed).map_err(PluginError::Json)?;
        fs::write(destination.join(".jolter-plugin.json"), contents).map_err(PluginError::Io)?;
        Ok(InstalledPlugin {
            canonical_name: canonical,
            version,
            path: destination,
        })
    }

    pub fn resolve_name(&self, name: &str) -> Result<String, PluginError> {
        if name.starts_with('@') {
            validate_plugin_name(name)?;
            return Ok(name.to_ascii_lowercase());
        }
        let url = format!("{}/api/v1/resolve/{}", self.registry_url, encode_path(name));
        let text = self.http.get_text(&url)?;
        let response: AliasResponse = serde_json::from_str(&text)
            .map_err(|source| PluginError::ManifestJson { url, source })?;
        validate_plugin_name(&response.canonical)?;
        Ok(response.canonical.to_ascii_lowercase())
    }

    pub fn select_version(&self, canonical: &str, selector: &str) -> Result<Version, PluginError> {
        let url = format!(
            "{}/api/v1/plugins/{}/versions",
            self.registry_url,
            encode_path(canonical)
        );
        let text = self.http.get_text(&url)?;
        let response: VersionsResponse = serde_json::from_str(&text)
            .map_err(|source| PluginError::ManifestJson { url, source })?;
        let selected = if selector.eq_ignore_ascii_case("latest") {
            response.latest
        } else {
            response
                .versions
                .into_iter()
                .filter_map(|value| Version::parse(&value).ok())
                .filter(|version| selector_matches(selector, version))
                .max()
                .map(|version| version.to_string())
        }
        .ok_or_else(|| PluginError::VersionNotFound {
            name: canonical.to_owned(),
            selector: selector.to_owned(),
        })?;
        Version::parse(&selected).map_err(|source| PluginError::InvalidVersion {
            value: selected,
            source,
        })
    }

    fn release_urls(&self, canonical: &str, version: &Version) -> Result<ReleaseUrls, PluginError> {
        let url = format!(
            "{}/api/v1/plugins/{}/releases/{}",
            self.registry_url,
            encode_path(canonical),
            version
        );
        let text = self.http.get_text(&url)?;
        serde_json::from_str(&text).map_err(|source| PluginError::ManifestJson { url, source })
    }

    fn plugin_metadata(&self, canonical: &str) -> Result<String, PluginError> {
        let url = format!(
            "{}/api/v1/plugins/{}",
            self.registry_url,
            encode_path(canonical)
        );
        self.http.get_text(&url)
    }

    fn absolute_get_text(&self, url: &str) -> Result<String, PluginError> {
        self.http.get_text(&absolute_url(&self.registry_url, url)?)
    }

    fn absolute_download(&self, url: &str, destination: &Path) -> Result<(), PluginError> {
        self.http
            .download(&absolute_url(&self.registry_url, url)?, destination)
    }
}

pub fn read_installed_manifest(path: &Path) -> Result<InstalledPluginManifest, PluginError> {
    let contents = fs::read_to_string(path.join(".jolter-plugin.json")).map_err(PluginError::Io)?;
    serde_json::from_str(&contents).map_err(PluginError::Json)
}

#[must_use]
pub fn commands_from_provides(provides: &PluginProvides) -> Vec<String> {
    let mut commands = provides
        .tools
        .values()
        .flat_map(|tool| tool.commands.iter().cloned())
        .collect::<Vec<_>>();
    commands.sort();
    commands.dedup();
    commands
}

fn registry_url_from_env() -> String {
    std::env::var("JOLTER_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_owned())
}

fn validate_release_manifest(
    manifest: &PluginReleaseManifest,
    canonical: &str,
    version: &Version,
) -> Result<(), PluginError> {
    if manifest.schema_version != 1 {
        return Err(PluginError::UnsupportedSchema(manifest.schema_version));
    }
    if let Some(schema_url) = &manifest.schema_url
        && schema_url != PLUGIN_RELEASE_SCHEMA_URL
    {
        return Err(PluginError::SchemaUrlMismatch {
            found: schema_url.clone(),
            expected: PLUGIN_RELEASE_SCHEMA_URL.to_owned(),
        });
    }
    if manifest.name.to_ascii_lowercase() != canonical {
        return Err(PluginError::ManifestIdentity {
            expected: canonical.to_owned(),
            actual: manifest.name.clone(),
        });
    }
    if manifest.version != version.to_string() {
        return Err(PluginError::ManifestVersion {
            expected: version.to_string(),
            actual: manifest.version.clone(),
        });
    }
    if manifest.entrypoint.kind != "wasm"
        || manifest.entrypoint.path != manifest.artifacts.wasm.file
    {
        return Err(PluginError::InvalidEntrypoint);
    }
    if manifest.artifacts.wasm.sha256.len() != 64
        || !manifest
            .artifacts
            .wasm
            .sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(PluginError::InvalidSha256(
            manifest.artifacts.wasm.sha256.clone(),
        ));
    }
    if manifest
        .permissions
        .commands
        .as_ref()
        .is_some_and(|commands| commands.execute)
    {
        return Err(PluginError::CommandExecutionUnsupported);
    }
    Ok(())
}

fn validate_plugin_name(value: &str) -> Result<(), PluginError> {
    let valid_scoped = value.starts_with('@')
        && value.split('/').count() == 2
        && value
            .trim_start_matches('@')
            .split('/')
            .all(valid_identifier_component);
    let valid_alias = !value.starts_with('@') && valid_identifier_component(value);
    if valid_scoped || valid_alias {
        Ok(())
    } else {
        Err(PluginError::InvalidName(value.to_owned()))
    }
}

fn valid_identifier_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        && value
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}

fn validate_selector(value: &str) -> Result<(), PluginError> {
    if value.eq_ignore_ascii_case("latest") || selector_components_valid(value) {
        Ok(())
    } else {
        Err(PluginError::InvalidSelector(value.to_owned()))
    }
}

fn selector_components_valid(value: &str) -> bool {
    let components = value.split('.').collect::<Vec<_>>();
    !components.is_empty()
        && components.len() <= 3
        && components.iter().all(|component| {
            component.eq_ignore_ascii_case("x")
                || *component == "*"
                || (!component.is_empty()
                    && component
                        .chars()
                        .all(|character| character.is_ascii_digit()))
        })
}

fn selector_matches(selector: &str, version: &Version) -> bool {
    if selector.eq_ignore_ascii_case("latest")
        || selector == "*"
        || selector.eq_ignore_ascii_case("x")
    {
        return true;
    }
    let parts = selector
        .split('.')
        .filter(|part| !part.eq_ignore_ascii_case("x") && *part != "*")
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>();
    match parts.as_deref() {
        Ok([major]) => version.major == *major,
        Ok([major, minor]) => version.major == *major && version.minor == *minor,
        Ok([major, minor, patch]) => {
            version.major == *major && version.minor == *minor && version.patch == *patch
        }
        _ => false,
    }
}

fn absolute_url(registry_url: &str, value: &str) -> Result<String, PluginError> {
    if value.starts_with("https://") {
        Ok(value.to_owned())
    } else if value.starts_with('/') {
        Ok(format!("{}{}", registry_url.trim_end_matches('/'), value))
    } else {
        Err(PluginError::InvalidUrl(value.to_owned()))
    }
}

fn encode_path(value: &str) -> String {
    value.replace('@', "%40").replace('/', "%2F")
}

fn ensure_https(url: &str) -> Result<(), PluginError> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err(PluginError::InsecureUrl(url.to_owned()))
    }
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), PluginError> {
    let bytes = fs::read(path).map_err(PluginError::Io)?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == expected.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(PluginError::ChecksumMismatch {
            expected: expected.to_owned(),
            actual,
        })
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plugin_requests() {
        let request: PluginRequest = "eslint@1.x".parse().unwrap();
        assert_eq!(request.name, "eslint");
        assert_eq!(request.selector, "1.x");
        let request: PluginRequest = "@eslint/eslint".parse().unwrap();
        assert_eq!(request.selector, "latest");
    }

    #[test]
    fn validates_manifest_identity_and_commands() {
        let manifest = PluginReleaseManifest {
            schema_url: Some(PLUGIN_RELEASE_SCHEMA_URL.to_owned()),
            schema_version: 1,
            name: "@eslint/eslint".to_owned(),
            version: "1.2.0".to_owned(),
            jolter: JolterApiRequirement {
                minimum_version: "0.3.0".to_owned(),
                api_version: "1".to_owned(),
            },
            entrypoint: PluginEntrypoint {
                kind: "wasm".to_owned(),
                path: "plugin.wasm".to_owned(),
            },
            provides: PluginProvides {
                tools: BTreeMap::from([(
                    "eslint".to_owned(),
                    PluginToolDefinition {
                        display_name: None,
                        description: None,
                        commands: vec!["eslint".to_owned()],
                    },
                )]),
            },
            permissions: PluginPermissions::default(),
            artifacts: PluginArtifacts {
                wasm: WasmArtifact {
                    file: "plugin.wasm".to_owned(),
                    sha256: "a".repeat(64),
                    size: 1,
                },
            },
        };
        validate_release_manifest(
            &manifest,
            "@eslint/eslint",
            &Version::parse("1.2.0").unwrap(),
        )
        .unwrap();
        assert_eq!(commands_from_provides(&manifest.provides), ["eslint"]);
    }

    #[test]
    fn parses_jdt_release_manifest_shape() {
        let manifest: PluginReleaseManifest = serde_json::from_str(
            r#"{
                "$schema": "https://schemas.jolter.dev/plugin-release/v1/schema.json",
                "schemaVersion": 1,
                "name": "@jolter-example/hello-tool",
                "version": "1.2.3",
                "repository": {
                    "type": "github",
                    "owner": "jolterjs",
                    "repo": "jolter-plugin-hello-tool"
                },
                "release": { "tag": "v1.2.3", "commit": "UNKNOWN_COMMIT" },
                "jolter": { "minimumVersion": "0.3.0", "apiVersion": "1" },
                "entrypoint": { "type": "wasm", "path": "plugin.wasm" },
                "provides": {
                    "tools": {
                        "hello-tool": {
                            "displayName": "Hello Tool",
                            "description": "A deterministic fixture tool.",
                            "commands": ["hello-tool"]
                        }
                    }
                },
                "permissions": {
                    "network": { "allowedHosts": ["downloads.example.test"] },
                    "filesystem": {
                        "read": ["project"],
                        "write": ["jolter-cache", "jolter-tools"]
                    },
                    "commands": { "execute": false }
                },
                "artifacts": {
                    "wasm": {
                        "file": "plugin.wasm",
                        "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                        "size": 8
                    }
                }
            }"#,
        )
        .unwrap();

        validate_release_manifest(
            &manifest,
            "@jolter-example/hello-tool",
            &Version::parse("1.2.3").unwrap(),
        )
        .unwrap();
        assert_eq!(commands_from_provides(&manifest.provides), ["hello-tool"]);
    }

    #[test]
    fn rejects_mismatched_release_schema_url() {
        let manifest: PluginReleaseManifest = serde_json::from_str(
            r#"{
                "$schema": "https://schemas.jolter.dev/plugin/v1/schema.json",
                "schemaVersion": 1,
                "name": "@jolter-example/hello-tool",
                "version": "1.2.3",
                "jolter": { "minimumVersion": "0.3.0", "apiVersion": "1" },
                "entrypoint": { "type": "wasm", "path": "plugin.wasm" },
                "provides": { "tools": { "hello-tool": { "commands": ["hello-tool"] } } },
                "artifacts": {
                    "wasm": {
                        "file": "plugin.wasm",
                        "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                        "size": 8
                    }
                }
            }"#,
        )
        .unwrap();

        assert!(matches!(
            validate_release_manifest(
                &manifest,
                "@jolter-example/hello-tool",
                &Version::parse("1.2.3").unwrap(),
            ),
            Err(PluginError::SchemaUrlMismatch { .. })
        ));
    }
}
