pub mod error;
pub mod executor;
pub mod http;
pub mod manager;
pub mod manifest;
pub mod validation;

pub const DEFAULT_REGISTRY_URL: &str = "https://registry.jolter.dev";
pub const PLUGIN_RELEASE_SCHEMA_URL: &str =
    "https://schemas.jolter.dev/plugin-release/v1/schema.json";
pub(crate) const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_WASM_BYTES: u64 = 128 * 1024 * 1024;

pub use error::PluginError;
pub use executor::PluginExecutor;
pub use http::{RegistryHttp, ReqwestRegistryHttp};
pub use manager::{PluginManager, PluginRequest};
pub use manifest::{
    CommandPermissions, FilesystemPermissions, InstalledPluginManifest, JolterApiRequirement,
    NetworkPermissions, PluginArtifacts, PluginEntrypoint, PluginPermissions, PluginPlatform,
    PluginProvides, PluginReleaseManifest, PluginTool, PluginToolDefinition, PluginToolRelease,
    WasmArtifact, commands_from_provides, read_installed_manifest,
};

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use jolter_storage::Storage;
    use semver::Version;
    use sha2::Digest;

    use super::*;
    use validation::{
        absolute_url, ensure_https, selector_matches, validate_plugin_name,
        validate_release_manifest, validate_selector,
    };

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

    #[derive(Default)]
    struct MockRegistryHttp {
        responses: std::sync::Mutex<BTreeMap<String, String>>,
        downloads: std::sync::Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl MockRegistryHttp {
        fn set_text(&self, url: impl Into<String>, content: impl Into<String>) {
            self.responses
                .lock()
                .unwrap()
                .insert(url.into(), content.into());
        }

        fn set_download(&self, url: impl Into<String>, bytes: Vec<u8>) {
            self.downloads.lock().unwrap().insert(url.into(), bytes);
        }
    }

    impl RegistryHttp for MockRegistryHttp {
        fn get_text(&self, url: &str) -> Result<String, PluginError> {
            ensure_https(url)?;
            self.responses
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| PluginError::InsecureUrl(url.to_owned()))
        }

        fn download(&self, url: &str, destination: &Path) -> Result<(), PluginError> {
            ensure_https(url)?;
            let bytes = self
                .downloads
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| PluginError::InsecureUrl(url.to_owned()))?;
            fs::write(destination, bytes).map_err(PluginError::Io)
        }
    }

    #[test]
    fn tests_constructors_and_http_helpers() {
        assert!(PluginExecutor::new().is_ok());
        assert!(ReqwestRegistryHttp::new().is_ok());
    }

    #[test]
    fn tests_plugin_request_display_and_validation() {
        let req = PluginRequest::new("eslint", "1.x").unwrap();
        assert_eq!(format!("{req}"), "eslint@1.x");
        assert!(PluginRequest::new("invalid name!", "1.0").is_err());
        assert!(PluginRequest::new("eslint", "invalid selector!").is_err());

        assert!(validate_plugin_name("eslint").is_ok());
        assert!(validate_plugin_name("@eslint/eslint").is_ok());
        assert!(validate_plugin_name("invalid name!").is_err());
        assert!(validate_plugin_name("@foo/bar/baz").is_err());

        assert!(validate_selector("latest").is_ok());
        assert!(validate_selector("1.x").is_ok());
        assert!(validate_selector("1.2.3").is_ok());
        assert!(validate_selector("invalid").is_err());
    }

    #[test]
    fn tests_selector_matching_and_urls() {
        let v1_2_3 = Version::parse("1.2.3").unwrap();
        assert!(selector_matches("latest", &v1_2_3));
        assert!(selector_matches("*", &v1_2_3));
        assert!(selector_matches("1", &v1_2_3));
        assert!(selector_matches("1.2", &v1_2_3));
        assert!(selector_matches("1.2.3", &v1_2_3));
        assert!(!selector_matches("2", &v1_2_3));
        assert!(!selector_matches("1.3", &v1_2_3));

        assert_eq!(
            absolute_url("https://registry.test", "https://other.test/wasm").unwrap(),
            "https://other.test/wasm"
        );
        assert_eq!(
            absolute_url("https://registry.test", "/api/v1/test").unwrap(),
            "https://registry.test/api/v1/test"
        );
        assert!(absolute_url("https://registry.test", "relative/path").is_err());

        assert!(ensure_https("https://secure.test").is_ok());
        assert!(ensure_https("http://insecure.test").is_err());
    }

    #[test]
    fn tests_manifest_validation_error_paths() {
        let mut manifest = PluginReleaseManifest {
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
            provides: PluginProvides::default(),
            permissions: PluginPermissions::default(),
            artifacts: PluginArtifacts {
                wasm: WasmArtifact {
                    file: "plugin.wasm".to_owned(),
                    sha256: "a".repeat(64),
                    size: 1,
                },
            },
        };

        manifest.schema_version = 2;
        assert!(matches!(
            validate_release_manifest(
                &manifest,
                "@eslint/eslint",
                &Version::parse("1.2.0").unwrap()
            ),
            Err(PluginError::UnsupportedSchema(2))
        ));

        manifest.schema_version = 1;
        manifest.name = "@eslint/other".to_owned();
        assert!(matches!(
            validate_release_manifest(
                &manifest,
                "@eslint/eslint",
                &Version::parse("1.2.0").unwrap()
            ),
            Err(PluginError::ManifestIdentity { .. })
        ));

        manifest.name = "@eslint/eslint".to_owned();
        manifest.version = "1.3.0".to_owned();
        assert!(matches!(
            validate_release_manifest(
                &manifest,
                "@eslint/eslint",
                &Version::parse("1.2.0").unwrap()
            ),
            Err(PluginError::ManifestVersion { .. })
        ));

        manifest.version = "1.2.0".to_owned();
        manifest.entrypoint.kind = "js".to_owned();
        assert!(matches!(
            validate_release_manifest(
                &manifest,
                "@eslint/eslint",
                &Version::parse("1.2.0").unwrap()
            ),
            Err(PluginError::InvalidEntrypoint)
        ));

        manifest.entrypoint.kind = "wasm".to_owned();
        manifest.artifacts.wasm.sha256 = "invalid_hash".to_owned();
        assert!(matches!(
            validate_release_manifest(
                &manifest,
                "@eslint/eslint",
                &Version::parse("1.2.0").unwrap()
            ),
            Err(PluginError::InvalidSha256(_))
        ));

        manifest.artifacts.wasm.sha256 = "a".repeat(64);
        manifest.permissions.commands = Some(CommandPermissions { execute: true });
        assert!(matches!(
            validate_release_manifest(
                &manifest,
                "@eslint/eslint",
                &Version::parse("1.2.0").unwrap()
            ),
            Err(PluginError::CommandExecutionUnsupported)
        ));
    }

    #[test]
    fn tests_plugin_manager_lifecycle_with_mock() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp_dir.path().join("storage"));
        let http = MockRegistryHttp::default();
        let registry = "https://registry.test";

        http.set_text(
            format!("{registry}/api/v1/resolve/eslint"),
            r#"{"canonical":"@eslint/eslint"}"#,
        );
        http.set_text(
            format!("{registry}/api/v1/plugins/%40eslint%2Feslint/versions"),
            r#"{"latest":"1.2.0","versions":["1.0.0","1.2.0"]}"#,
        );
        http.set_text(
            format!("{registry}/api/v1/plugins/%40eslint%2Feslint/releases/1.2.0"),
            r#"{
                "wasmUrl": "https://registry.test/wasm/eslint-1.2.0.wasm",
                "manifestUrl": "https://registry.test/manifests/eslint-1.2.0.json",
                "yanked": false
            }"#,
        );
        http.set_text(
            "https://registry.test/manifests/eslint-1.2.0.json",
            r#"{
                "$schema": "https://schemas.jolter.dev/plugin-release/v1/schema.json",
                "schemaVersion": 1,
                "name": "@eslint/eslint",
                "version": "1.2.0",
                "jolter": { "minimumVersion": "0.3.0", "apiVersion": "1" },
                "entrypoint": { "type": "wasm", "path": "plugin.wasm" },
                "provides": { "tools": { "eslint": { "commands": ["eslint"] } } },
                "artifacts": {
                    "wasm": {
                        "file": "plugin.wasm",
                        "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
                        "size": 5
                    }
                }
            }"#,
        );
        http.set_download(
            "https://registry.test/wasm/eslint-1.2.0.wasm",
            b"hello".to_vec(),
        );

        let manager = PluginManager::with_registry(storage, registry, http);
        let req = PluginRequest::new("eslint", "latest").unwrap();
        let installed = manager.install(&req).unwrap();

        assert_eq!(installed.canonical_name, "@eslint/eslint");
        assert_eq!(installed.version, Version::new(1, 2, 0));
        assert!(installed.path.join("plugin.wasm").is_file());

        let manifest = read_installed_manifest(&installed.path).unwrap();
        assert_eq!(manifest.canonical_name, "@eslint/eslint");
        assert_eq!(manifest.commands, vec!["eslint"]);
    }

    #[test]
    fn tests_executor_instantiation_and_error_paths() {
        let executor = PluginExecutor::new().unwrap();
        let invalid_wasm = Path::new("/nonexistent/path/plugin.wasm");

        assert!(executor.list_tools(invalid_wasm).is_err());

        assert!(
            executor
                .resolve_tool(
                    invalid_wasm,
                    "eslint",
                    "latest",
                    PluginPlatform {
                        os: "linux".to_owned(),
                        arch: "x64".to_owned(),
                    }
                )
                .is_err()
        );

        assert!(
            executor
                .validate_installed(
                    invalid_wasm,
                    "eslint",
                    &Version::new(1, 0, 0),
                    Path::new("/root")
                )
                .is_err()
        );
    }

    #[test]
    fn tests_reqwest_registry_http_and_serde() {
        let client = ReqwestRegistryHttp::new().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("out.wasm");

        let err_get = client.get_text("http://insecure.test");
        assert!(matches!(err_get, Err(PluginError::InsecureUrl(_))));

        let err_dl = client.download("http://insecure.test", &dest);
        assert!(matches!(err_dl, Err(PluginError::InsecureUrl(_))));

        let err_conn_get = client.get_text("https://127.0.0.1:1");
        assert!(matches!(err_conn_get, Err(PluginError::Http { .. })));

        let err_conn_dl = client.download("https://127.0.0.1:1", &dest);
        assert!(matches!(err_conn_dl, Err(PluginError::Http { .. })));

        let v_resp: http::VersionsResponse =
            serde_json::from_str(r#"{"latest":"1.0.0","versions":["1.0.0"]}"#).unwrap();
        assert_eq!(v_resp.latest, Some("1.0.0".to_owned()));
        assert_eq!(v_resp.versions, vec!["1.0.0"]);

        let alias_resp: http::AliasResponse =
            serde_json::from_str(r#"{"canonical":"@scope/name"}"#).unwrap();
        assert_eq!(alias_resp.canonical, "@scope/name");

        let release_urls: http::ReleaseUrls = serde_json::from_str(
            r#"{
                "wasmUrl": "https://a.test/w",
                "manifestUrl": "https://a.test/m",
                "yanked": false,
                "deprecationMessage": "deprecated"
            }"#,
        )
        .unwrap();
        assert_eq!(release_urls.wasm_url, "https://a.test/w");
        assert_eq!(release_urls.manifest_url, "https://a.test/m");
        assert!(!release_urls.yanked);
        assert_eq!(
            release_urls.deprecation_message,
            Some("deprecated".to_owned())
        );
    }

    #[test]
    fn tests_plugin_error_display_paths() {
        let err = PluginError::InsecureUrl("http://insecure.test".to_owned());
        assert!(err.to_string().contains("http://insecure.test"));

        let err = PluginError::VersionNotFound {
            name: "pkg".to_owned(),
            selector: "1.0".to_owned(),
        };
        assert!(err.to_string().contains("pkg"));

        let err = PluginError::YankedVersion {
            name: "pkg".to_owned(),
            version: Version::new(1, 0, 0),
            message: Some("reason".to_owned()),
        };
        assert!(err.to_string().contains("yanked"));

        let err = PluginError::WasmTooLarge {
            url: "https://url.test".to_owned(),
        };
        assert!(err.to_string().contains("large"));

        let err = PluginError::ManifestTooLarge {
            url: "https://url.test".to_owned(),
        };
        assert!(err.to_string().contains("large"));

        let err = PluginError::InvalidPluginToolVersion {
            value: "bad_ver".to_owned(),
            source: Version::parse("bad_ver").unwrap_err(),
        };
        assert!(err.to_string().contains("bad_ver"));

        let err = PluginError::InvalidEntrypoint;
        assert!(err.to_string().contains("entrypoint"));

        let err = PluginError::CommandExecutionUnsupported;
        assert!(err.to_string().contains("permissions"));

        let err = PluginError::ChecksumMismatch {
            expected: "exp_hash".to_owned(),
            actual: "act_hash".to_owned(),
        };
        assert!(err.to_string().contains("exp_hash"));

        let err = PluginError::UnsupportedSchema(99);
        assert!(err.to_string().contains("99"));
    }

    struct FakeRegistryHttp {
        responses: std::collections::HashMap<String, String>,
        downloads: std::collections::HashMap<String, Vec<u8>>,
    }

    impl http::RegistryHttp for FakeRegistryHttp {
        fn get_text(&self, url: &str) -> Result<String, PluginError> {
            self.responses
                .get(url)
                .cloned()
                .ok_or_else(|| PluginError::InsecureUrl(url.to_owned()))
        }

        fn download(&self, url: &str, destination: &Path) -> Result<(), PluginError> {
            let bytes = self
                .downloads
                .get(url)
                .ok_or_else(|| PluginError::InsecureUrl(url.to_owned()))?;
            fs::write(destination, bytes).map_err(PluginError::Io)
        }
    }

    #[test]
    fn tests_plugin_executor_error_paths() {
        let executor = PluginExecutor::new().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let dummy_wasm = temp.path().join("invalid.wasm");
        fs::write(&dummy_wasm, b"not a wasm module").unwrap();

        assert!(executor.list_tools(&dummy_wasm).is_err());
        assert!(
            executor
                .resolve_tool(
                    &dummy_wasm,
                    "tool",
                    "1.0",
                    PluginPlatform {
                        os: "linux".to_owned(),
                        arch: "x64".to_owned()
                    }
                )
                .is_err()
        );
        assert!(
            executor
                .validate_installed(&dummy_wasm, "tool", &Version::new(1, 0, 0), temp.path())
                .is_err()
        );
    }

    #[test]
    fn tests_plugin_manager_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        let mut responses = std::collections::HashMap::new();
        responses.insert(
            "https://registry.jolter.dev/api/v1/resolve/eslint".to_owned(),
            r#"{"canonical":"@jolter/eslint"}"#.to_owned(),
        );
        responses.insert(
            "https://registry.jolter.dev/api/v1/plugins/%40jolter%2Feslint/versions".to_owned(),
            r#"{"latest":"1.2.3","versions":["1.0.0","1.2.3"]}"#.to_owned(),
        );
        responses.insert(
            "https://registry.jolter.dev/api/v1/plugins/%40jolter%2Feslint/releases/1.2.3".to_owned(),
            r#"{"manifestUrl":"https://registry.jolter.dev/manifest.json","wasmUrl":"https://registry.jolter.dev/plugin.wasm","yanked":false,"deprecationMessage":null}"#.to_owned(),
        );

        let wasm_bytes = b"\x00asm\x01\x00\x00\x00";
        let sha256_hex = format!("{:x}", sha2::Sha256::digest(wasm_bytes));

        let manifest_json = format!(
            r#"{{
                "schemaVersion": 1,
                "name": "@jolter/eslint",
                "version": "1.2.3",
                "jolter": {{ "minimumVersion": "0.4.0", "apiVersion": "1.0.0" }},
                "entrypoint": {{ "type": "wasm", "path": "plugin.wasm" }},
                "provides": {{ "tools": {{}} }},
                "artifacts": {{ "wasm": {{ "file": "plugin.wasm", "sha256": "{sha256_hex}", "size": {} }} }}
            }}"#,
            wasm_bytes.len()
        );

        responses.insert(
            "https://registry.jolter.dev/manifest.json".to_owned(),
            manifest_json,
        );

        let mut downloads = std::collections::HashMap::new();
        downloads.insert(
            "https://registry.jolter.dev/plugin.wasm".to_owned(),
            wasm_bytes.to_vec(),
        );

        let fake_http = FakeRegistryHttp {
            responses,
            downloads,
        };

        let manager =
            PluginManager::with_registry(storage, "https://registry.jolter.dev", fake_http);

        let canonical = manager.resolve_name("eslint").unwrap();
        assert_eq!(canonical, "@jolter/eslint");

        let ver = manager.select_version("@jolter/eslint", "latest").unwrap();
        assert_eq!(ver, Version::new(1, 2, 3));

        let req = PluginRequest::new("eslint", "1.2.3").unwrap();
        let installed = manager.install(&req).unwrap();
        assert_eq!(installed.canonical_name, "@jolter/eslint");
        assert_eq!(installed.version, Version::new(1, 2, 3));

        assert!(matches!(
            manager.select_version("@jolter/eslint", "99.x"),
            Err(PluginError::VersionNotFound { .. })
        ));
    }

    #[test]
    fn tests_plugin_manager_yanked_and_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());

        let mut responses = std::collections::HashMap::new();
        responses.insert(
            "https://registry.jolter.dev/api/v1/plugins/%40jolter%2Feslint/versions".to_owned(),
            r#"{"latest":"1.0.0","versions":["1.0.0"]}"#.to_owned(),
        );
        responses.insert(
            "https://registry.jolter.dev/api/v1/plugins/%40jolter%2Feslint/releases/1.0.0".to_owned(),
            r#"{"manifestUrl":"https://registry.jolter.dev/manifest.json","wasmUrl":"https://registry.jolter.dev/plugin.wasm","yanked":true,"deprecationMessage":"yanked"}"#.to_owned(),
        );

        let fake_http = FakeRegistryHttp {
            responses,
            downloads: std::collections::HashMap::new(),
        };

        let manager =
            PluginManager::with_registry(storage.clone(), "https://registry.jolter.dev", fake_http);

        let req = PluginRequest::new("@jolter/eslint", "1.0.0").unwrap();
        assert!(matches!(
            manager.install(&req),
            Err(PluginError::YankedVersion { .. })
        ));

        let missing_file = temp.path().join("non_existent_file.wasm");
        assert!(matches!(
            validation::verify_sha256(
                &missing_file,
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            Err(PluginError::Io(_))
        ));

        let sample_file = temp.path().join("sample.wasm");
        fs::write(&sample_file, b"sample_content").unwrap();
        assert!(matches!(
            validation::verify_sha256(
                &sample_file,
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            Err(PluginError::ChecksumMismatch { .. })
        ));

        assert!(matches!(
            validation::ensure_https("http://insecure.test"),
            Err(PluginError::InsecureUrl(_))
        ));

        assert!(matches!(
            validation::absolute_url("https://registry.dev", "invalid_url_no_slash"),
            Err(PluginError::InvalidUrl(_))
        ));

        let executor = PluginExecutor::new().unwrap();
        assert!(executor.list_tools(Path::new("/nonexistent/wasm")).is_err());

        let mut fake_http_ver = FakeRegistryHttp {
            responses: std::collections::HashMap::new(),
            downloads: std::collections::HashMap::new(),
        };
        fake_http_ver.responses.insert(
            "https://registry.jolter.dev/api/v1/plugins/%40jolter%2Feslint/versions".to_string(),
            r#"{"latest":null,"versions":[]}"#.to_string(),
        );
        let mgr_ver = PluginManager::with_registry(
            storage.clone(),
            "https://registry.jolter.dev",
            fake_http_ver,
        );
        assert!(matches!(
            mgr_ver.select_version("@jolter/eslint", "latest"),
            Err(PluginError::VersionNotFound { .. })
        ));

        let mut fake_http_bad_ver = FakeRegistryHttp {
            responses: std::collections::HashMap::new(),
            downloads: std::collections::HashMap::new(),
        };
        fake_http_bad_ver.responses.insert(
            "https://registry.jolter.dev/api/v1/plugins/%40jolter%2Feslint/versions".to_string(),
            r#"{"latest":"not_a_version","versions":["not_a_version"]}"#.to_string(),
        );
        let mgr_bad_ver =
            PluginManager::with_registry(storage, "https://registry.jolter.dev", fake_http_bad_ver);
        assert!(matches!(
            mgr_bad_ver.select_version("@jolter/eslint", "latest"),
            Err(PluginError::InvalidVersion { .. })
        ));
    }
}
