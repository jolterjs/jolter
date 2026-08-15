use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    error::ConfigError,
    runtime::RuntimeConfig,
    validation::{validate_numeric_selector, validate_plugin_name, validate_plugin_tool_name},
};

pub const CONFIG_FILE_NAME: &str = "jolter.json";
pub const CURRENT_SCHEMA_VERSION: u32 = 2;
pub const MIN_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_SCHEMA_BASE_URL: &str = "https://schemas.jolter.dev/project";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default, rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema_url: Option<String>,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "RuntimeConfig::is_empty")]
    pub runtime: RuntimeConfig,
    #[serde(
        default,
        alias = "packageManager",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub tools: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugins: BTreeMap<String, String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            schema_url: Some(schema_url_for_version(CURRENT_SCHEMA_VERSION).to_owned()),
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig::default(),
            tools: BTreeMap::new(),
            plugins: BTreeMap::new(),
        }
    }
}

impl ProjectConfig {
    pub fn from_path(path: &Path) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Self =
            serde_json::from_str(&contents).map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        config.validate()?;
        Ok(config)
    }

    pub fn write_to(&self, path: &Path) -> Result<(), ConfigError> {
        let mut config = self.clone();
        config.schema_url = Some(schema_url_for_version(config.schema_version).to_owned());
        config.validate()?;
        let contents = serde_json::to_string_pretty(&config)
            .map_err(|source| ConfigError::Serialize { source })?;
        let parent = path.parent().ok_or_else(|| ConfigError::InvalidPath {
            path: path.to_path_buf(),
        })?;
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|source| ConfigError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        std::io::Write::write_all(&mut temporary, format!("{contents}\n").as_bytes()).map_err(
            |source| ConfigError::Write {
                path: path.to_path_buf(),
                source,
            },
        )?;
        temporary
            .persist(path)
            .map_err(|error| ConfigError::Write {
                path: path.to_path_buf(),
                source: error.error,
            })?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(MIN_SCHEMA_VERSION..=CURRENT_SCHEMA_VERSION).contains(&self.schema_version) {
            return Err(ConfigError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if let Some(schema_url) = &self.schema_url {
            let expected = schema_url_for_version(self.schema_version);
            if schema_url != expected {
                return Err(ConfigError::SchemaUrlMismatch {
                    found: schema_url.clone(),
                    expected: expected.to_owned(),
                });
            }
        }
        let configured_runtimes = self.runtime.configured_count();
        if configured_runtimes > 1 {
            return Err(ConfigError::MultipleRuntimes);
        }
        for (name, selector) in self
            .runtime
            .entries()
            .chain(
                self.tools
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )
            .chain(
                self.plugins
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )
        {
            if selector.trim().is_empty() {
                return Err(ConfigError::EmptySelector(name.to_owned()));
            }
        }
        if let Some((name, selector)) = self.runtime.entries().next() {
            jolter_runtime::RuntimeRequest::new(name.parse()?, selector)?;
        }
        for (name, selector) in &self.tools {
            if let Ok(kind) = name.parse() {
                jolter_runtime::ToolRequest::new(kind, selector)?;
            } else if self.schema_version < 2 || self.plugins.is_empty() {
                return Err(ConfigError::Tool(
                    jolter_runtime::ToolRequestError::UnsupportedTool(name.clone()),
                ));
            } else {
                validate_plugin_tool_name(name)?;
                validate_numeric_selector(selector).map_err(|()| {
                    ConfigError::InvalidPluginToolSelector {
                        name: name.clone(),
                        selector: selector.clone(),
                    }
                })?;
            }
        }
        for (name, selector) in &self.plugins {
            validate_plugin_name(name)?;
            validate_numeric_selector(selector).map_err(|()| {
                ConfigError::InvalidPluginSelector {
                    name: name.clone(),
                    selector: selector.clone(),
                }
            })?;
        }
        Ok(())
    }
}

const fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

#[must_use]
pub fn schema_url_for_version(version: u32) -> &'static str {
    match version {
        1 => "https://schemas.jolter.dev/project/v1/schema.json",
        _ => "https://schemas.jolter.dev/project/v2/schema.json",
    }
}
