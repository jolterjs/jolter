use std::{fs, io::Read, path::Path, time::Duration};

use reqwest::{blocking::Client, redirect::Policy};
use serde::Deserialize;

use crate::{MAX_MANIFEST_BYTES, MAX_WASM_BYTES, error::PluginError, validation::ensure_https};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VersionsResponse {
    pub latest: Option<String>,
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AliasResponse {
    pub canonical: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseUrls {
    pub wasm_url: String,
    pub manifest_url: String,
    pub yanked: bool,
    pub deprecation_message: Option<String>,
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
