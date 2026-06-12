use std::{fs, process::Command};

fn jolter_command(project: &std::path::Path, home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jolter"));
    command.current_dir(project).env("JOLTER_HOME", home);
    command
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
    assert!(stdout.contains("pnpm@10.2.0 [ready]"));
    assert!(stdout.contains("yarn@4.1.0 [incomplete]"));
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
