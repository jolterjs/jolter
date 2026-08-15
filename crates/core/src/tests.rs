use super::*;
use jolter_config::{CONFIG_FILE_NAME, ProjectConfig, RuntimeConfig};
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use jolter_storage::Storage;
use semver::Version;
use std::{collections::BTreeMap, fs, path::PathBuf};

#[test]
fn pin_runtime_preserves_tool_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let mut tools = BTreeMap::new();
    tools.insert("pnpm".to_owned(), "10.x".to_owned());
    ProjectConfig {
        schema_url: Some(
            jolter_config::schema_url_for_version(jolter_config::CURRENT_SCHEMA_VERSION).to_owned(),
        ),
        schema_version: jolter_config::CURRENT_SCHEMA_VERSION,
        runtime: RuntimeConfig::default(),
        tools,
        plugins: BTreeMap::new(),
    }
    .write_to(&temp.path().join(CONFIG_FILE_NAME))
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let jolter = Jolter::with_storage(Storage::new(storage_temp.path())).unwrap();

    jolter
        .pin_runtime(temp.path(), &"node@24".parse().unwrap())
        .unwrap();

    let config = ProjectConfig::from_path(&temp.path().join(CONFIG_FILE_NAME)).unwrap();
    assert_eq!(config.runtime.node.as_deref(), Some("24"));
    assert_eq!(config.tools.get("pnpm").map(String::as_str), Some("10.x"));
}

#[test]
fn pin_tool_preserves_runtime_and_other_tools() {
    let temp = tempfile::tempdir().unwrap();
    ProjectConfig {
        schema_url: Some(
            jolter_config::schema_url_for_version(jolter_config::CURRENT_SCHEMA_VERSION).to_owned(),
        ),
        schema_version: jolter_config::CURRENT_SCHEMA_VERSION,
        runtime: RuntimeConfig {
            node: Some("24".to_owned()),
            bun: None,
            deno: None,
        },
        tools: BTreeMap::from([("pnpm".to_owned(), "10".to_owned())]),
        plugins: BTreeMap::new(),
    }
    .write_to(&temp.path().join(CONFIG_FILE_NAME))
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let jolter = Jolter::with_storage(Storage::new(storage_temp.path())).unwrap();

    jolter
        .pin_tool(temp.path(), &"yarn@4".parse().unwrap())
        .unwrap();

    let config = ProjectConfig::from_path(&temp.path().join(CONFIG_FILE_NAME)).unwrap();
    assert_eq!(config.runtime.node.as_deref(), Some("24"));
    assert_eq!(config.tools.get("pnpm").map(String::as_str), Some("10"));
    assert_eq!(config.tools.get("yarn").map(String::as_str), Some("4"));
}

#[test]
fn pin_plugin_tool_records_tool_and_exact_provider() {
    let project = tempfile::tempdir().unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let provider_version = Version::new(1, 2, 3);
    let plugin = storage.plugin_version_dir("@jolter/jolter", &provider_version);
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join(".jolter-plugin.json"),
        r#"{
            "canonicalName":"@jolter/jolter",
            "requestedName":"jolter",
            "version":"1.2.3",
            "registryUrl":"https://registry.jolter.dev",
            "wasmSha256":"0000000000000000000000000000000000000000000000000000000000000000",
            "commands":["jolter"],
            "provides":{"tools":{"jolter":{"commands":["jolter"]}}}
        }"#,
    )
    .unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();

    jolter
        .pin_plugin_tool(project.path(), "jolter", "latest")
        .unwrap();

    let config = ProjectConfig::from_path(&project.path().join(CONFIG_FILE_NAME)).unwrap();
    assert_eq!(config.schema_version, jolter_config::CURRENT_SCHEMA_VERSION);
    assert_eq!(
        config.tools.get("jolter").map(String::as_str),
        Some("latest")
    );
    assert_eq!(
        config.plugins.get("@jolter/jolter").map(String::as_str),
        Some("1.2.3")
    );
}

#[test]
fn sync_uses_an_existing_matching_runtime_without_network() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(CONFIG_FILE_NAME),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    let version = semver::Version::new(24, 1, 0);
    let executable = storage.runtime_executable(RuntimeKind::Node, &version);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"node").unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();

    let outcome = jolter.sync(project.path()).unwrap();

    assert_eq!(outcome.runtime.version, version);
    assert!(!outcome.downloaded);
}

#[test]
fn sync_uses_an_existing_matching_tool_without_network() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(CONFIG_FILE_NAME),
        r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
    )
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    let runtime_version = semver::Version::new(24, 1, 0);
    let executable = storage.runtime_executable(RuntimeKind::Node, &runtime_version);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"node").unwrap();
    let tool_version = semver::Version::new(10, 2, 0);
    let entrypoint = storage
        .tool_entrypoint(jolter_runtime::ToolKind::Pnpm, &tool_version, "pnpm")
        .unwrap();
    fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
    fs::write(entrypoint, b"pnpm").unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let outcome = jolter.sync(project.path()).unwrap();

    assert_eq!(outcome.tools[0].tool.version, tool_version);
    assert_eq!(
        storage.active_tool_version(ToolKind::Pnpm).unwrap(),
        Some(tool_version)
    );
}

#[test]
fn sync_activates_multiple_configured_tools() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(CONFIG_FILE_NAME),
        r#"{"runtime":{"node":"24"},"tools":{"pnpm":"10","yarn":"4"}}"#,
    )
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let node_version = Version::new(24, 1, 0);
    let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(node, b"node").unwrap();
    for (kind, version, command) in [
        (ToolKind::Pnpm, Version::new(10, 2, 0), "pnpm"),
        (ToolKind::Yarn, Version::new(4, 1, 0), "yarn"),
    ] {
        let entrypoint = storage.tool_entrypoint(kind, &version, command).unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"tool").unwrap();
    }
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let outcome = jolter.sync(project.path()).unwrap();

    assert_eq!(outcome.tools.len(), 2);
    assert_eq!(
        storage.active_tool_version(ToolKind::Pnpm).unwrap(),
        Some(Version::new(10, 2, 0))
    );
    assert_eq!(
        storage.active_tool_version(ToolKind::Yarn).unwrap(),
        Some(Version::new(4, 1, 0))
    );
}

#[test]
fn use_reuses_and_activates_a_tool_with_active_node() {
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    let node_version = semver::Version::new(24, 1, 0);
    let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(node, b"node").unwrap();
    storage.activate(RuntimeKind::Node, &node_version).unwrap();
    let tool_version = semver::Version::new(10, 2, 0);
    let entrypoint = storage
        .tool_entrypoint(ToolKind::Pnpm, &tool_version, "pnpm")
        .unwrap();
    fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
    fs::write(entrypoint, b"pnpm").unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let action = jolter.use_tool(&"pnpm@10".parse().unwrap()).unwrap();

    assert_eq!(action.tool.version, tool_version);
    assert!(!action.downloaded);
    assert_eq!(
        storage.active_tool_version(ToolKind::Pnpm).unwrap(),
        Some(tool_version)
    );
}

#[test]
fn use_tool_requires_an_active_node_runtime() {
    let storage_temp = tempfile::tempdir().unwrap();
    let jolter = Jolter::with_storage(Storage::new(storage_temp.path())).unwrap();

    let error = jolter.use_tool(&"pnpm@10".parse().unwrap()).unwrap_err();

    assert!(matches!(error, CoreError::ToolRequiresActiveNode(_)));
}

#[test]
fn sync_rejects_an_existing_tool_incompatible_with_node() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(CONFIG_FILE_NAME),
        r#"{"runtime":{"node":"20"},"packageManager":{"pnpm":"11"}}"#,
    )
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    let runtime_version = semver::Version::new(20, 19, 0);
    let executable = storage.runtime_executable(RuntimeKind::Node, &runtime_version);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"node").unwrap();
    let tool_version = semver::Version::new(11, 6, 0);
    let entrypoint = storage
        .tool_entrypoint(jolter_runtime::ToolKind::Pnpm, &tool_version, "pnpm")
        .unwrap();
    fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
    fs::write(entrypoint, b"pnpm").unwrap();
    fs::write(
        storage
            .tool_version_dir(jolter_runtime::ToolKind::Pnpm, &tool_version)
            .join("package.json"),
        r#"{"engines":{"node":">=22.13"}}"#,
    )
    .unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();

    let error = jolter.sync(project.path()).unwrap_err();

    assert!(matches!(
        error,
        CoreError::Installer(jolter_installer::InstallerError::IncompatibleNodeVersion { .. })
    ));
}

#[test]
fn prune_preserves_active_and_project_versions() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(CONFIG_FILE_NAME),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    for version in [
        semver::Version::new(20, 1, 0),
        semver::Version::new(22, 1, 0),
        semver::Version::new(24, 1, 0),
    ] {
        let executable = storage.runtime_executable(RuntimeKind::Node, &version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(executable, b"node").unwrap();
    }
    storage
        .activate(RuntimeKind::Node, &semver::Version::new(22, 1, 0))
        .unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let preview = jolter.prune(project.path(), 0, true).unwrap();
    assert_eq!(preview.removed.len(), 1);
    assert_eq!(preview.removed[0].version, semver::Version::new(20, 1, 0));
    assert!(preview.reclaimed_bytes() > 0);

    let applied = jolter.prune(project.path(), 0, false).unwrap();
    assert_eq!(applied.removed.len(), 1);
    assert!(
        !storage
            .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(20, 1, 0))
            .exists()
    );
    assert!(
        storage
            .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(22, 1, 0))
            .exists()
    );
    assert!(
        storage
            .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(24, 1, 0))
            .exists()
    );
}

#[test]
fn uninstall_and_cache_lifecycle_are_exposed_by_core() {
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let version = semver::Version::new(2, 1, 0);
    let executable = storage.runtime_executable(RuntimeKind::Deno, &version);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"deno").unwrap();
    let cache = storage.cache_dir().join("downloads").join("archive.zip");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(&cache, b"archive").unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();

    assert!(jolter.cache_stats().unwrap().files > 0);
    assert!(
        jolter
            .uninstall_runtime(RuntimeKind::Deno, &version, false)
            .unwrap()
            .reclaimed_bytes
            > 0
    );
    assert!(!executable.exists());
    assert!(jolter.clean_cache().unwrap().removed_files > 0);
}

#[test]
fn active_tools_are_protected_from_prune_and_uninstall() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(CONFIG_FILE_NAME),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let node_version = semver::Version::new(24, 1, 0);
    let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(node, b"node").unwrap();
    for version in [
        semver::Version::new(9, 1, 0),
        semver::Version::new(10, 2, 0),
    ] {
        let entrypoint = storage
            .tool_entrypoint(ToolKind::Pnpm, &version, "pnpm")
            .unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"pnpm").unwrap();
    }
    let active = semver::Version::new(9, 1, 0);
    storage.activate_tool(ToolKind::Pnpm, &active).unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let preview = jolter.prune(project.path(), 0, true).unwrap();
    assert_eq!(preview.removed.len(), 1);
    assert_eq!(preview.removed[0].version, semver::Version::new(10, 2, 0));
    assert!(matches!(
        jolter.uninstall_tool(ToolKind::Pnpm, &active, false),
        Err(CoreError::ActiveToolRemoval { .. })
    ));
    jolter
        .uninstall_tool(ToolKind::Pnpm, &active, true)
        .unwrap();
    assert_eq!(storage.active_tool_version(ToolKind::Pnpm).unwrap(), None);
}

#[test]
fn tests_pin_plugin_and_list() {
    let project = tempfile::tempdir().unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    jolter
        .pin_plugin(project.path(), "my-plugin", "1.x")
        .unwrap();
    let config = jolter_config::ProjectConfig::from_path(
        &project.path().join(jolter_config::CONFIG_FILE_NAME),
    )
    .unwrap();
    assert_eq!(config.plugins.get("my-plugin").unwrap(), "1.x");
}

#[test]
fn tests_listing_doctor_and_pinning_methods() {
    let project = tempfile::tempdir().unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    assert!(jolter.list().unwrap().is_empty());
    assert!(jolter.list_tools().unwrap().is_empty());
    assert!(jolter.list_plugin_tools().unwrap().is_empty());
    assert!(jolter.list_plugins().unwrap().is_empty());

    let report = jolter.doctor(project.path()).unwrap();
    assert!(report.is_healthy());

    let node_req: RuntimeRequest = "node@20".parse().unwrap();
    jolter.pin_runtime(project.path(), &node_req).unwrap();

    let pnpm_req: ToolRequest = "pnpm@10".parse().unwrap();
    jolter.pin_tool(project.path(), &pnpm_req).unwrap();

    let config_path = project.path().join(jolter_config::CONFIG_FILE_NAME);
    let config = jolter_config::ProjectConfig::from_path(&config_path).unwrap();
    assert_eq!(config.runtime.node, Some("20".to_owned()));
    assert_eq!(config.tools.get("pnpm"), Some(&"10".to_owned()));
}

#[test]
fn tests_uninstall_plugin_and_plugin_tool_error_paths() {
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();

    let err = jolter
        .uninstall_plugin("@scoped/missing", true)
        .unwrap_err();
    assert!(matches!(err, CoreError::PluginNotInstalled(_)));

    let v1 = Version::new(1, 0, 0);
    let err = jolter
        .uninstall_plugin_tool("prov", "tool", &v1, true)
        .unwrap_err();
    assert!(matches!(err, CoreError::Installer(_)));
}

#[test]
fn tests_core_prune_cache_and_sync_methods() {
    let project = tempfile::tempdir().unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();

    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();
    fs::create_dir_all(storage.cache_dir().join("downloads")).unwrap();
    fs::write(
        storage.cache_dir().join("downloads").join("cached.tar.gz"),
        b"cached data",
    )
    .unwrap();

    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let stats = jolter.cache_stats().unwrap();
    assert_eq!(stats.files, 1);

    let clean = jolter.clean_cache().unwrap();
    assert_eq!(clean.removed_files, 1);

    let prune = jolter.prune(project.path(), 0, false).unwrap();
    assert!(prune.removed.is_empty());

    let sync_res = jolter.sync(project.path());
    assert!(sync_res.is_ok());

    let repair_res = jolter.repair(project.path());
    assert!(repair_res.is_ok());
}

#[test]
fn tests_core_prune_and_uninstall_methods() {
    let temp = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let v1 = Version::new(24, 0, 0);
    let v2 = Version::new(24, 1, 0);

    let exe1 = storage.runtime_executable(RuntimeKind::Node, &v1);
    fs::create_dir_all(exe1.parent().unwrap()).unwrap();
    fs::write(&exe1, b"node").unwrap();

    let exe2 = storage.runtime_executable(RuntimeKind::Node, &v2);
    fs::create_dir_all(exe2.parent().unwrap()).unwrap();
    fs::write(&exe2, b"node").unwrap();

    let jolter = Jolter::with_storage(storage.clone()).unwrap();
    let request: jolter_runtime::RuntimeRequest = "node@24.1.0".parse().unwrap();
    jolter.use_runtime(&request).unwrap();

    let prune = jolter.prune(project.path(), 0, false).unwrap();
    assert_eq!(prune.removed.len(), 1);
    assert_eq!(prune.removed[0].version, v1);

    let uninst_err = jolter.uninstall_runtime(RuntimeKind::Node, &v2, false);
    assert!(uninst_err.is_err());

    let uninst_ok = jolter.uninstall_runtime(RuntimeKind::Node, &v2, true);
    assert!(uninst_ok.is_ok());

    let tool_ver = Version::new(10, 0, 0);
    let tool_exe = storage
        .tool_entrypoint(ToolKind::Pnpm, &tool_ver, "pnpm")
        .unwrap();
    fs::create_dir_all(tool_exe.parent().unwrap()).unwrap();
    fs::write(&tool_exe, b"pnpm").unwrap();
    storage.activate_tool(ToolKind::Pnpm, &tool_ver).unwrap();

    assert!(
        jolter
            .uninstall_tool(ToolKind::Pnpm, &tool_ver, false)
            .is_err()
    );
    assert!(
        jolter
            .uninstall_tool(ToolKind::Pnpm, &tool_ver, true)
            .is_ok()
    );

    assert!(jolter.storage().deactivate(RuntimeKind::Node, None).is_ok());
    assert!(
        jolter
            .storage()
            .deactivate_tool(ToolKind::Pnpm, None)
            .is_ok()
    );
}

#[test]
fn tests_core_error_display() {
    let err = CoreError::PluginNotInstalled("my-plugin".to_owned());
    assert_eq!(err.to_string(), "plugin `my-plugin` is not installed");

    let err = CoreError::NoActivePluginTool("my-tool".to_owned());
    assert_eq!(
        err.to_string(),
        "no active plugin tool `my-tool`; pass an explicit selector such as my-tool@latest"
    );

    let err = CoreError::NoRuntimeRequirement(PathBuf::from("/project"));
    assert!(err.to_string().contains("/project"));

    let err = CoreError::ToolRequiresActiveNode("pnpm@10".parse().unwrap());
    assert!(
        err.to_string()
            .contains("requires an active Node.js runtime")
    );

    let err = CoreError::ActiveNodeRuntimeMissing {
        version: Version::new(24, 0, 0),
        path: PathBuf::from("/node"),
    };
    assert!(
        err.to_string()
            .contains("active node@24.0.0 runtime is missing")
    );

    let err = CoreError::ActiveRuntimeRemoval {
        kind: RuntimeKind::Node,
        version: Version::new(24, 0, 0),
    };
    assert!(
        err.to_string()
            .contains("refusing to uninstall active node@24.0.0")
    );

    let err = CoreError::ActiveToolRemoval {
        kind: ToolKind::Pnpm,
        version: Version::new(10, 0, 0),
    };
    assert!(
        err.to_string()
            .contains("refusing to uninstall active pnpm@10.0.0")
    );

    let err = CoreError::ActivePluginToolRemoval {
        provider: "prov".to_owned(),
        tool: "t".to_owned(),
        version: Version::new(1, 0, 0),
    };
    assert!(
        err.to_string()
            .contains("refusing to uninstall active plugin tool t@1.0.0 via prov")
    );

    let err = CoreError::MissingProjectPlugin {
        name: "plug".to_owned(),
        selector: "1.0".to_owned(),
    };
    assert!(err.to_string().contains("project requires plugin plug@1.0"));

    let err = CoreError::PluginToolProviderMissing("t".to_owned());
    assert!(
        err.to_string()
            .contains("no installed plugin provides tool `t`")
    );

    let err = CoreError::AmbiguousPluginToolProvider {
        tool: "t".to_owned(),
        providers: "p1, p2".to_owned(),
    };
    assert!(
        err.to_string()
            .contains("multiple installed plugins provide tool `t`")
    );

    let err = CoreError::UnsupportedPluginToolArchive("rar".to_owned());
    assert!(
        err.to_string()
            .contains("plugin tool archive format `rar` is not supported")
    );

    let err = CoreError::DirectPluginToolUseUnsupported("t".to_owned());
    assert!(
        err.to_string()
            .contains("direct use of plugin-provided tool `t` is not available yet")
    );

    let err = CoreError::ActivePluginRemoval("plug".to_owned());
    assert!(
        err.to_string()
            .contains("plugin `plug` is installed with active shim commands")
    );
}

#[test]
fn tests_core_error_display_extended() {
    let err = CoreError::PluginNotInstalled("my-plugin".to_owned());
    assert!(err.to_string().contains("my-plugin"));

    let err = CoreError::NoActivePluginTool("my-tool".to_owned());
    assert!(err.to_string().contains("my-tool"));

    let req: ToolRequest = "pnpm@10".parse().unwrap();
    let err = CoreError::ToolRequiresNode(req);
    assert!(err.to_string().contains("requires a Node.js runtime"));
}

#[test]
fn install_runtime_and_tool_does_not_activate() {
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let version = semver::Version::new(24, 1, 0);
    let executable = storage.runtime_executable(RuntimeKind::Node, &version);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"node").unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let action = jolter
        .install_runtime(&"node@24.1.0".parse().unwrap())
        .unwrap();
    assert_eq!(action.runtime.version, version);
    assert_eq!(storage.active_version(RuntimeKind::Node).unwrap(), None);
}

#[test]
fn tests_pin_plugin_and_list_extra() {
    let project = tempfile::tempdir().unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    jolter.pin_plugin(project.path(), "eslint", "1.x").unwrap();
    let config = ProjectConfig::from_path(&project.path().join(CONFIG_FILE_NAME)).unwrap();
    assert_eq!(
        config.plugins.get("eslint").map(String::as_str),
        Some("1.x")
    );

    let plugins = jolter.list_plugins().unwrap();
    assert!(plugins.is_empty());

    let tools = jolter.list_plugin_tools().unwrap();
    assert!(tools.is_empty());
}

#[test]
fn tests_uninstall_plugin_and_plugin_tool_error_paths_extra() {
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();

    assert!(jolter.uninstall_plugin("missing", false).is_err());

    let ver = Version::new(1, 0, 0);
    assert!(
        jolter
            .uninstall_plugin_tool("@scope/name", "tool", &ver, false)
            .is_err()
    );

    assert!(matches!(
        jolter.update_plugin_tool("missing", None),
        Err(CoreError::NoActivePluginTool(_))
    ));
    assert!(matches!(
        jolter.install_plugin_tool("missing", "1.0"),
        Err(CoreError::PluginToolProviderMissing(_))
    ));
    assert!(matches!(
        jolter.use_plugin_tool("missing", "1.0"),
        Err(CoreError::PluginToolProviderMissing(_))
    ));
}

#[test]
fn tests_pin_plugin_tool_and_resolution() {
    let project = tempfile::tempdir().unwrap();
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    let v1 = Version::new(1, 0, 0);
    let plugin_dir = storage.plugin_version_dir("@scope/eslint", &v1);
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join(".jolter-plugin.json"),
        r#"{"canonicalName":"@scope/eslint","requestedName":"eslint","version":"1.0.0","registryUrl":"","wasmSha256":"","commands":["eslint"],"provides":{"tools":{"eslint":{"commands":["eslint"]}}}}"#,
    )
    .unwrap();

    jolter
        .pin_plugin_tool(project.path(), "eslint", "1.0.0")
        .unwrap();

    let config = ProjectConfig::from_path(&project.path().join(CONFIG_FILE_NAME)).unwrap();
    assert_eq!(
        config.tools.get("eslint").map(String::as_str),
        Some("1.0.0")
    );
    assert_eq!(
        config.plugins.get("@scope/eslint").map(String::as_str),
        Some("1.0.0")
    );
}

#[test]
fn tests_core_prune_cache_and_sync_methods_extra() {
    let storage_temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(storage_temp.path());
    storage.ensure_layout().unwrap();
    let _jolter_init = Jolter::with_storage(storage.clone()).unwrap();

    let project = tempfile::tempdir().unwrap();
    let v24_0 = Version::new(24, 0, 0);
    let v24_1 = Version::new(24, 1, 0);

    let _node0_dir = storage.runtime_version_dir(RuntimeKind::Node, &v24_0);
    let node0_exe = storage.runtime_executable(RuntimeKind::Node, &v24_0);
    fs::create_dir_all(node0_exe.parent().unwrap()).unwrap();
    fs::write(&node0_exe, b"node").unwrap();

    let _node1_dir = storage.runtime_version_dir(RuntimeKind::Node, &v24_1);
    let node1_exe = storage.runtime_executable(RuntimeKind::Node, &v24_1);
    fs::create_dir_all(node1_exe.parent().unwrap()).unwrap();
    fs::write(&node1_exe, b"node").unwrap();

    let jolter = Jolter::with_storage(storage.clone()).unwrap();
    assert!(matches!(
        jolter.update_plugin_tool("eslint", None),
        Err(CoreError::NoActivePluginTool(_))
    ));

    assert!(
        jolter
            .uninstall_plugin("@nonexistent/plugin", false)
            .is_err()
    );

    let v1 = Version::new(1, 0, 0);
    storage
        .activate_plugin_tool("@scope/eslint", "eslint-cli", &v1)
        .unwrap();
    assert!(matches!(
        jolter.uninstall_plugin_tool("@scope/eslint", "eslint-cli", &v1, false),
        Err(CoreError::ActivePluginToolRemoval { .. })
    ));

    storage.activate(RuntimeKind::Node, &v24_1).unwrap();

    let pruned = jolter.prune(project.path(), 1, false).unwrap();
    assert_eq!(pruned.removed.len(), 1);
    assert_eq!(pruned.removed[0].version, v24_0);

    let stats = jolter.cache_stats().unwrap();
    assert_eq!(stats.files, 0);

    let shims = jolter.install_shims(&node1_exe).unwrap();
    assert!(!shims.is_empty());

    let cleaned = jolter.clean_cache().unwrap();
    assert_eq!(cleaned.reclaimed_bytes, 0);

    let prune_pt = PruneItemKind::PluginTool {
        provider: "@scope/eslint".to_owned(),
        tool: "eslint-cli".to_owned(),
    };
    assert_eq!(prune_pt.to_string(), "eslint-cli via @scope/eslint");

    let prune_rt = PruneItemKind::Runtime(RuntimeKind::Node);
    assert_eq!(prune_rt.to_string(), "node");

    let prune_tl = PruneItemKind::Tool(ToolKind::Pnpm);
    assert_eq!(prune_tl.to_string(), "pnpm");
}
