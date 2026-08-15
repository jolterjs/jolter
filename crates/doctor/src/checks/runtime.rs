use std::{fs, path::Path, process::Command};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use jolter_resolver::ProjectResolution;
use jolter_storage::{InstalledRuntime, Storage};
use serde::Deserialize;

use crate::{error::DoctorError, probe::version_probe_check, types::Check};

pub fn runtime_checks(
    storage: &Storage,
    resolution: &ProjectResolution,
    checks: &mut Vec<Check>,
) -> Result<Option<InstalledRuntime>, DoctorError> {
    let Some(runtime) = &resolution.runtime else {
        checks.push(Check::warning(
            "runtime",
            "no project runtime requirement was found",
            "run `jolter pin node@<version>` or add a supported version file",
        ));
        return Ok(None);
    };
    let matching = storage.find_matching(&runtime.request)?;
    let Some(candidate) = matching else {
        checks.push(Check::fail(
            "runtime",
            format!(
                "{} is required but no complete installation was found",
                runtime.request
            ),
            "run `jolter sync` to install the required runtime",
        ));
        return Ok(None);
    };

    checks.push(Check::pass(
        "runtime",
        format!(
            "{} is satisfied by {}@{}",
            runtime.request, candidate.kind, candidate.version
        ),
    ));
    checks.push(runtime_manifest_check(&candidate));
    checks.push(runtime_permission_check(&candidate));
    checks.push(version_probe_check(
        "runtime version",
        runtime_probe_command(&candidate),
        &candidate.version,
        "run `jolter repair` to replace the runtime installation",
    ));
    Ok(Some(candidate))
}

#[must_use]
pub fn runtime_manifest_check(runtime: &InstalledRuntime) -> Check {
    let path = runtime.path.join(".jolter-install.json");
    let manifest: RuntimeManifest = match read_manifest(&path) {
        Ok(manifest) => manifest,
        Err(ManifestRead::Missing) => {
            return Check::warning(
                "runtime manifest",
                format!("installation manifest is missing at {}", path.display()),
                "run `jolter repair` to recreate a verified installation",
            );
        }
        Err(ManifestRead::Invalid(error)) => {
            return Check::fail(
                "runtime manifest",
                format!(
                    "invalid installation manifest at {}: {error}",
                    path.display()
                ),
                "run `jolter repair` to replace the installation",
            );
        }
    };
    if manifest.runtime != runtime.kind.to_string()
        || manifest.version != runtime.version.to_string()
        || !valid_manifest_artifact(&manifest.artifact_url, &manifest.integrity)
    {
        return Check::fail(
            "runtime manifest",
            format!(
                "manifest identity or integrity metadata does not match {}@{}",
                runtime.kind, runtime.version
            ),
            "run `jolter repair` to replace the installation",
        );
    }
    Check::pass(
        "runtime manifest",
        format!("verified metadata is present at {}", path.display()),
    )
}

#[cfg(unix)]
#[must_use]
pub fn runtime_permission_check(runtime: &InstalledRuntime) -> Check {
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(runtime.executable()) {
        Ok(metadata) if metadata.permissions().mode() & 0o111 != 0 => Check::pass(
            "runtime permissions",
            "runtime executable permission is set",
        ),
        Ok(_) => Check::fail(
            "runtime permissions",
            format!("{} is not executable", runtime.executable().display()),
            "run `jolter repair` to restore executable permissions",
        ),
        Err(error) => Check::fail(
            "runtime permissions",
            format!(
                "could not inspect {}: {error}",
                runtime.executable().display()
            ),
            "run `jolter repair` to replace the installation",
        ),
    }
}

#[cfg(not(unix))]
#[must_use]
pub fn runtime_permission_check(_runtime: &InstalledRuntime) -> Check {
    Check::pass(
        "runtime permissions",
        "runtime executable is present on Windows",
    )
}

#[must_use]
pub fn runtime_probe_command(runtime: &InstalledRuntime) -> Command {
    let mut command = Command::new(runtime.executable());
    command.arg("--version");
    command
}

#[must_use]
pub fn valid_manifest_artifact(url: &str, integrity: &str) -> bool {
    url.starts_with("https://")
        && (integrity.strip_prefix("sha256-").is_some_and(|value| {
            value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
        }) || integrity
            .strip_prefix("sha512-")
            .and_then(|value| BASE64.decode(value).ok())
            .is_some_and(|value| value.len() == 64))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeManifest {
    runtime: String,
    version: String,
    artifact_url: String,
    integrity: String,
}

pub(crate) enum ManifestRead {
    Missing,
    Invalid(String),
}

pub(crate) fn read_manifest<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ManifestRead> {
    let contents = fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ManifestRead::Missing
        } else {
            ManifestRead::Invalid(error.to_string())
        }
    })?;
    serde_json::from_str(&contents).map_err(|error| ManifestRead::Invalid(error.to_string()))
}
