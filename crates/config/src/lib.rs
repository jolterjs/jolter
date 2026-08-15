pub mod discovery;
pub mod error;
pub mod project;
pub mod runtime;
pub mod validation;

pub use discovery::discover;
pub use error::ConfigError;
pub use project::{
    CONFIG_FILE_NAME, CURRENT_SCHEMA_VERSION, MIN_SCHEMA_VERSION, PROJECT_SCHEMA_BASE_URL,
    ProjectConfig, schema_url_for_version,
};
pub use runtime::RuntimeConfig;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs, path::PathBuf};

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

        let err = ConfigError::Read {
            path: PathBuf::from("/read.json"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert!(err.to_string().contains("/read.json"));

        let err = ConfigError::Write {
            path: PathBuf::from("/write.json"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        };
        assert!(err.to_string().contains("/write.json"));
    }
}
