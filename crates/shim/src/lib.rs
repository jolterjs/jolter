pub mod error;
pub mod executor;
pub mod generator;
pub mod resolver;
pub mod target;

pub const SHIM_COMMANDS: [&str; 7] = ["node", "npm", "npx", "pnpm", "yarn", "bun", "deno"];

pub use error::ShimError;
pub use executor::{invoked_command_name, run_command, run_invoked_command};
pub use generator::{desired_shim_commands, install_shims};
pub use resolver::resolve_command;
pub use target::{ResolvedCommand, ShimTarget, target_for_command};

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use jolter_runtime::{RuntimeKind, ToolKind};
    use jolter_storage::Storage;
    use semver::Version;

    use super::*;

    #[test]
    fn maps_commands_to_targets() {
        assert_eq!(
            target_for_command("node"),
            Some(ShimTarget::Runtime(RuntimeKind::Node))
        );
        assert_eq!(
            target_for_command("pnpm"),
            Some(ShimTarget::NodeTool("pnpm"))
        );
        assert_eq!(target_for_command("invalid"), None);
    }

    #[test]
    fn resolves_runtime_binary_path() {
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
        storage.activate(RuntimeKind::Node, &version).unwrap();

        let resolved = resolve_command("node", temp.path(), &storage).unwrap();
        assert_eq!(resolved.executable, exe);
        assert!(resolved.arguments.is_empty());
    }

    #[test]
    fn resolves_bundled_node_tool_path() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let version = Version::new(24, 0, 0);

        let node_dir = storage.runtime_version_dir(RuntimeKind::Node, &version);
        fs::create_dir_all(&node_dir).unwrap();

        #[cfg(windows)]
        let node_exe = node_dir.join("node.exe");
        #[cfg(not(windows))]
        let node_exe = node_dir.join("bin").join("node");

        fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
        fs::write(&node_exe, "").unwrap();

        let npm_exe = storage.node_tool_executable(&version, "npm");
        fs::create_dir_all(npm_exe.parent().unwrap()).unwrap();
        fs::write(&npm_exe, "").unwrap();

        storage.activate(RuntimeKind::Node, &version).unwrap();

        let resolved = resolve_command("npm", temp.path(), &storage).unwrap();
        assert_eq!(resolved.executable, npm_exe);
        assert!(resolved.arguments.is_empty());
    }

    #[test]
    fn resolves_standalone_managed_tool_path() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let version = Version::new(24, 0, 0);
        let pnpm_version = Version::new(10, 2, 0);

        let node_dir = storage.runtime_version_dir(RuntimeKind::Node, &version);
        fs::create_dir_all(&node_dir).unwrap();

        #[cfg(windows)]
        let node_exe = node_dir.join("node.exe");
        #[cfg(not(windows))]
        let node_exe = node_dir.join("bin").join("node");

        fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
        fs::write(&node_exe, "").unwrap();
        storage.activate(RuntimeKind::Node, &version).unwrap();

        let pnpm_dir = storage.tool_version_dir(ToolKind::Pnpm, &pnpm_version);
        let entrypoint = pnpm_dir.join("bin").join("pnpm.cjs");
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(&entrypoint, "").unwrap();
        storage
            .activate_tool(ToolKind::Pnpm, &pnpm_version)
            .unwrap();

        let resolved = resolve_command("pnpm", temp.path(), &storage).unwrap();
        assert_eq!(resolved.executable, node_exe);
        assert_eq!(resolved.arguments, vec![entrypoint]);
    }

    #[test]
    fn installs_shims() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());

        let source = temp.path().join("fake-jolter");
        fs::write(&source, "binary content").unwrap();

        let node_dir = storage.runtime_version_dir(RuntimeKind::Node, &Version::new(24, 0, 0));
        fs::create_dir_all(&node_dir).unwrap();

        #[cfg(windows)]
        let node_exe = node_dir.join("node.exe");
        #[cfg(not(windows))]
        let node_exe = node_dir.join("bin").join("node");

        fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
        fs::write(node_exe, "").unwrap();

        let installed = install_shims(&source, &storage).unwrap();

        #[cfg(windows)]
        assert!(installed.contains(&storage.shims_dir().join("node.exe")));
        #[cfg(not(windows))]
        assert!(installed.contains(&storage.shims_dir().join("node")));
    }

    #[test]
    fn resolves_installed_plugin_tool_command() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let version = Version::new(21, 0, 2);

        let plugin_dir = storage.plugin_version_dir("@jolter/jdk", &version);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join(".jolter-plugin.json"),
            r#"{
                "name": "@jolter/jdk",
                "version": "21.0.2",
                "provides": {
                    "tools": {
                        "java": {
                            "commands": ["java"]
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let tool_dir = storage.plugin_tool_version_dir("@jolter/jdk", "java", &version);
        fs::create_dir_all(&tool_dir).unwrap();
        fs::write(
            tool_dir.join(".jolter-plugin-tool.json"),
            r#"{"commands":["java"]}"#,
        )
        .unwrap();
        fs::write(tool_dir.join("java"), "binary").unwrap();

        storage
            .activate_plugin_tool("@jolter/jdk", "java", &version)
            .unwrap();

        let resolved = resolve_command("java", temp.path(), &storage).unwrap();
        assert_eq!(resolved.executable, tool_dir.join("java"));
        assert!(resolved.arguments.is_empty());
    }

    #[test]
    fn tests_shim_error_display() {
        let err = ShimError::UnsupportedCommand("foo".to_owned());
        assert!(err.to_string().contains("unsupported shim command `foo`"));

        let err = ShimError::PluginToolNotInstalled("java".to_owned());
        assert!(err.to_string().contains("plugin command `java` is known"));

        let err = ShimError::NoActiveRuntime(RuntimeKind::Node);
        assert!(err.to_string().contains("no active node runtime"));

        let err = ShimError::ActiveRuntimeMissing {
            kind: RuntimeKind::Node,
            version: Version::new(24, 0, 0),
            path: PathBuf::from("/path"),
        };
        assert!(
            err.to_string()
                .contains("active node@24.0.0 runtime is missing")
        );

        let err = ShimError::ActiveToolMissing {
            kind: ToolKind::Pnpm,
            version: Version::new(10, 0, 0),
            path: PathBuf::from("/path"),
        };
        assert!(
            err.to_string()
                .contains("active pnpm@10.0.0 tool is missing")
        );

        let err = ShimError::ExecutableNotFound {
            command: "node".to_owned(),
            path: PathBuf::from("/missing"),
        };
        assert!(
            err.to_string()
                .contains("command `node` was not found at /missing")
        );

        let err = ShimError::SourceExecutableMissing(PathBuf::from("/src"));
        assert!(
            err.to_string()
                .contains("Jolter executable was not found at /src")
        );

        let err = ShimError::RuntimeNotInstalled(
            jolter_runtime::RuntimeRequest::new(RuntimeKind::Node, "24").unwrap(),
        );
        assert!(
            err.to_string()
                .contains("runtime required by the project is not installed: node@24")
        );

        let err = ShimError::ToolNotInstalled(
            jolter_runtime::ToolRequest::new(ToolKind::Pnpm, "10").unwrap(),
        );
        assert!(
            err.to_string()
                .contains("tool required by the project is not installed: pnpm@10")
        );
    }

    #[test]
    fn tests_executor_utility_functions() {
        let name = invoked_command_name();
        assert!(name.is_some());

        let mut cmd = std::process::Command::new("ls");
        let temp = tempfile::tempdir().unwrap();
        executor::prepend_runtime_path(&mut cmd, RuntimeKind::Node, temp.path()).unwrap();

        assert!(run_command("unsupported_xyz").is_err());
    }

    #[test]
    fn tests_desired_shim_commands_and_generator() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        let v1 = Version::new(24, 0, 0);
        let _node_dir = storage.runtime_version_dir(RuntimeKind::Node, &v1);
        let exe = storage.runtime_executable(RuntimeKind::Node, &v1);
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"node").unwrap();

        let pnpm_dir = storage.tool_version_dir(ToolKind::Pnpm, &Version::new(10, 0, 0));
        fs::create_dir_all(pnpm_dir.join("bin")).unwrap();
        fs::write(pnpm_dir.join("bin").join("pnpm.cjs"), b"pnpm").unwrap();

        let desired = desired_shim_commands(&storage).unwrap();
        assert!(desired.contains("node"));
        assert!(desired.contains("npm"));
        assert!(desired.contains("npx"));
        assert!(desired.contains("pnpm"));

        let missing = temp.path().join("nonexistent_source");
        assert!(matches!(
            install_shims(&missing, &storage),
            Err(ShimError::SourceExecutableMissing(_))
        ));
    }

    #[test]
    fn tests_plugin_command_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        let v1 = Version::new(1, 0, 0);
        let plugin_dir = storage.plugin_version_dir("@scope/eslint", &v1);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join(".jolter-plugin.json"),
            r#"{"commands":["eslint-custom"],"provides":{"tools":{"eslint":{"commands":["eslint-command"]}}}}"#,
        )
        .unwrap();

        let pt_dir = storage.plugin_tool_version_dir("@scope/eslint", "eslint", &v1);
        fs::create_dir_all(&pt_dir).unwrap();
        fs::write(
            pt_dir.join(".jolter-plugin-tool.json"),
            r#"{"commands":["eslint-command"]}"#,
        )
        .unwrap();
        fs::write(pt_dir.join("eslint-command"), b"binary").unwrap();

        storage
            .activate_plugin_tool("@scope/eslint", "eslint", &v1)
            .unwrap();

        let v24 = Version::new(24, 0, 0);
        let node_exe = storage.runtime_executable(RuntimeKind::Node, &v24);
        fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
        fs::write(&node_exe, b"node").unwrap();
        storage.activate(RuntimeKind::Node, &v24).unwrap();

        let resolved = resolve_command("eslint-command", temp.path(), &storage).unwrap();
        assert_eq!(resolved.executable, pt_dir.join("eslint-command"));
    }

    #[test]
    fn tests_prepend_runtime_path_helper() {
        use std::process::Command;
        let mut cmd = Command::new("echo");
        let path = std::path::Path::new("/tmp/test_node");

        assert!(executor::prepend_runtime_path(&mut cmd, RuntimeKind::Node, path).is_ok());
        assert!(executor::prepend_runtime_path(&mut cmd, RuntimeKind::Bun, path).is_ok());
    }

    #[test]
    fn tests_shim_resolver_error_paths() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());

        assert!(matches!(
            resolve_command("node", temp.path(), &storage),
            Err(ShimError::NoActiveRuntime(RuntimeKind::Node))
        ));

        assert!(matches!(
            resolve_command("nonexistent_command_xyz", temp.path(), &storage),
            Err(ShimError::UnsupportedCommand(_))
        ));

        let v24 = semver::Version::new(24, 0, 0);
        storage.activate(RuntimeKind::Node, &v24).unwrap();
        assert!(matches!(
            resolve_command("node", temp.path(), &storage),
            Err(ShimError::ActiveRuntimeMissing { .. })
        ));

        let v10 = semver::Version::new(10, 0, 0);
        let node_exe = storage.runtime_executable(RuntimeKind::Node, &v24);
        std::fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
        std::fs::write(&node_exe, b"node").unwrap();
        storage
            .activate_tool(jolter_runtime::ToolKind::Pnpm, &v10)
            .unwrap();
        assert!(matches!(
            resolve_command("pnpm", temp.path(), &storage),
            Err(ShimError::ActiveToolMissing { .. })
        ));

        let missing_exe = temp.path().join("missing_exe");
        assert!(matches!(
            install_shims(&missing_exe, &storage),
            Err(ShimError::SourceExecutableMissing(_))
        ));

        let fake_exe = temp.path().join("fake_jolter");
        std::fs::write(&fake_exe, b"exe").unwrap();
        let node_exe = storage.runtime_executable(RuntimeKind::Node, &v24);
        std::fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
        std::fs::write(&node_exe, b"node").unwrap();
        storage.activate(RuntimeKind::Node, &v24).unwrap();

        let obsolete_shim = storage.shims_dir().join("obsolete_cmd");
        std::fs::create_dir_all(storage.shims_dir()).unwrap();
        std::fs::write(&obsolete_shim, b"old").unwrap();
        assert!(obsolete_shim.is_file());

        let installed = install_shims(&fake_exe, &storage).unwrap();
        assert!(!installed.is_empty());
        assert!(!obsolete_shim.exists());

        let plugin_ver_dir = storage.plugins_dir().join("@scope/test/1.0.0");
        std::fs::create_dir_all(&plugin_ver_dir).unwrap();
        let invalid_manifest = plugin_ver_dir.join(".jolter-plugin.json");
        std::fs::write(&invalid_manifest, b"invalid_json!").unwrap();
        assert!(resolve_command("some_plugin_cmd", temp.path(), &storage).is_err());

        let proj_dir = temp.path().join("project_with_missing_ver");
        std::fs::create_dir_all(&proj_dir).unwrap();
        std::fs::write(
            proj_dir.join("jolter.json"),
            r#"{"runtime":{"node":"99.0.0"},"tools":{"pnpm":"99.0.0"}}"#,
        )
        .unwrap();

        assert!(matches!(
            resolve_command("node", &proj_dir, &storage),
            Err(ShimError::RuntimeNotInstalled(_))
        ));

        let v99 = semver::Version::new(99, 0, 0);
        let node99_exe = storage.runtime_executable(RuntimeKind::Node, &v99);
        std::fs::create_dir_all(node99_exe.parent().unwrap()).unwrap();
        std::fs::write(&node99_exe, b"node").unwrap();

        assert!(matches!(
            resolve_command("pnpm", &proj_dir, &storage),
            Err(ShimError::ToolNotInstalled(_))
        ));
    }

    #[test]
    fn tests_shim_resolver_fallback_paths() {
        let temp_fallback = tempfile::tempdir().unwrap();
        let storage_fb = Storage::new(temp_fallback.path());
        storage_fb.ensure_layout().unwrap();

        let v1 = Version::new(1, 0, 0);
        let v2 = Version::new(2, 0, 0);

        let node1 = storage_fb.runtime_executable(RuntimeKind::Node, &v1);
        let node2 = storage_fb.runtime_executable(RuntimeKind::Node, &v2);
        fs::create_dir_all(node1.parent().unwrap()).unwrap();
        fs::create_dir_all(node2.parent().unwrap()).unwrap();
        fs::write(&node1, b"node1").unwrap();
        fs::write(&node2, b"node2").unwrap();

        let fb_runtime = resolver::active_runtime(&storage_fb, RuntimeKind::Node).unwrap();
        assert_eq!(fb_runtime.version, v2);

        let pnpm_dir2 = storage_fb.tool_version_dir(ToolKind::Pnpm, &v2);
        fs::create_dir_all(pnpm_dir2.join("bin")).unwrap();
        fs::write(pnpm_dir2.join("bin").join("pnpm.cjs"), b"pnpm2").unwrap();

        let resolved_pnpm = resolve_command("pnpm", temp_fallback.path(), &storage_fb).unwrap();
        assert_eq!(resolved_pnpm.executable, node2);

        let bun_exe = storage_fb.runtime_executable(RuntimeKind::Bun, &v1);
        fs::create_dir_all(bun_exe.parent().unwrap()).unwrap();
        fs::write(&bun_exe, b"bun").unwrap();

        let deno_exe = storage_fb.runtime_executable(RuntimeKind::Deno, &v1);
        fs::create_dir_all(deno_exe.parent().unwrap()).unwrap();
        fs::write(&deno_exe, b"deno").unwrap();

        let yarn_dir = storage_fb.tool_version_dir(ToolKind::Yarn, &v1);
        fs::create_dir_all(yarn_dir.join("bin")).unwrap();
        fs::write(yarn_dir.join("bin").join("yarn.js"), b"yarn").unwrap();

        let desired_all = desired_shim_commands(&storage_fb).unwrap();
        assert!(desired_all.contains("bun"));
        assert!(desired_all.contains("deno"));
        assert!(desired_all.contains("yarn"));

        assert!(matches!(
            resolve_command("unsupported_tool", temp_fallback.path(), &storage_fb),
            Err(ShimError::UnsupportedCommand(_))
        ));

        let npx_bin = node2.parent().unwrap().join("npx");
        fs::write(&npx_bin, b"npx").unwrap();
        let node_tool_fallback = resolve_command("npx", temp_fallback.path(), &storage_fb).unwrap();
        assert_eq!(node_tool_fallback.executable, npx_bin);

        assert!(matches!(
            generator::install_shims(&std::path::PathBuf::from("/nonexistent/exe"), &storage_fb),
            Err(ShimError::SourceExecutableMissing(_))
        ));

        let old_shim = storage_fb.shims_dir().join("old_obsolete_tool");
        fs::write(&old_shim, b"old").unwrap();
        let hidden_temp = storage_fb.shims_dir().join(".tmp_hidden");
        fs::write(&hidden_temp, b"hidden").unwrap();

        let _ = generator::install_shims(&node2, &storage_fb).unwrap();
        assert!(!old_shim.exists());
        assert!(hidden_temp.exists());

        assert!(executor::invoked_command_name().is_some());
    }
}
