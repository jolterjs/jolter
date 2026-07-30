use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bun: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deno: Option<String>,
}

impl RuntimeConfig {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.node.is_none() && self.bun.is_none() && self.deno.is_none()
    }

    #[must_use]
    pub fn configured_count(&self) -> usize {
        self.entries().count()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            ("node", self.node.as_deref()),
            ("bun", self.bun.as_deref()),
            ("deno", self.deno.as_deref()),
        ]
        .into_iter()
        .filter_map(|(name, selector)| selector.map(|selector| (name, selector)))
    }
}

#[must_use]
pub fn discover(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|directory| directory.join(CONFIG_FILE_NAME))
        .find(|candidate| candidate.is_file())
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read configuration at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid configuration at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize configuration: {source}")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write configuration at {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("only one runtime may be configured per project")]
    MultipleRuntimes,
    #[error(
        "unsupported jolter.json schema version {found}; this Jolter release supports version {supported}"
    )]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error("jolter.json $schema `{found}` does not match schemaVersion; expected `{expected}`")]
    SchemaUrlMismatch { found: String, expected: String },
    #[error("selector for `{0}` cannot be empty")]
    EmptySelector(String),
    #[error("invalid plugin name `{0}`")]
    InvalidPluginName(String),
    #[error("invalid plugin tool name `{0}`")]
    InvalidPluginToolName(String),
    #[error("invalid plugin selector `{selector}` for `{name}`")]
    InvalidPluginSelector { name: String, selector: String },
    #[error("invalid plugin tool selector `{selector}` for `{name}`")]
    InvalidPluginToolSelector { name: String, selector: String },
    #[error(transparent)]
    Tool(#[from] jolter_runtime::ToolRequestError),
    #[error(transparent)]
    Runtime(#[from] jolter_runtime::RuntimeRequestError),
    #[error("invalid configuration path {path}")]
    InvalidPath { path: PathBuf },
}

fn validate_plugin_name(value: &str) -> Result<(), ConfigError> {
    let valid_alias = !value.starts_with('@') && valid_component(value);
    let valid_scoped = value.starts_with('@')
        && value
            .trim_start_matches('@')
            .split_once('/')
            .is_some_and(|(scope, name)| valid_component(scope) && valid_component(name));
    if valid_alias || valid_scoped {
        Ok(())
    } else {
        Err(ConfigError::InvalidPluginName(value.to_owned()))
    }
}

fn validate_plugin_tool_name(value: &str) -> Result<(), ConfigError> {
    if valid_component(value) {
        Ok(())
    } else {
        Err(ConfigError::InvalidPluginToolName(value.to_owned()))
    }
}

fn valid_component(value: &str) -> bool {
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

fn validate_numeric_selector(selector: &str) -> Result<(), ()> {
    if selector.eq_ignore_ascii_case("latest") {
        return Ok(());
    }
    let components = selector.split('.').collect::<Vec<_>>();
    if components.is_empty() || components.len() > 3 {
        return Err(());
    }
    let mut wildcard_seen = false;
    for component in components {
        if component.eq_ignore_ascii_case("x") || component == "*" {
            wildcard_seen = true;
        } else if wildcard_seen
            || component.is_empty()
            || !component
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return Err(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_example() {
        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "runtime": { "node": "24.x" },
                "tools": { "pnpm": "10.x", "yarn": "4.x" }
            }"#,
        )
        .unwrap();

        config.validate().unwrap();
        assert_eq!(config.runtime.node.as_deref(), Some("24.x"));
        assert_eq!(config.tools.get("pnpm").map(String::as_str), Some("10.x"));
        assert_eq!(config.tools.get("yarn").map(String::as_str), Some("4.x"));
    }

    #[test]
    fn discovers_config_in_parent() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("one").join("two");
        fs::create_dir_all(&nested).unwrap();
        let expected = temp.path().join(CONFIG_FILE_NAME);
        fs::write(&expected, "{}").unwrap();

        assert_eq!(discover(&nested), Some(expected));
    }

    #[test]
    fn rejects_multiple_runtimes() {
        let config = ProjectConfig {
            schema_url: Some(schema_url_for_version(CURRENT_SCHEMA_VERSION).to_owned()),
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig {
                node: Some("24".to_owned()),
                bun: Some("1".to_owned()),
                deno: None,
            },
            tools: BTreeMap::new(),
            plugins: BTreeMap::new(),
        };

        assert!(matches!(
            config.validate(),
            Err(ConfigError::MultipleRuntimes)
        ));
    }

    #[test]
    fn accepts_multiple_tools_and_rejects_unknown_tools() {
        let config = ProjectConfig {
            schema_url: Some(schema_url_for_version(CURRENT_SCHEMA_VERSION).to_owned()),
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig::default(),
            tools: BTreeMap::from([
                ("pnpm".to_owned(), "10".to_owned()),
                ("yarn".to_owned(), "4".to_owned()),
            ]),
            plugins: BTreeMap::new(),
        };
        config.validate().unwrap();

        let config = ProjectConfig {
            schema_url: Some(schema_url_for_version(CURRENT_SCHEMA_VERSION).to_owned()),
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig::default(),
            tools: BTreeMap::from([("rush".to_owned(), "5".to_owned())]),
            plugins: BTreeMap::new(),
        };
        assert!(matches!(config.validate(), Err(ConfigError::Tool(_))));
    }

    #[test]
    fn reads_legacy_package_manager_key_and_writes_tools() {
        let config: ProjectConfig =
            serde_json::from_str(r#"{"packageManager":{"pnpm":"10"}}"#).unwrap();
        config.validate().unwrap();
        assert_eq!(config.tools.get("pnpm").map(String::as_str), Some("10"));

        let serialized = serde_json::to_string(&config).unwrap();
        assert!(serialized.contains(r#""tools":{"pnpm":"10"}"#));
        assert!(!serialized.contains("packageManager"));
    }

    #[test]
    fn defaults_configuration_to_current_schema_version() {
        let config: ProjectConfig = serde_json::from_str(r#"{"runtime":{"node":"24"}}"#).unwrap();

        assert_eq!(config.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(config.schema_url, None);
        config.validate().unwrap();
    }

    #[test]
    fn rejects_unknown_schema_versions() {
        let config: ProjectConfig =
            serde_json::from_str(r#"{"schemaVersion":3,"runtime":{"node":"24"}}"#).unwrap();

        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnsupportedSchemaVersion {
                found: 3,
                supported: 2
            })
        ));
    }

    #[test]
    fn accepts_matching_schema_url_and_rejects_mismatch() {
        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "$schema": "https://schemas.jolter.dev/project/v2/schema.json",
                "schemaVersion": 2,
                "runtime": { "node": "24" }
            }"#,
        )
        .unwrap();
        config.validate().unwrap();

        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "$schema": "https://schemas.jolter.dev/project/v1/schema.json",
                "schemaVersion": 2,
                "runtime": { "node": "24" }
            }"#,
        )
        .unwrap();

        assert!(matches!(
            config.validate(),
            Err(ConfigError::SchemaUrlMismatch { .. })
        ));
    }

    #[test]
    fn writes_current_schema_url() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(CONFIG_FILE_NAME);
        ProjectConfig::default().write_to(&path).unwrap();

        let contents = fs::read_to_string(path).unwrap();
        assert!(
            contents.contains(r#""$schema": "https://schemas.jolter.dev/project/v2/schema.json""#)
        );
    }

    #[test]
    fn accepts_schema_two_plugin_requirements() {
        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "schemaVersion": 2,
                "runtime": { "node": "24.x" },
                "tools": { "eslint": "8.x" },
                "plugins": { "eslint": "1.x" }
            }"#,
        )
        .unwrap();

        config.validate().unwrap();
        assert_eq!(
            config.plugins.get("eslint").map(String::as_str),
            Some("1.x")
        );
    }

    #[test]
    fn rejects_invalid_runtime_selectors_during_config_parsing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(CONFIG_FILE_NAME);
        fs::write(&path, r#"{"runtime":{"node":"not-semver"}}"#).unwrap();

        assert!(matches!(
            ProjectConfig::from_path(&path),
            Err(ConfigError::Runtime(_))
        ));
    }

    #[test]
    fn tests_config_error_display() {
        let err = ConfigError::UnsupportedSchemaVersion {
            found: 99,
            supported: 2,
        };
        assert!(err.to_string().contains("99"));

        let err = ConfigError::EmptySelector("node".to_owned());
        assert!(err.to_string().contains("node"));

        let err = ConfigError::InvalidPluginName("bad".to_owned());
        assert!(err.to_string().contains("bad"));

        let err = ConfigError::InvalidPluginToolName("bad".to_owned());
        assert!(err.to_string().contains("bad"));

        let err = ConfigError::InvalidPluginSelector {
            name: "p".to_owned(),
            selector: "bad".to_owned(),
        };
        assert!(
            err.to_string()
                .contains("invalid plugin selector `bad` for `p`")
        );

        let err = ConfigError::InvalidPluginToolSelector {
            name: "p".to_owned(),
            selector: "bad".to_owned(),
        };
        assert!(
            err.to_string()
                .contains("invalid plugin tool selector `bad` for `p`")
        );

        let err = ConfigError::InvalidPath {
            path: PathBuf::from("/invalid"),
        };
        assert!(err.to_string().contains("/invalid"));
    }
}
