use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CONFIG_FILE_NAME: &str = "jolter.json";
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "RuntimeConfig::is_empty")]
    pub runtime: RuntimeConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub package_manager: BTreeMap<String, String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig::default(),
            package_manager: BTreeMap::new(),
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
        self.validate()?;
        let contents = serde_json::to_string_pretty(self)
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
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        let configured_runtimes = self.runtime.configured_count();
        if configured_runtimes > 1 {
            return Err(ConfigError::MultipleRuntimes);
        }
        if self.package_manager.len() > 1 {
            return Err(ConfigError::MultiplePackageManagers);
        }
        for (name, selector) in self.runtime.entries().chain(
            self.package_manager
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        ) {
            if selector.trim().is_empty() {
                return Err(ConfigError::EmptySelector(name.to_owned()));
            }
        }
        if let Some((name, selector)) = self.runtime.entries().next() {
            jolter_runtime::RuntimeRequest::new(name.parse()?, selector)?;
        }
        if let Some((name, selector)) = self.package_manager.iter().next() {
            jolter_runtime::PackageManagerRequest::new(name.parse()?, selector)?;
        }
        Ok(())
    }
}

const fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
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
    #[error("only one package manager may be configured per project")]
    MultiplePackageManagers,
    #[error("selector for `{0}` cannot be empty")]
    EmptySelector(String),
    #[error(transparent)]
    PackageManager(#[from] jolter_runtime::PackageManagerRequestError),
    #[error(transparent)]
    Runtime(#[from] jolter_runtime::RuntimeRequestError),
    #[error("invalid configuration path {path}")]
    InvalidPath { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_example() {
        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "runtime": { "node": "24.x" },
                "packageManager": { "pnpm": "10.x" }
            }"#,
        )
        .unwrap();

        config.validate().unwrap();
        assert_eq!(config.runtime.node.as_deref(), Some("24.x"));
        assert_eq!(
            config.package_manager.get("pnpm").map(String::as_str),
            Some("10.x")
        );
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
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig {
                node: Some("24".to_owned()),
                bun: Some("1".to_owned()),
                deno: None,
            },
            package_manager: BTreeMap::new(),
        };

        assert!(matches!(
            config.validate(),
            Err(ConfigError::MultipleRuntimes)
        ));
    }

    #[test]
    fn rejects_multiple_or_unknown_package_managers() {
        let mut package_manager = BTreeMap::new();
        package_manager.insert("pnpm".to_owned(), "10".to_owned());
        package_manager.insert("yarn".to_owned(), "4".to_owned());
        let config = ProjectConfig {
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig::default(),
            package_manager,
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::MultiplePackageManagers)
        ));

        let config = ProjectConfig {
            schema_version: CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig::default(),
            package_manager: BTreeMap::from([("rush".to_owned(), "5".to_owned())]),
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::PackageManager(_))
        ));
    }

    #[test]
    fn defaults_legacy_configuration_to_schema_version_one() {
        let config: ProjectConfig = serde_json::from_str(r#"{"runtime":{"node":"24"}}"#).unwrap();

        assert_eq!(config.schema_version, CURRENT_SCHEMA_VERSION);
        config.validate().unwrap();
    }

    #[test]
    fn rejects_unknown_schema_versions() {
        let config: ProjectConfig =
            serde_json::from_str(r#"{"schemaVersion":2,"runtime":{"node":"24"}}"#).unwrap();

        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnsupportedSchemaVersion {
                found: 2,
                supported: 1
            })
        ));
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
}
