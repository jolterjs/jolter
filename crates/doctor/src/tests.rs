use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use jolter_runtime::{RuntimeKind, ToolKind};
use jolter_storage::{InstalledRuntime, InstalledTool, Storage};
use semver::Version;
use std::{fs, path::PathBuf, process::Command};

#[cfg(not(windows))]
use std::os::unix::fs::PermissionsExt;

use checks::{
    runtime::{runtime_manifest_check, valid_manifest_artifact},
    tool::tool_engine_check,
};
use probe::run_probe;

#[test]
fn reports_a_matching_managed_tool_as_healthy() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();
    fs::write(
        project.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
    )
    .unwrap();
    let node = storage.runtime_executable(RuntimeKind::Node, &Version::new(24, 1, 0));
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(node, b"node").unwrap();
    let pnpm = storage
        .tool_entrypoint(ToolKind::Pnpm, &Version::new(10, 2, 0), "pnpm")
        .unwrap();
    fs::create_dir_all(pnpm.parent().unwrap()).unwrap();
    fs::write(pnpm, b"pnpm").unwrap();

    let report = examine(project.path(), &storage).unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.name == "tool")
        .unwrap();

    assert_eq!(check.status, CheckStatus::Pass);
}

#[test]
fn reports_invalid_configuration_as_a_check() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();
    fs::write(project.path().join("jolter.json"), "{not-json").unwrap();

    let report = examine(project.path(), &storage).unwrap();

    assert!(report.checks.iter().any(|check| {
        check.name == "configuration"
            && check.status == CheckStatus::Fail
            && check.remediation.is_some()
    }));
}

#[test]
fn parses_common_runtime_version_output() {
    assert_eq!(extract_version("v24.2.1\n"), Some(Version::new(24, 2, 1)));
    assert_eq!(
        extract_version("deno 2.4.0 (stable, release)"),
        Some(Version::new(2, 4, 0))
    );
}

#[test]
fn bounded_probe_reports_a_version() {
    #[cfg(windows)]
    {
        let mut command = Command::new(env::var_os("COMSPEC").unwrap());
        command.args(["/d", "/c", "echo", "3.2.1"]);
        let output = run_probe(command).unwrap();
        assert_eq!(
            extract_version(&output.combined_output()),
            Some(Version::new(3, 2, 1))
        );
    }
    #[cfg(not(windows))]
    {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("probe");
        fs::write(&script, "#!/bin/sh\nprintf '3.2.1\\n'").unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        let output = run_probe(Command::new(&script)).unwrap();
        assert_eq!(
            extract_version(&output.combined_output()),
            Some(Version::new(3, 2, 1))
        );
    }
}

#[test]
fn validates_runtime_and_tool_manifests() {
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();
    let runtime = InstalledRuntime {
        kind: RuntimeKind::Node,
        version: Version::new(24, 1, 0),
        path: storage.runtime_version_dir(RuntimeKind::Node, &Version::new(24, 1, 0)),
    };
    fs::create_dir_all(&runtime.path).unwrap();
    fs::write(
        runtime.path.join(".jolter-install.json"),
        r#"{
            "runtime":"node",
            "version":"24.1.0",
            "artifactUrl":"https://nodejs.org/node.zip",
            "integrity":"sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }"#,
    )
    .unwrap();
    assert_eq!(runtime_manifest_check(&runtime).status, CheckStatus::Pass);

    let tool = InstalledTool {
        kind: ToolKind::Pnpm,
        version: Version::new(10, 2, 0),
        path: storage.tool_version_dir(ToolKind::Pnpm, &Version::new(10, 2, 0)),
    };
    fs::create_dir_all(&tool.path).unwrap();
    let integrity = format!("sha512-{}", BASE64.encode([0_u8; 64]));
    fs::write(
        tool.path.join(".jolter-tool.json"),
        format!(
            r#"{{
            "packageManager":"pnpm",
            "version":"10.2.0",
            "artifactUrl":"https://registry.npmjs.org/pnpm.tgz",
            "integrity":"{integrity}"
        }}"#
        ),
    )
    .unwrap();
    assert!(
        checks::tool::tool_checks(
            &storage,
            &jolter_resolver::ProjectResolution {
                root: home.path().to_path_buf(),
                runtime: None,
                tools: Vec::new(),
                plugin_tools: Vec::new(),
                plugins: Vec::new(),
                dev_engines: None,
            },
            None,
            &mut Vec::new()
        )
        .is_ok()
    );

    fs::write(
        tool.path.join(".jolter-tool.json"),
        r#"{"packageManager":"yarn","version":"10.2.0","artifactUrl":"http://example.test/tool","integrity":"bad"}"#,
    )
    .unwrap();
}

#[test]
fn diagnoses_tool_engine_metadata() {
    let home = tempfile::tempdir().unwrap();
    let tool = InstalledTool {
        kind: ToolKind::Pnpm,
        version: Version::new(10, 2, 0),
        path: home.path().join("pnpm"),
    };
    fs::create_dir_all(&tool.path).unwrap();

    fs::write(
        tool.path.join("package.json"),
        r#"{"engines":{"node":"^20.0.0 || >=22"}}"#,
    )
    .unwrap();
    assert_eq!(
        tool_engine_check(&tool, &Version::new(22, 1, 0)).status,
        CheckStatus::Pass
    );
    assert_eq!(
        tool_engine_check(&tool, &Version::new(21, 0, 0)).status,
        CheckStatus::Fail
    );

    fs::write(
        tool.path.join("package.json"),
        r#"{"engines":{"node":"definitely not semver"}}"#,
    )
    .unwrap();
    assert_eq!(
        tool_engine_check(&tool, &Version::new(22, 1, 0)).status,
        CheckStatus::Fail
    );

    fs::write(tool.path.join("package.json"), "{}").unwrap();
    assert_eq!(
        tool_engine_check(&tool, &Version::new(22, 1, 0)).status,
        CheckStatus::Pass
    );
}

#[test]
fn reports_invalid_cache_entries() {
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();
    let invalid = storage
        .cache_dir()
        .join("downloads")
        .join("not-a-cache-key");
    fs::create_dir_all(invalid.parent().unwrap()).unwrap();
    fs::write(invalid, b"bad").unwrap();

    let check = cache_check(&storage).unwrap();

    assert_eq!(check.status, CheckStatus::Warning);
    assert!(check.remediation.unwrap().contains("cache clean"));
}

#[test]
fn version_probe_detects_mismatches_and_nonzero_exits() {
    #[cfg(windows)]
    let version_command = {
        let mut command = Command::new(env::var_os("COMSPEC").unwrap());
        command.args(["/d", "/c", "echo", "3.2.1"]);
        command
    };
    #[cfg(not(windows))]
    let version_command = {
        let mut command = Command::new("sh");
        command.args(["-c", "printf '3.2.1\\n'"]);
        command
    };
    assert_eq!(
        version_probe_check("version", version_command, &Version::new(3, 2, 0), "repair").status,
        CheckStatus::Fail
    );

    #[cfg(windows)]
    let failing_command = {
        let mut command = Command::new(env::var_os("COMSPEC").unwrap());
        command.args(["/d", "/c", "exit", "7"]);
        command
    };
    #[cfg(not(windows))]
    let failing_command = {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 7"]);
        command
    };
    assert_eq!(
        version_probe_check("version", failing_command, &Version::new(3, 2, 1), "repair").status,
        CheckStatus::Fail
    );
}

#[test]
fn diagnoses_dev_engines_requirements() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();

    fs::write(
        project.path().join("package.json"),
        r#"{
            "devEngines": {
                "runtime": { "name": "node", "version": "^24.0.0", "onFail": "error" },
                "cpu": { "name": "x64", "onFail": "error" },
                "os": { "name": "win32", "onFail": "warn" }
            }
        }"#,
    )
    .unwrap();

    let node = storage.runtime_executable(RuntimeKind::Node, &Version::new(24, 1, 0));
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(node, b"node").unwrap();

    let report = examine(project.path(), &storage).unwrap();
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.name == "devEngines: runtime")
    );
    assert!(report.checks.iter().any(|c| c.name == "devEngines: cpu"));
}

#[test]
fn tests_check_status_report_and_extract_version() {
    let report = Report {
        checks: vec![
            Check::pass("test1", "ok"),
            Check::warning("test2", "warn", "fix"),
            Check::fail("test3", "failed", "repair"),
        ],
    };
    assert!(!report.is_healthy());

    assert_eq!(
        extract_version("node v20.11.0"),
        Some(Version::new(20, 11, 0))
    );
    assert_eq!(
        extract_version("pnpm 9.1.0, done"),
        Some(Version::new(9, 1, 0))
    );
    assert_eq!(extract_version("no version here"), None);
}

#[test]
fn tests_valid_manifest_artifact_and_reports() {
    assert!(valid_manifest_artifact(
        "https://example.com/file.tar.gz",
        "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    ));
    assert!(!valid_manifest_artifact(
        "http://insecure.com/file.tar.gz",
        "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    ));
    assert!(!valid_manifest_artifact(
        "https://example.com/file.tar.gz",
        "invalid-hash"
    ));

    let report = Report {
        checks: vec![
            Check::pass("Check 1", "pass detail"),
            Check::warning("Check 2", "warn detail", "warn action"),
            Check::fail("Check 3", "fail detail", "fail action"),
        ],
    };
    assert!(!report.is_healthy());
    assert_eq!(report.checks.len(), 3);
    assert_eq!(report.checks[0].status, CheckStatus::Pass);
    assert_eq!(report.checks[1].status, CheckStatus::Warning);
    assert_eq!(report.checks[2].status, CheckStatus::Fail);
    assert_eq!(report.checks[2].remediation.as_deref(), Some("fail action"));
}

#[test]
fn tests_probe_output_and_unparseable_version() {
    use semver::Version;
    use std::process::Command;

    let probe_empty = crate::probe::ProbeOutput {
        status: Command::new("true").status().unwrap(),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    };
    assert_eq!(probe_empty.combined_output(), "<no output>");

    let probe_both = crate::probe::ProbeOutput {
        status: Command::new("true").status().unwrap(),
        stdout: "out".to_owned(),
        stderr: "err".to_owned(),
        timed_out: false,
    };
    assert_eq!(probe_both.combined_output(), "out; err");

    let mut echo_no_ver = Command::new("echo");
    echo_no_ver.arg("no_semver_here");
    let check = crate::probe::version_probe_check(
        "test_check",
        echo_no_ver,
        &Version::new(1, 0, 0),
        "fix it",
    );
    assert!(!matches!(check.status, crate::types::CheckStatus::Pass));
    assert!(check.message.contains("could not parse a semantic version"));
}

#[test]
fn tests_tool_and_runtime_manifest_checks() {
    use semver::Version;

    let temp = tempfile::tempdir().unwrap();
    let tool = InstalledTool {
        kind: jolter_runtime::ToolKind::Pnpm,
        version: Version::new(10, 0, 0),
        path: temp.path().join("pnpm_tool"),
    };
    fs::create_dir_all(&tool.path).unwrap();

    let missing_check = crate::checks::tool::tool_manifest_check(&tool);
    assert!(matches!(
        missing_check.status,
        crate::types::CheckStatus::Warning
    ));
    assert!(
        missing_check
            .message
            .contains("installation manifest is missing")
    );

    let manifest_path = tool.path.join(".jolter-tool.json");
    fs::write(&manifest_path, b"invalid_json!").unwrap();
    let invalid_check = crate::checks::tool::tool_manifest_check(&tool);
    assert!(matches!(
        invalid_check.status,
        crate::types::CheckStatus::Fail
    ));
    assert!(
        invalid_check
            .message
            .contains("invalid installation manifest")
    );

    fs::write(
        &manifest_path,
        r#"{"tool":"yarn","version":"1.0.0","artifactUrl":"https://example.test/y.tar.gz","integrity":"sha256-abc"}"#,
    )
    .unwrap();
    let mismatch_check = crate::checks::tool::tool_manifest_check(&tool);
    assert!(matches!(
        mismatch_check.status,
        crate::types::CheckStatus::Fail
    ));
    assert!(
        mismatch_check
            .message
            .contains("manifest identity or integrity metadata does not match")
    );

    let runtime = InstalledRuntime {
        kind: jolter_runtime::RuntimeKind::Node,
        version: Version::new(24, 0, 0),
        path: temp.path().join("missing_node_runtime"),
    };
    let perm_missing = crate::checks::runtime::runtime_permission_check(&runtime);
    assert!(matches!(
        perm_missing.status,
        crate::types::CheckStatus::Fail
    ));
    assert!(perm_missing.message.contains("could not inspect"));

    let rt_dir = temp.path().join("rt_node");
    fs::create_dir_all(&rt_dir).unwrap();
    let rt_installed = InstalledRuntime {
        kind: jolter_runtime::RuntimeKind::Node,
        version: Version::new(24, 0, 0),
        path: rt_dir,
    };
    let rt_missing = crate::checks::runtime::runtime_manifest_check(&rt_installed);
    assert!(matches!(
        rt_missing.status,
        crate::types::CheckStatus::Warning
    ));

    let rt_manifest_path = rt_installed.path.join(".jolter-install.json");
    fs::write(&rt_manifest_path, b"invalid_json!").unwrap();
    let rt_invalid = crate::checks::runtime::runtime_manifest_check(&rt_installed);
    assert!(matches!(rt_invalid.status, crate::types::CheckStatus::Fail));

    fs::write(
        &rt_manifest_path,
        r#"{"runtime":"bun","version":"1.0.0","artifactUrl":"https://example.test/b.tar.gz","integrity":"sha256-abc"}"#,
    )
    .unwrap();
    let rt_mismatch = crate::checks::runtime::runtime_manifest_check(&rt_installed);
    assert!(matches!(
        rt_mismatch.status,
        crate::types::CheckStatus::Fail
    ));
}

#[test]
fn tests_tool_engine_check() {
    let temp = tempfile::tempdir().unwrap();
    let tool = InstalledTool {
        kind: ToolKind::Pnpm,
        version: Version::new(10, 0, 0),
        path: temp.path().to_path_buf(),
    };
    let node_v = Version::new(20, 0, 0);

    let check = tool_engine_check(&tool, &node_v);
    assert_eq!(check.status, CheckStatus::Warning);

    fs::write(temp.path().join("package.json"), b"invalid json").unwrap();
    let check = tool_engine_check(&tool, &node_v);
    assert_eq!(check.status, CheckStatus::Fail);

    fs::write(
        temp.path().join("package.json"),
        r#"{"engines":{"node":">=18.0.0"}}"#,
    )
    .unwrap();
    let check = tool_engine_check(&tool, &node_v);
    assert_eq!(check.status, CheckStatus::Pass);

    fs::write(
        temp.path().join("package.json"),
        r#"{"engines":{"node":">=22.0.0"}}"#,
    )
    .unwrap();
    let check = tool_engine_check(&tool, &node_v);
    assert_eq!(check.status, CheckStatus::Fail);
}

#[test]
fn tests_dev_engines_mismatch_and_modes() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();

    fs::write(
        project.path().join("package.json"),
        r#"{
            "devEngines": {
                "runtime": { "name": "node", "version": "<10.0.0", "onFail": "warn" },
                "cpu": { "name": "!x64", "onFail": "warn" },
                "os": { "name": "nonexistent_os", "onFail": "error" }
            }
        }"#,
    )
    .unwrap();

    let report = examine(project.path(), &storage).unwrap();
    assert!(!report.is_healthy());
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.status == CheckStatus::Warning)
    );
    assert!(report.checks.iter().any(|c| c.status == CheckStatus::Fail));
}

#[test]
fn tests_doctor_error_display() {
    let err = DoctorError::ReadDirectory {
        path: PathBuf::from("/invalid/path"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
    };
    assert!(err.to_string().contains("/invalid/path"));
}

#[test]
fn tests_environment_checks_paths() {
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();

    let project = tempfile::tempdir().unwrap();
    let report = examine(project.path(), &storage).unwrap();

    #[cfg(unix)]
    let status = std::os::unix::process::ExitStatusExt::from_raw(0);

    #[cfg(windows)]
    let status = std::os::windows::process::ExitStatusExt::from_abi(0);

    let both_output = probe::ProbeOutput {
        status,
        stdout: "stdout_msg".to_string(),
        stderr: "stderr_msg".to_string(),
        timed_out: false,
    };
    assert_eq!(both_output.combined_output(), "stdout_msg; stderr_msg");

    let no_output = probe::ProbeOutput {
        status,
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    };
    assert_eq!(no_output.combined_output(), "<no output>");

    assert!(report.checks.iter().any(|c| c.name == "PATH"));
    assert!(report.checks.iter().any(|c| c.name == "shims"));
}

#[test]
fn tests_dev_engines_package_manager_checks() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();

    fs::write(
        project.path().join("package.json"),
        r#"{
            "devEngines": {
                "packageManager": { "name": "pnpm", "version": ">=10.0.0", "onFail": "error" }
            }
        }"#,
    )
    .unwrap();

    let report = examine(project.path(), &storage).unwrap();
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.name.starts_with("devEngines"))
    );
}

#[test]
fn tests_environment_checks_extended() {
    let home = tempfile::tempdir().unwrap();
    let storage = Storage::new(home.path());
    storage.ensure_layout().unwrap();

    let check_sw = checks::environment::storage_write_check(&storage);
    assert_eq!(check_sw.status, types::CheckStatus::Pass);

    let check_pf = checks::environment::platform_check();
    assert_eq!(check_pf.status, types::CheckStatus::Pass);

    let check_path_conflict = checks::environment::path_conflict_check(&storage);
    assert_eq!(check_path_conflict.status, types::CheckStatus::Warning);

    let invalid_cache_file = storage.cache_dir().join("downloads").join("invalid.zip");
    fs::create_dir_all(invalid_cache_file.parent().unwrap()).unwrap();
    fs::write(&invalid_cache_file, b"bad").unwrap();

    let check_cc = checks::environment::cache_check(&storage).unwrap();
    assert_eq!(check_cc.status, types::CheckStatus::Warning);

    let probe_both = probe::ProbeOutput {
        status: std::process::Command::new("true").status().unwrap(),
        stdout: "out_text".to_owned(),
        stderr: "err_text".to_owned(),
        timed_out: false,
    };
    assert_eq!(probe_both.combined_output(), "out_text; err_text");

    let probe_none = probe::ProbeOutput {
        status: std::process::Command::new("true").status().unwrap(),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
    };
    assert_eq!(probe_none.combined_output(), "<no output>");

    let mut cmd_echo_wrong = std::process::Command::new("echo");
    cmd_echo_wrong.arg("v1.0.0");
    let check_mismatch = probe::version_probe_check(
        "test probe",
        cmd_echo_wrong,
        &Version::new(2, 0, 0),
        "remediation",
    );
    assert_eq!(check_mismatch.status, types::CheckStatus::Fail);
    assert!(check_mismatch.message.contains("does not match"));

    let mut cmd_echo_bad = std::process::Command::new("echo");
    cmd_echo_bad.arg("no_version_here");
    let check_bad = probe::version_probe_check(
        "test probe",
        cmd_echo_bad,
        &Version::new(1, 0, 0),
        "remediation",
    );
    assert_eq!(check_bad.status, types::CheckStatus::Fail);
    assert!(
        check_bad
            .message
            .contains("could not parse a semantic version")
    );
}
