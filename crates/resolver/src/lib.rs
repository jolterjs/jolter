pub mod dev_engines;
pub mod discovery;
pub mod error;
pub mod parsers;
pub mod types;

use std::path::Path;

use jolter_config::ProjectConfig;

pub use dev_engines::{DevEngineItem, DevEngineOnFail, DevEngines};
use discovery::discover_project_files;
pub use error::ResolverError;
use parsers::{
    parse_node_file, parse_package_json, plugin_tools_from_config, plugins_from_config,
    runtime_from_config, tools_from_config,
};
pub use types::{
    ProjectResolution, RequirementSource, ResolvedPlugin, ResolvedPluginTool, ResolvedRuntime,
    ResolvedTool,
};

pub fn resolve(start: &Path) -> Result<ProjectResolution, ResolverError> {
    let start = start
        .canonicalize()
        .map_err(|source| ResolverError::Canonicalize {
            path: start.to_path_buf(),
            source,
        })?;

    let discovered = discover_project_files(&start);

    let config = discovered
        .config
        .as_deref()
        .map(ProjectConfig::from_path)
        .transpose()?;
    let project_root = discovered
        .config
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(&start)
        .to_path_buf();

    let (pkg_runtime, pkg_tools, dev_engines) = match discovered.package_json.as_deref() {
        Some(path) => parse_package_json(path)?,
        None => (None, Vec::new(), None),
    };

    let runtime = match config.as_ref().and_then(runtime_from_config).transpose()? {
        Some(runtime) => Some(runtime),
        None => match discovered.node_version {
            Some((path, source)) => parse_node_file(&path, source)?,
            None => pkg_runtime,
        },
    };

    let tools = match config.as_ref().map(tools_from_config).transpose()? {
        Some(tools) if !tools.is_empty() => tools,
        _ => pkg_tools,
    };
    let plugin_tools = config
        .as_ref()
        .map_or_else(Vec::new, plugin_tools_from_config);
    let plugins = config.as_ref().map_or_else(Vec::new, plugins_from_config);

    Ok(ProjectResolution {
        root: project_root,
        runtime,
        tools,
        plugin_tools,
        plugins,
        dev_engines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    #[test]
    fn jolter_config_takes_runtime_priority() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("jolter.json"),
            r#"{"runtime":{"node":"24.x"}}"#,
        )
        .unwrap();
        fs::write(temp.path().join(".node-version"), "22").unwrap();

        let resolution = resolve(temp.path()).unwrap();
        let runtime = resolution.runtime.unwrap();
        assert_eq!(runtime.request.selector, "24.x");
        assert_eq!(runtime.source, RequirementSource::JolterConfig);
    }

    #[test]
    fn resolves_package_json_tool_and_node_version_independently() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"pnpm@10.12.1"}"#,
        )
        .unwrap();
        fs::write(temp.path().join(".nvmrc"), "22").unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(resolution.runtime.unwrap().request.to_string(), "node@22");
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@10.12.1");
    }

    #[test]
    fn resolves_dev_engines_runtime_and_package_manager() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{
                "devEngines": {
                    "runtime": { "name": "node", "version": "^20.0.0", "onFail": "error" },
                    "packageManager": { "name": "pnpm", "version": "^11.17.0", "onFail": "download" }
                }
            }"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        let runtime = resolution.runtime.unwrap();
        assert_eq!(runtime.request.to_string(), "node@20.0.0");
        assert_eq!(runtime.source, RequirementSource::PackageJson);

        assert_eq!(resolution.tools.len(), 1);
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@11.17.0");
        assert_eq!(resolution.tools[0].source, RequirementSource::PackageJson);

        let dev_engines = resolution.dev_engines.unwrap();
        assert_eq!(dev_engines.runtime.len(), 1);
        assert_eq!(
            dev_engines.runtime[0].on_fail_mode(),
            DevEngineOnFail::Error
        );
        assert_eq!(dev_engines.package_manager.len(), 1);
        assert_eq!(
            dev_engines.package_manager[0].on_fail_mode(),
            DevEngineOnFail::Download
        );
    }

    #[test]
    fn resolves_dev_engines_array_and_cpu_os() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{
                "devEngines": {
                    "runtime": [{ "name": "node", "version": "22.0.0" }],
                    "packageManager": [
                        { "name": "pnpm", "version": "10.12.1" },
                        { "name": "yarn", "version": "4.0.0" }
                    ],
                    "cpu": { "name": "x64" },
                    "os": [{ "name": "darwin" }, { "name": "linux" }, { "name": "win32" }]
                }
            }"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(
            resolution.runtime.unwrap().request.to_string(),
            "node@22.0.0"
        );
        assert_eq!(resolution.tools.len(), 2);
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@10.12.1");
        assert_eq!(resolution.tools[1].request.to_string(), "yarn@4.0.0");

        let dev_engines = resolution.dev_engines.unwrap();
        assert_eq!(dev_engines.cpu.len(), 1);
        assert_eq!(dev_engines.cpu[0].name, "x64");
        assert_eq!(dev_engines.os.len(), 3);
    }

    #[test]
    fn resolves_a_corepack_hashed_tool() {
        let temp = tempfile::tempdir().unwrap();
        let hash = "a".repeat(56);
        fs::write(
            temp.path().join("package.json"),
            format!(r#"{{"packageManager":"pnpm@10.12.1+sha224.{hash}"}}"#),
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();

        assert_eq!(
            resolution.tools[0].request.to_string(),
            format!("pnpm@10.12.1+sha224.{hash}")
        );
    }

    #[test]
    fn resolves_multiple_tools_from_jolter_config() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("jolter.json"),
            r#"{"tools":{"pnpm":"10","yarn":"4"}}"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        let requests = resolution
            .tools
            .iter()
            .map(|tool| tool.request.to_string())
            .collect::<Vec<_>>();

        assert_eq!(requests, ["pnpm@10", "yarn@4"]);
    }

    #[test]
    fn lower_priority_nvmrc_does_not_override_or_invalidate_node_version() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".node-version"), "24").unwrap();
        fs::write(temp.path().join(".nvmrc"), "not a valid selector @").unwrap();

        let resolution = resolve(temp.path()).unwrap();

        assert_eq!(resolution.runtime.unwrap().request.to_string(), "node@24");
    }

    #[test]
    fn ignores_unmanaged_package_manager_tools_in_package_json() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"bun@1.3.14"}"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert!(resolution.tools.is_empty());
    }

    #[test]
    fn jolter_config_with_no_tools_ignores_unmanaged_package_manager() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("jolter.json"),
            r#"{"runtime":{"node":"24.x"}}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"bun@1.3.14"}"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(resolution.runtime.unwrap().request.to_string(), "node@24.x");
        assert!(resolution.tools.is_empty());
    }

    #[test]
    fn resolves_dev_engines_semver_range_expression() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{
                "devEngines": {
                    "runtime": { "name": "node", "version": "^20.0.0 || >=22.0.0" },
                    "packageManager": { "name": "pnpm", "version": "^10.0.0" }
                }
            }"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(
            resolution.runtime.unwrap().request.to_string(),
            "node@20.0.0"
        );
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@10.0.0");
    }

    #[test]
    fn tests_dev_engine_selector_normalization_and_package_manager_errors() {
        use parsers::{normalize_dev_engine_selector, parse_package_json};

        assert_eq!(normalize_dev_engine_selector("lts"), "lts");
        assert_eq!(normalize_dev_engine_selector("latest"), "latest");
        assert_eq!(normalize_dev_engine_selector("*"), "*");
        assert_eq!(normalize_dev_engine_selector("^24.0.0"), "24.0.0");
        assert_eq!(normalize_dev_engine_selector(">= 24.0.0"), "24.0.0");
        assert_eq!(normalize_dev_engine_selector("!!!"), "*");

        let temp = tempfile::tempdir().unwrap();
        let pkg = temp.path().join("package.json");
        fs::write(&pkg, r#"{"packageManager": "pnpm_no_at_version"}"#).unwrap();
        assert!(matches!(
            parse_package_json(&pkg),
            Err(ResolverError::InvalidPackageManager(_))
        ));

        let pkg_empty_name = temp.path().join("pkg_empty_name.json");
        fs::write(&pkg_empty_name, r#"{"packageManager": "@1.0.0"}"#).unwrap();
        assert!(matches!(
            parse_package_json(&pkg_empty_name),
            Err(ResolverError::InvalidPackageManager(_))
        ));

        let pkg_empty_sel = temp.path().join("pkg_empty_sel.json");
        fs::write(&pkg_empty_sel, r#"{"packageManager": "pnpm@"}"#).unwrap();
        assert!(matches!(
            parse_package_json(&pkg_empty_sel),
            Err(ResolverError::InvalidPackageManager(_))
        ));

        let missing = temp.path().join("missing_file");

        assert!(matches!(
            parsers::parse_node_file(&missing, RequirementSource::Nvmrc),
            Err(ResolverError::Read { .. })
        ));
        assert!(matches!(
            parse_package_json(&missing),
            Err(ResolverError::Read { .. })
        ));

        let item_ignore = dev_engines::DevEngineItem {
            name: "node".to_owned(),
            version: None,
            on_fail: Some("ignore".to_owned()),
        };
        assert_eq!(
            item_ignore.on_fail_mode(),
            dev_engines::DevEngineOnFail::Ignore
        );

        let item_download = dev_engines::DevEngineItem {
            name: "node".to_owned(),
            version: None,
            on_fail: Some("download".to_owned()),
        };
        assert_eq!(
            item_download.on_fail_mode(),
            dev_engines::DevEngineOnFail::Download
        );
    }

    #[test]
    fn tests_resolver_error_display() {
        let err = ResolverError::Read {
            path: PathBuf::from("/read"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert!(err.to_string().contains("failed to read /read"));

        let err = ResolverError::PackageJson {
            path: PathBuf::from("/pj"),
            source: serde_json::from_str::<serde_json::Value>("bad json").unwrap_err(),
        };
        assert!(err.to_string().contains("invalid package.json at"));

        assert_eq!(parsers::normalize_dev_engine_selector("  LTS "), "lts");
        assert_eq!(parsers::normalize_dev_engine_selector("  * "), "*");
        assert_eq!(parsers::normalize_dev_engine_selector(""), "*");
        assert_eq!(
            parsers::normalize_dev_engine_selector("  LATEST "),
            "latest"
        );

        let temp = tempfile::tempdir().unwrap();
        let pj_no_at = temp.path().join("package.json");
        std::fs::write(&pj_no_at, r#"{"packageManager":"pnpm"}"#).unwrap();
        assert!(matches!(
            parsers::parse_package_json(&pj_no_at),
            Err(ResolverError::InvalidPackageManager(_))
        ));

        let pj_empty_name = temp.path().join("pj_empty.json");
        std::fs::write(&pj_empty_name, r#"{"packageManager":"@1.0.0"}"#).unwrap();
        assert!(matches!(
            parsers::parse_package_json(&pj_empty_name),
            Err(ResolverError::InvalidPackageManager(_))
        ));
    }
}
