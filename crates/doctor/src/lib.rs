pub mod checks;
pub mod error;
pub mod probe;
pub mod types;

#[cfg(test)]
mod tests;

use std::{path::Path, time::Duration};

use jolter_resolver::resolve;
use jolter_storage::Storage;

pub use checks::{
    cache_check, dev_engines_checks, network_environment_check, path_check, path_conflict_check,
    platform_check, runtime_checks, shim_check, storage_write_check, tool_checks,
};
pub use error::DoctorError;
pub use probe::{extract_version, version_probe_check};
pub use types::{Check, CheckStatus, Report};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROBE_OUTPUT: u64 = 16 * 1024;

pub fn examine(project: &Path, storage: &Storage) -> Result<Report, DoctorError> {
    let mut checks = vec![
        Check::pass("storage", format!("using {}", storage.root().display())),
        storage_write_check(storage),
        platform_check(),
    ];
    let resolution = match resolve(project) {
        Ok(resolution) => {
            checks.push(Check::pass(
                "configuration",
                format!(
                    "project requirements resolved from {}",
                    resolution.root.display()
                ),
            ));
            Some(resolution)
        }
        Err(error) => {
            checks.push(Check::fail(
                "configuration",
                error.to_string(),
                "fix the project requirement file reported above, then rerun `jolter doctor`",
            ));
            None
        }
    };

    if let Some(resolution) = resolution.as_ref() {
        let matching_runtime = runtime_checks(storage, resolution, &mut checks)?;
        tool_checks(storage, resolution, matching_runtime.as_ref(), &mut checks)?;
        dev_engines_checks(resolution, matching_runtime.as_ref(), &mut checks);
    } else {
        checks.push(Check::warning(
            "runtime",
            "runtime health was not evaluated because project configuration is invalid",
            "fix the configuration check first",
        ));
        checks.push(Check::warning(
            "tools",
            "tool health was not evaluated because project configuration is invalid",
            "fix the configuration check first",
        ));
    }

    checks.push(shim_check(storage));
    checks.push(path_check(storage));
    checks.push(path_conflict_check(storage));
    checks.push(cache_check(storage)?);
    checks.push(network_environment_check());

    Ok(Report { checks })
}
