use std::{fs, process::Command};

fn jolter_command(project: &std::path::Path, home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jolter"));
    command.current_dir(project).env("JOLTER_HOME", home);
    command
}

fn runtime_executable(home: &std::path::Path, kind: &str, version: &str) -> std::path::PathBuf {
    let root = home.join("runtimes").join(kind).join(version);
    #[cfg(windows)]
    {
        root.join(format!("{kind}.exe"))
    }
    #[cfg(not(windows))]
    {
        if kind == "node" {
            root.join("bin").join("node")
        } else {
            root.join(kind)
        }
    }
}

#[test]
fn pin_writes_project_configuration() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["pin", "node@24"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = fs::read_to_string(project.path().join("jolter.json")).unwrap();
    assert!(config.contains(r#""node": "24""#));
}

#[test]
fn global_output_flags_control_operational_logging() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let plain = jolter_command(project.path(), home.path())
        .args(["pin", "node@24", "--no-progress", "--no-color"])
        .output()
        .unwrap();
    assert!(plain.status.success());
    let stderr = String::from_utf8_lossy(&plain.stderr);
    assert!(stderr.contains("[jolter] config"));
    assert!(!stderr.contains('\r'));

    let quiet = jolter_command(project.path(), home.path())
        .args(["pin", "node@22", "--quiet"])
        .output()
        .unwrap();
    assert!(quiet.status.success());
    assert!(quiet.stderr.is_empty());
    assert!(String::from_utf8_lossy(&quiet.stdout).contains("Pinned node@22"));
}

#[test]
fn help_documents_terminal_output_controls() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = jolter_command(project.path(), home.path())
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--no-progress"));
    assert!(stdout.contains("--no-color"));
    assert!(stdout.contains("--quiet"));
    assert!(stdout.contains("--verbose"));
}

#[test]
fn use_activates_an_installed_package_manager() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let node = runtime_executable(home.path(), "node", "24.1.0");
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(node, b"node").unwrap();
    let active = home.path().join("config").join("active.json");
    fs::create_dir_all(active.parent().unwrap()).unwrap();
    fs::write(&active, r#"{"node":"24.1.0"}"#).unwrap();
    let pnpm = home
        .path()
        .join("tools")
        .join("pnpm")
        .join("10.2.0")
        .join("bin")
        .join("pnpm.cjs");
    fs::create_dir_all(pnpm.parent().unwrap()).unwrap();
    fs::write(pnpm, b"pnpm").unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["use", "pnpm@10"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Activated package manager pnpm@10.2.0")
    );
    let active: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(active).unwrap()).unwrap();
    assert_eq!(active["node"], "24.1.0");
    assert_eq!(active["pnpm"], "10.2.0");

    let list = jolter_command(project.path(), home.path())
        .arg("list")
        .output()
        .unwrap();
    assert!(list.status.success());
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(list_stdout.contains("* pnpm@10.2.0"));
    assert!(list_stdout.contains("[ready]"));
    assert!(list.stderr.is_empty());
}

#[test]
fn list_reports_installed_runtime_directories() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("runtimes").join("node").join("24.1.0")).unwrap();

    let output = jolter_command(project.path(), home.path())
        .arg("list")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("node@24.1.0"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("[incomplete]"));
}

#[test]
fn list_reports_managed_package_managers_and_health() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let pnpm = home
        .path()
        .join("tools")
        .join("pnpm")
        .join("10.2.0")
        .join("bin")
        .join("pnpm.cjs");
    fs::create_dir_all(pnpm.parent().unwrap()).unwrap();
    fs::write(pnpm, b"pnpm").unwrap();
    fs::create_dir_all(home.path().join("tools").join("yarn").join("4.1.0")).unwrap();

    let output = jolter_command(project.path(), home.path())
        .arg("list")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Package managers:"));
    assert!(stdout.contains("pnpm@10.2.0"));
    assert!(stdout.contains("yarn@4.1.0"));
    assert!(stdout.contains("[ready]"));
    assert!(stdout.contains("[incomplete]"));
    assert!(output.stderr.is_empty());
}

#[test]
fn list_aligns_status_and_path_columns_without_tabs() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let node = runtime_executable(home.path(), "node", "24.16.0");
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(&node, b"node").unwrap();
    let bun_root = home.path().join("runtimes").join("bun").join("1.2.3");
    fs::create_dir_all(&bun_root).unwrap();

    let output = jolter_command(project.path(), home.path())
        .arg("list")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let bun_line = stdout
        .lines()
        .find(|line| line.contains("bun@1.2.3"))
        .unwrap();
    let node_line = stdout
        .lines()
        .find(|line| line.contains("node@24.16.0"))
        .unwrap();
    assert_eq!(bun_line.find("[incomplete]"), node_line.find("[ready]"));
    assert_eq!(
        bun_line.find(&bun_root.display().to_string()),
        node_line.find(
            &home
                .path()
                .join("runtimes")
                .join("node")
                .join("24.16.0")
                .display()
                .to_string()
        )
    );
    assert!(!stdout.contains('\t'));
    assert!(output.stderr.is_empty());
}

#[test]
fn doctor_can_emit_json() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["doctor", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["healthy"], true);
    assert!(
        value["checks"]
            .as_array()
            .is_some_and(|checks| !checks.is_empty())
    );
    assert_eq!(value["checks"][0]["status"], "pass");
    assert!(output.stderr.is_empty());
}

#[test]
fn doctor_json_remains_parseable_when_checks_fail() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["doctor", "--json"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["healthy"], false);
    assert!(
        value["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "runtime" && check["status"] == "fail")
    );
}

#[test]
fn setup_installs_shims_and_prints_shell_guidance() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["setup", "--shell", "bash"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Installed Jolter shims"));
    assert!(stdout.contains("export PATH="));
    #[cfg(windows)]
    assert!(home.path().join("shims").join("node.exe").is_file());
    #[cfg(not(windows))]
    assert!(home.path().join("shims").join("node").is_file());
}

#[test]
fn sync_installs_a_working_project_aware_shim() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();
    let runtime = home.path().join("runtimes").join("node").join("24.1.0");
    #[cfg(windows)]
    let executable = runtime.join("node.exe");
    #[cfg(not(windows))]
    let executable = runtime.join("bin").join("node");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    #[cfg(windows)]
    fs::copy(std::env::var_os("COMSPEC").unwrap(), &executable).unwrap();
    #[cfg(not(windows))]
    fs::copy("/bin/sh", &executable).unwrap();

    let output = jolter_command(project.path(), home.path())
        .arg("sync")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    #[cfg(windows)]
    let shim = home.path().join("shims").join("node.exe");
    #[cfg(not(windows))]
    let shim = home.path().join("shims").join("node");
    let mut command = Command::new(shim);
    command
        .current_dir(project.path())
        .env("JOLTER_HOME", home.path());
    #[cfg(windows)]
    command.args(["/d", "/c", "echo", "shim-ok"]);
    #[cfg(not(windows))]
    command.args(["-c", "echo shim-ok"]);
    let output = command.output().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("shim-ok"));
}

#[test]
fn uninstall_refuses_an_active_runtime_without_force() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let executable = runtime_executable(home.path(), "node", "24.1.0");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"node").unwrap();

    let use_output = jolter_command(project.path(), home.path())
        .args(["use", "node@24"])
        .output()
        .unwrap();
    assert!(
        use_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&use_output.stderr)
    );

    let refused = jolter_command(project.path(), home.path())
        .args(["uninstall", "node@24.1.0"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("active"));
    assert!(executable.is_file());

    let forced = jolter_command(project.path(), home.path())
        .args(["uninstall", "node@24.1.0", "--force"])
        .output()
        .unwrap();
    assert!(
        forced.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(!executable.exists());
}

#[test]
fn prune_dry_run_preserves_files_and_then_removes_old_versions() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let old = runtime_executable(home.path(), "node", "22.1.0");
    let current = runtime_executable(home.path(), "node", "24.1.0");
    for executable in [&old, &current] {
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(executable, b"node").unwrap();
    }

    let preview = jolter_command(project.path(), home.path())
        .args(["prune", "--keep", "1", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(old.is_file());
    assert!(String::from_utf8_lossy(&preview.stdout).contains("Would remove node@22.1.0"));

    let applied = jolter_command(project.path(), home.path())
        .args(["prune", "--keep", "1"])
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(!old.exists());
    assert!(current.is_file());
}

#[test]
fn cache_status_and_clean_report_reclaimed_files() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cached = home
        .path()
        .join("cache")
        .join("downloads")
        .join(format!("{}.zip", "a".repeat(64)));
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
    fs::write(&cached, b"cached archive").unwrap();

    let status = jolter_command(project.path(), home.path())
        .args(["cache", "status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("1 file(s)"));

    let clean = jolter_command(project.path(), home.path())
        .args(["cache", "clean"])
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(String::from_utf8_lossy(&clean.stdout).contains("Removed 1 cached file(s)"));
    assert!(!cached.exists());
}

#[test]
fn doctor_json_includes_actionable_remediation() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let runtime = value["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "runtime")
        .unwrap();

    assert_eq!(runtime["status"], "fail");
    assert!(
        runtime["remediation"]
            .as_str()
            .unwrap()
            .contains("jolter sync")
    );
}

#[test]
fn list_can_emit_machine_readable_inventory() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let executable = runtime_executable(home.path(), "node", "24.1.0");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(executable, b"node").unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["list", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["runtimes"][0]["kind"], "node");
    assert_eq!(value["runtimes"][0]["version"], "24.1.0");
    assert_eq!(value["runtimes"][0]["ready"], true);
}

#[test]
fn setup_ci_emits_exact_resolved_versions() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();
    let executable = runtime_executable(home.path(), "node", "24.1.0");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(executable, b"node").unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["setup-ci", "--json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["runtime"]["kind"], "node");
    assert_eq!(value["runtime"]["version"], "24.1.0");
    assert!(value["cache"].as_str().is_some());
}

#[test]
fn completions_generates_a_shell_script() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["completions", "bash"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("_jolter"));
}

#[test]
fn setup_prints_guidance_for_every_supported_shell() {
    let project = tempfile::tempdir().unwrap();
    for (shell, expected) in [
        ("powershell", "Current PowerShell session"),
        ("cmd", "Current Command Prompt session"),
        ("zsh", ".zshrc"),
        ("fish", "fish_add_path"),
    ] {
        let home = tempfile::tempdir().unwrap();
        let output = jolter_command(project.path(), home.path())
            .args(["setup", "--shell", shell])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{shell} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(expected),
            "{shell} stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn completions_cover_all_supported_generators() {
    let project = tempfile::tempdir().unwrap();
    for shell in ["elvish", "fish", "powershell", "zsh"] {
        let home = tempfile::tempdir().unwrap();
        let output = jolter_command(project.path(), home.path())
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{shell} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty());
    }
}

#[test]
fn empty_inventory_and_prune_are_noops() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let list = jolter_command(project.path(), home.path())
        .arg("list")
        .output()
        .unwrap();
    assert!(list.status.success());
    assert!(
        String::from_utf8_lossy(&list.stdout).contains("No runtimes or package managers installed")
    );

    let prune = jolter_command(project.path(), home.path())
        .arg("prune")
        .output()
        .unwrap();
    assert!(prune.status.success());
    assert!(String::from_utf8_lossy(&prune.stdout).contains("Nothing to prune"));
}

#[test]
fn uninstall_removes_an_exact_package_manager() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let entrypoint = home
        .path()
        .join("tools")
        .join("pnpm")
        .join("10.2.0")
        .join("bin")
        .join("pnpm.cjs");
    fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
    fs::write(&entrypoint, b"pnpm").unwrap();

    let output = jolter_command(project.path(), home.path())
        .args(["uninstall", "pnpm@10.2.0"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Uninstalled pnpm@10.2.0"));
    assert!(!entrypoint.exists());
}

#[test]
fn uninstall_protects_an_active_package_manager_without_force() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let entrypoint = home
        .path()
        .join("tools")
        .join("pnpm")
        .join("10.2.0")
        .join("bin")
        .join("pnpm.cjs");
    fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
    fs::write(&entrypoint, b"pnpm").unwrap();
    let active = home.path().join("config").join("active.json");
    fs::create_dir_all(active.parent().unwrap()).unwrap();
    fs::write(&active, r#"{"pnpm":"10.2.0"}"#).unwrap();

    let refused = jolter_command(project.path(), home.path())
        .args(["uninstall", "pnpm@10.2.0"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("active pnpm@10.2.0"));
    assert!(entrypoint.exists());

    let forced = jolter_command(project.path(), home.path())
        .args(["uninstall", "pnpm@10.2.0", "--force"])
        .output()
        .unwrap();
    assert!(forced.status.success());
    assert!(!entrypoint.exists());
    let active: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(active).unwrap()).unwrap();
    assert!(active.get("pnpm").is_none());
}

#[test]
fn setup_ci_human_output_and_repair_cover_command_paths() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();
    let executable = runtime_executable(home.path(), "node", "24.1.0");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(executable, b"node").unwrap();

    let setup_ci = jolter_command(project.path(), home.path())
        .arg("setup-ci")
        .output()
        .unwrap();
    assert!(setup_ci.status.success());
    assert!(String::from_utf8_lossy(&setup_ci.stdout).contains("CI provider:"));
    assert!(String::from_utf8_lossy(&setup_ci.stdout).contains("Cache:"));

    let repair = jolter_command(project.path(), home.path())
        .arg("repair")
        .output()
        .unwrap();
    assert!(repair.status.success());
    assert!(String::from_utf8_lossy(&repair.stdout).contains("Repaired node@24.1.0"));
}

#[test]
fn human_doctor_prints_remediation_actions() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();

    let output = jolter_command(project.path(), home.path())
        .arg("doctor")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("action: run `jolter sync`"));
}
