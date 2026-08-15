use super::*;

#[test]
fn discovers_custom_home() {
    if let Ok(path) = env::var("JOLTER_HOME") {
        let storage = Storage::discover().unwrap();
        assert_eq!(storage.root(), Path::new(&path));
    } else if let Some(home) = home_directory() {
        let storage = Storage::discover().unwrap();
        assert_eq!(storage.root(), home.join(".jolter"));
    }
}

#[test]
fn builds_expected_paths() {
    let storage = Storage::new("/tmp/jolter");
    let version = Version::new(24, 0, 0);

    assert_eq!(
        storage.runtime_version_dir(RuntimeKind::Node, &version),
        PathBuf::from("/tmp/jolter/runtimes/node/24.0.0")
    );
    assert_eq!(
        storage.tool_version_dir(ToolKind::Pnpm, &version),
        PathBuf::from("/tmp/jolter/tools/pnpm/24.0.0")
    );
    assert_eq!(
        storage.plugin_version_dir("@jolter/jdk", &version),
        PathBuf::from("/tmp/jolter/plugins/jolter/jdk/24.0.0")
    );
    assert_eq!(
        storage.plugin_tool_version_dir("@jolter/jdk", "java", &version),
        PathBuf::from("/tmp/jolter/plugin-tools/jolter/jdk/java/24.0.0")
    );
}

#[test]
fn ensures_storage_layout_exists() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());

    storage.ensure_layout().unwrap();

    assert!(storage.runtimes_dir().exists());
    assert!(storage.tools_dir().exists());
    assert!(storage.plugins_dir().exists());
    assert!(storage.plugin_tools_dir().exists());
    assert!(storage.shims_dir().exists());
    assert!(storage.cache_dir().exists());
    assert!(storage.config_dir().exists());
}

#[test]
fn manages_active_versions_and_tools() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let v24 = Version::new(24, 0, 0);
    let v22 = Version::new(22, 0, 0);

    storage.activate(RuntimeKind::Node, &v24).unwrap();
    assert_eq!(
        storage.active_version(RuntimeKind::Node).unwrap(),
        Some(v24.clone())
    );

    assert!(!storage.deactivate(RuntimeKind::Node, Some(&v22)).unwrap());
    assert!(storage.deactivate(RuntimeKind::Node, Some(&v24)).unwrap());
    assert_eq!(storage.active_version(RuntimeKind::Node).unwrap(), None);

    storage.activate_tool(ToolKind::Pnpm, &v24).unwrap();
    assert_eq!(
        storage.active_tool_version(ToolKind::Pnpm).unwrap(),
        Some(v24.clone())
    );

    assert!(!storage.deactivate_tool(ToolKind::Pnpm, Some(&v22)).unwrap());
    assert!(storage.deactivate_tool(ToolKind::Pnpm, Some(&v24)).unwrap());
    assert_eq!(storage.active_tool_version(ToolKind::Pnpm).unwrap(), None);
}

#[test]
fn manages_active_plugin_tools() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let version = Version::new(21, 0, 2);
    let dir = storage.plugin_tool_version_dir("@jolter/jdk", "java", &version);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(".jolter-plugin-tool.json"),
        r#"{"commands":["java","javac"]}"#,
    )
    .unwrap();

    storage
        .activate_plugin_tool("@jolter/jdk", "java", &version)
        .unwrap();

    let active = storage.active_plugin_tool("java").unwrap().unwrap();
    assert_eq!(active.provider, "@jolter/jdk");
    assert_eq!(active.tool, "java");
    assert_eq!(active.version, version);
    assert_eq!(active.commands, vec!["java", "javac"]);

    let all = storage.active_plugin_tools().unwrap();
    assert_eq!(all.len(), 1);

    assert!(
        storage
            .deactivate_plugin_tool("java", Some(&version))
            .unwrap()
    );
    assert!(storage.active_plugin_tool("java").unwrap().is_none());
}

#[test]
fn calculates_cache_stats() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let downloads = storage.cache_dir().join("downloads");
    let metadata = storage.cache_dir().join("metadata");
    fs::create_dir_all(&downloads).unwrap();
    fs::create_dir_all(&metadata).unwrap();

    fs::write(downloads.join("a.tar.gz"), "hello").unwrap();
    fs::write(metadata.join("b.json"), "world!").unwrap();

    let stats = storage.cache_stats().unwrap();
    assert_eq!(stats.files, 2);
    assert_eq!(stats.bytes, 11);
}

#[test]
fn finds_installed_runtimes_and_tools() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let version = Version::new(24, 0, 0);

    let node_dir = storage.runtime_version_dir(RuntimeKind::Node, &version);
    fs::create_dir_all(&node_dir).unwrap();

    #[cfg(windows)]
    let exe = node_dir.join("node.exe");
    #[cfg(not(windows))]
    let exe = node_dir.join("bin").join("node");

    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, "").unwrap();

    let found = storage.installed_runtimes().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].version, version);

    let request = jolter_runtime::RuntimeRequest::new(RuntimeKind::Node, "24").unwrap();
    let matched = storage.find_matching(&request).unwrap().unwrap();
    assert_eq!(matched.version, version);

    let tool_dir = storage.tool_version_dir(ToolKind::Pnpm, &version);
    fs::create_dir_all(tool_dir.join("bin")).unwrap();
    fs::write(tool_dir.join("bin").join("pnpm.cjs"), "").unwrap();

    let found_tools = storage.installed_tools().unwrap();
    assert_eq!(found_tools.len(), 1);
    assert_eq!(found_tools[0].version, version);

    let request = ToolRequest::new(ToolKind::Pnpm, "24").unwrap();
    let matched_tool = storage.find_matching_tool(&request).unwrap().unwrap();
    assert_eq!(matched_tool.version, version);
}

#[test]
fn tests_storage_error_display() {
    let err = StorageError::HomeDirectoryUnavailable;
    assert!(err.to_string().contains("could not determine"));

    let err = StorageError::InvalidPath {
        path: PathBuf::from("/invalid"),
    };
    assert!(err.to_string().contains("/invalid"));

    let err = StorageError::InvalidActiveVersion {
        kind: RuntimeKind::Node,
        value: "bad".to_owned(),
        source: Version::parse("bad").unwrap_err(),
    };
    assert!(
        err.to_string()
            .contains("active node version `bad` is invalid")
    );

    let err = StorageError::InvalidActiveToolVersion {
        kind: ToolKind::Pnpm,
        value: "bad".to_owned(),
        source: Version::parse("bad").unwrap_err(),
    };
    assert!(
        err.to_string()
            .contains("active pnpm tool version `bad` is invalid")
    );

    let err = StorageError::InvalidActivePluginToolVersion {
        tool: "java".to_owned(),
        value: "bad".to_owned(),
        source: Version::parse("bad").unwrap_err(),
    };
    assert!(
        err.to_string()
            .contains("active plugin tool java version `bad` is invalid")
    );

    let err = StorageError::Read {
        path: PathBuf::from("/read/error"),
        source: std::io::Error::other("read error"),
    };
    assert!(err.to_string().contains("/read/error"));

    let err = StorageError::ReadFile {
        path: PathBuf::from("/read_file/error"),
        source: std::io::Error::other("read file error"),
    };
    assert!(err.to_string().contains("/read_file/error"));

    let err = StorageError::Create {
        path: PathBuf::from("/create/error"),
        source: std::io::Error::other("create error"),
    };
    assert!(err.to_string().contains("/create/error"));

    let err = StorageError::WriteFile {
        path: PathBuf::from("/write/error"),
        source: std::io::Error::other("write error"),
    };
    assert!(err.to_string().contains("/write/error"));

    let err = StorageError::ParseActive {
        path: PathBuf::from("/parse/error"),
        source: serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err(),
    };
    assert!(err.to_string().contains("/parse/error"));
}

#[test]
fn tests_paths_utility_functions() {
    assert_eq!(
        plugin_path_parts("@jolter/jdk"),
        ("jolter".to_owned(), "jdk".to_owned())
    );
    assert_eq!(
        plugin_path_parts("simple-plugin"),
        ("unknown".to_owned(), "simple-plugin".to_owned())
    );

    let v = Version::new(1, 2, 3);
    assert!(selector_matches_version("latest", &v));
    assert!(selector_matches_version("*", &v));
    assert!(selector_matches_version("X", &v));
    assert!(selector_matches_version("1", &v));
    assert!(selector_matches_version("1.x", &v));
    assert!(selector_matches_version("1.2", &v));
    assert!(selector_matches_version("1.2.3", &v));
    assert!(!selector_matches_version("2", &v));
    assert!(!selector_matches_version("1.3", &v));
    assert!(!selector_matches_version("1.2.4", &v));
    assert!(!selector_matches_version("invalid.ver", &v));
    assert!(!selector_matches_version("1.2.3.4", &v));

    let root = Path::new("/test/root");
    let node_exe = runtime_executable_in(root, RuntimeKind::Node);
    let bun_exe = runtime_executable_in(root, RuntimeKind::Bun);
    let deno_exe = runtime_executable_in(root, RuntimeKind::Deno);
    assert!(node_exe.to_str().unwrap().contains("node"));
    assert!(bun_exe.to_str().unwrap().contains("bun"));
    assert!(deno_exe.to_str().unwrap().contains("deno"));
}

#[test]
fn tests_clean_directory_stats() {
    let temp = tempfile::tempdir().unwrap();

    let missing_stats = clean::directory_stats(&temp.path().join("nonexistent")).unwrap();
    assert_eq!(missing_stats.files, 0);
    assert_eq!(missing_stats.bytes, 0);

    let file_path = temp.path().join("file.txt");
    fs::write(&file_path, "12345").unwrap();
    let file_stats = clean::directory_stats(&file_path).unwrap();
    assert_eq!(file_stats.files, 1);
    assert_eq!(file_stats.bytes, 5);

    let sub_dir = temp.path().join("subdir");
    fs::create_dir(&sub_dir).unwrap();
    fs::write(sub_dir.join("subfile.txt"), "abc").unwrap();
    let dir_stats = clean::directory_stats(temp.path()).unwrap();
    assert_eq!(dir_stats.files, 2);
    assert_eq!(dir_stats.bytes, 8);
}

#[test]
fn tests_manifest_reading() {
    let temp = tempfile::tempdir().unwrap();
    let tool_root = temp.path().join("java");

    let v1 = tool_root.join("v1.0.0");
    let v2 = tool_root.join("2.0.0");
    let invalid = tool_root.join("not-a-version");
    let regular_file = tool_root.join("just-a-file.txt");

    fs::create_dir_all(&v1).unwrap();
    fs::create_dir_all(&v2).unwrap();
    fs::create_dir_all(&invalid).unwrap();
    fs::write(&regular_file, "text").unwrap();

    fs::write(
        v1.join(".jolter-plugin-tool.json"),
        r#"{"commands":["java","javac"]}"#,
    )
    .unwrap();

    let installed =
        manifest::read_installed_plugin_tool_versions("@jolter/jdk", "java", &tool_root).unwrap();
    assert_eq!(installed.len(), 2);

    let cmds_err = manifest::read_plugin_tool_commands(&v2);
    assert!(cmds_err.is_err());

    let bad_json_path = temp.path().join("bad.json");
    fs::write(&bad_json_path, "{invalid_json}").unwrap();
    let parse_err = manifest::read_plugin_tool_commands(temp.path());
    assert!(parse_err.is_err());
}

#[test]
fn tests_active_atomic_write_and_deactivations() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("nested").join("test.txt");

    active::atomic_write(&file_path, b"test contents").unwrap();
    assert_eq!(fs::read_to_string(&file_path).unwrap(), "test contents");

    assert!(active::atomic_write(Path::new(""), b"fail").is_err());

    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let v1 = Version::new(1, 0, 0);
    storage.activate(RuntimeKind::Node, &v1).unwrap();
    assert!(storage.deactivate(RuntimeKind::Node, None).unwrap());
    assert_eq!(storage.active_version(RuntimeKind::Node).unwrap(), None);

    storage.activate_tool(ToolKind::Pnpm, &v1).unwrap();
    assert!(storage.deactivate_tool(ToolKind::Pnpm, None).unwrap());
    assert_eq!(storage.active_tool_version(ToolKind::Pnpm).unwrap(), None);

    let tool_version_dir = storage.plugin_tool_version_dir("@jolter/jdk", "java", &v1);
    fs::create_dir_all(&tool_version_dir).unwrap();
    fs::write(
        tool_version_dir.join(".jolter-plugin-tool.json"),
        r#"{"commands":["java"]}"#,
    )
    .unwrap();

    storage
        .activate_plugin_tool("@jolter/jdk", "java", &v1)
        .unwrap();
    assert!(storage.deactivate_plugin_tool("java", None).unwrap());
    assert!(storage.active_plugin_tool("java").unwrap().is_none());
}

#[test]
fn tests_storage_removals_and_cache_clean() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let v1 = Version::new(1, 0, 0);
    let node_dir = storage.runtime_version_dir(RuntimeKind::Node, &v1);
    fs::create_dir_all(&node_dir).unwrap();
    assert!(fs::remove_dir_all(&node_dir).is_ok());
    assert!(!node_dir.exists());

    let pnpm_dir = storage.tool_version_dir(ToolKind::Pnpm, &v1);
    fs::create_dir_all(&pnpm_dir).unwrap();
    assert!(fs::remove_dir_all(&pnpm_dir).is_ok());
    assert!(!pnpm_dir.exists());

    let pt_dir = storage.plugin_tool_version_dir("@jolter/jdk", "java", &v1);
    fs::create_dir_all(&pt_dir).unwrap();
    assert!(fs::remove_dir_all(&pt_dir).is_ok());
    assert!(!pt_dir.exists());

    fs::write(storage.cache_dir().join("test.bin"), b"12345").unwrap();
    let stats = storage.path_stats(&storage.cache_dir()).unwrap();
    assert_eq!(stats.files, 1);
    assert_eq!(stats.bytes, 5);

    let found_pt = storage.find_matching_plugin_tool("@jolter/jdk", "java", "1.0.0");
    assert!(found_pt.unwrap().is_none());
}

#[test]
fn tests_installed_plugins_and_plugin_tools() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let v1 = Version::new(1, 0, 0);

    let plugin_ver_dir = storage.plugin_version_dir("@scope/eslint", &v1);
    fs::create_dir_all(&plugin_ver_dir).unwrap();
    fs::write(
        plugin_ver_dir.join(".jolter-plugin.json"),
        r#"{"canonicalName":"@scope/eslint","requestedName":"eslint","version":"1.0.0","registryUrl":"","wasmSha256":"","commands":["eslint"],"provides":{"tools":{}}}"#,
    )
    .unwrap();

    let pt_ver_dir = storage.plugin_tool_version_dir("@scope/eslint", "eslint-cli", &v1);
    fs::create_dir_all(&pt_ver_dir).unwrap();
    fs::write(
        pt_ver_dir.join(".jolter-plugin-tool.json"),
        r#"{"commands":["eslint-cli"]}"#,
    )
    .unwrap();
    fs::write(pt_ver_dir.join("eslint-cli"), b"binary").unwrap();

    let installed_p = storage.installed_plugins().unwrap();
    assert_eq!(installed_p.len(), 1);
    assert_eq!(installed_p[0].canonical_name, "@scope/eslint");

    let match_p = storage
        .find_matching_plugin("@scope/eslint", "1.x")
        .unwrap();
    assert!(match_p.is_some());

    let installed_pt = storage.installed_plugin_tools().unwrap();
    assert_eq!(installed_pt.len(), 1);
    assert_eq!(installed_pt[0].tool, "eslint-cli");

    let match_pt = storage
        .find_matching_plugin_tool("@scope/eslint", "eslint-cli", "1.0.0")
        .unwrap();
    assert!(match_pt.is_some());

    storage
        .activate_plugin_tool("@scope/eslint", "eslint-cli", &v1)
        .unwrap();
    let active_tools = storage.active_plugin_tools().unwrap();
    assert_eq!(active_tools.len(), 1);

    let active_single = storage.active_plugin_tool("eslint-cli").unwrap();
    assert!(active_single.is_some());

    storage.deactivate_plugin_tool("eslint-cli", None).unwrap();
    assert!(storage.active_plugin_tool("eslint-cli").unwrap().is_none());
}

#[test]
fn tests_clean_directory_stats_and_manifest_reading() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("single_file.txt");
    fs::write(&file, b"sample_bytes").unwrap();

    let stats = clean::directory_stats(&file).unwrap();
    assert_eq!(stats.files, 1);
    assert_eq!(stats.bytes, 12);

    let missing_path = temp.path().join("missing_dir");
    assert!(manifest::read_plugin_tool_commands(&missing_path).is_err());

    let invalid_json_dir = temp.path().join("bad_json");
    fs::create_dir_all(&invalid_json_dir).unwrap();
    fs::write(
        invalid_json_dir.join(".jolter-plugin-tool.json"),
        b"invalid_json!",
    )
    .unwrap();
    assert!(manifest::read_plugin_tool_commands(&invalid_json_dir).is_err());

    let storage = Storage::new(temp.path());
    let cfg = storage.config_dir();
    fs::create_dir_all(&cfg).unwrap();
    fs::write(
        cfg.join("active.json"),
        r#"{"node":"invalid_ver","pnpm":"invalid_tool_ver"}"#,
    )
    .unwrap();
    assert!(matches!(
        storage.active_version(jolter_runtime::RuntimeKind::Node),
        Err(StorageError::InvalidActiveVersion { .. })
    ));
    assert!(matches!(
        storage.active_tool_version(jolter_runtime::ToolKind::Pnpm),
        Err(StorageError::InvalidActiveToolVersion { .. })
    ));

    fs::write(
        cfg.join("active-plugin-tools.json"),
        r#"{"tools":{"eslint":{"provider":"@scope/eslint","version":"invalid_pt_ver"}}}"#,
    )
    .unwrap();
    assert!(matches!(
        storage.active_plugin_tool("eslint"),
        Err(StorageError::InvalidActivePluginToolVersion { .. })
    ));

    let _v1 = semver::Version::new(1, 0, 0);

    let v2 = semver::Version::new(2, 0, 0);
    fs::write(cfg.join("active.json"), r#"{"node":"1.0.0"}"#).unwrap();
    assert!(
        !storage
            .deactivate(jolter_runtime::RuntimeKind::Node, Some(&v2))
            .unwrap()
    );

    fs::write(
        cfg.join("active-plugin-tools.json"),
        r#"{"tools":{"eslint":{"provider":"@scope/eslint","version":"1.0.0"}}}"#,
    )
    .unwrap();
    assert!(!storage.deactivate_plugin_tool("eslint", Some(&v2)).unwrap());

    assert!(matches!(
        active::atomic_write(Path::new(""), b"test"),
        Err(StorageError::InvalidPath { .. })
    ));

    let file_as_dir = temp.path().join("file_parent");
    fs::write(&file_as_dir, b"im_a_file").unwrap();
    let child_of_file = file_as_dir.join("child.txt");
    assert!(matches!(
        active::atomic_write(&child_of_file, b"test"),
        Err(StorageError::Create { .. })
    ));

    let storage_fm = Storage::new(temp.path());
    let node_24_dir = storage_fm.runtime_version_dir(
        jolter_runtime::RuntimeKind::Node,
        &semver::Version::new(24, 1, 0),
    );
    fs::create_dir_all(&node_24_dir).unwrap();
    let node_bin = storage_fm.runtime_executable(
        jolter_runtime::RuntimeKind::Node,
        &semver::Version::new(24, 1, 0),
    );
    fs::create_dir_all(node_bin.parent().unwrap()).unwrap();
    fs::write(&node_bin, b"node").unwrap();

    let req_exact =
        jolter_runtime::RuntimeRequest::new(jolter_runtime::RuntimeKind::Node, "24.1.0").unwrap();
    let matched = storage_fm.find_matching(&req_exact).unwrap();
    assert!(matched.is_some());
    assert_eq!(matched.unwrap().version, semver::Version::new(24, 1, 0));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let symlink_file = temp.path().join("symlink_file");
        std::os::unix::fs::symlink(temp.path().join("single_file.txt"), &symlink_file).unwrap();
        let sym_stats = clean::directory_stats(&symlink_file).unwrap();
        assert_eq!(sym_stats.files, 1);

        let unreadable = temp.path().join("unreadable_dir");
        fs::create_dir_all(&unreadable).unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(matches!(
            clean::directory_stats(&unreadable),
            Err(StorageError::Read { .. })
        ));
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn tests_storage_active_parse_errors() {
    let temp = tempfile::tempdir().unwrap();
    let pt_root = temp.path().join("plugins_root");
    fs::create_dir_all(&pt_root).unwrap();
    fs::write(pt_root.join("not_a_dir.txt"), b"file").unwrap();
    fs::create_dir_all(pt_root.join("not_semver")).unwrap();

    let v1_dir = pt_root.join("1.0.0");
    fs::create_dir_all(&v1_dir).unwrap();
    fs::write(
        v1_dir.join(".jolter-plugin-tool.json"),
        r#"{"commands":["cmd1","cmd2"]}"#,
    )
    .unwrap();

    let installed_pts =
        manifest::read_installed_plugin_tool_versions("@scope/provider", "mytool", &pt_root)
            .unwrap();
    assert_eq!(installed_pts.len(), 1);
    assert_eq!(installed_pts[0].version, semver::Version::new(1, 0, 0));
    assert_eq!(
        installed_pts[0].commands,
        vec!["cmd1".to_string(), "cmd2".to_string()]
    );

    assert!(matches!(
        active::atomic_write(Path::new(""), b"data"),
        Err(StorageError::InvalidPath { .. })
    ));

    let storage_err = Storage::new(temp.path());
    storage_err.ensure_layout().unwrap();
    let config_dir = storage_err.config_dir();

    fs::write(config_dir.join("active.json"), b"invalid_json!").unwrap();
    assert!(matches!(
        storage_err.active_version(jolter_runtime::RuntimeKind::Node),
        Err(StorageError::ParseActive { .. })
    ));

    fs::write(config_dir.join("active.json"), r#"{"node":"bad_semver"}"#).unwrap();
    assert!(matches!(
        storage_err.active_version(jolter_runtime::RuntimeKind::Node),
        Err(StorageError::InvalidActiveVersion { .. })
    ));

    fs::write(config_dir.join("active.json"), r#"{"pnpm":"bad_semver"}"#).unwrap();
    assert!(matches!(
        storage_err.active_tool_version(jolter_runtime::ToolKind::Pnpm),
        Err(StorageError::InvalidActiveToolVersion { .. })
    ));

    fs::write(
        config_dir.join("active-plugin-tools.json"),
        b"invalid_json!",
    )
    .unwrap();
    assert!(matches!(
        storage_err.active_plugin_tool("eslint"),
        Err(StorageError::ParseActive { .. })
    ));

    fs::write(
        config_dir.join("active-plugin-tools.json"),
        r#"{"tools":{"eslint":{"provider":"scope","version":"bad_semver"}}}"#,
    )
    .unwrap();
    assert!(matches!(
        storage_err.active_plugin_tool("eslint"),
        Err(StorageError::InvalidActivePluginToolVersion { .. })
    ));
}
