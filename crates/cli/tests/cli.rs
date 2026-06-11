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
