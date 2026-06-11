use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CONFIG_FILE_NAME: &str = "jolter.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default, skip_serializing_if = "RuntimeConfig::is_empty")]
    pub runtime: RuntimeConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub package_manager: BTreeMap<String, String>,
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
        let configured_runtimes = self.runtime.configured_count();
        if configured_runtimes > 1 {
            return Err(ConfigError::MultipleRuntimes);
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
        Ok(())
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
    #[error("selector for `{0}` cannot be empty")]
    EmptySelector(String),
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
}
