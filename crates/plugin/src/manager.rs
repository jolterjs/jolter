use std::{fs, path::Path};

use jolter_storage::{InstalledPlugin, Storage};
use semver::Version;

use crate::{
    DEFAULT_REGISTRY_URL,
    error::PluginError,
    http::{AliasResponse, RegistryHttp, ReleaseUrls, ReqwestRegistryHttp, VersionsResponse},
    manifest::{InstalledPluginManifest, PluginReleaseManifest, commands_from_provides},
    validation::{
        absolute_url, encode_path, selector_matches, validate_plugin_name,
        validate_release_manifest, validate_selector, verify_sha256,
    },
};

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

fn registry_url_from_env() -> String {
    std::env::var("JOLTER_REGISTRY_URL").unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_owned())
}
