use std::env;

use jolter_resolver::{DevEngineItem, DevEngineOnFail, ProjectResolution};
use jolter_storage::InstalledRuntime;
use nodejs_semver::{Range as NodeRange, Version as NodeVersion};

use crate::types::Check;

pub fn dev_engines_checks(
    resolution: &ProjectResolution,
    runtime: Option<&InstalledRuntime>,
    checks: &mut Vec<Check>,
) {
    let Some(dev_engines) = &resolution.dev_engines else {
        return;
    };

    dev_engines_runtime_checks(&dev_engines.runtime, runtime, checks);
    dev_engines_package_manager_checks(&dev_engines.package_manager, checks);
    dev_engines_cpu_checks(&dev_engines.cpu, checks);
    dev_engines_os_checks(&dev_engines.os, checks);
}

fn dev_engines_runtime_checks(
    items: &[DevEngineItem],
    runtime: Option<&InstalledRuntime>,
    checks: &mut Vec<Check>,
) {
    for item in items {
        let (name, selector) = item.parsed_name_and_selector();
        let mode = item.on_fail_mode();
        if mode == DevEngineOnFail::Ignore {
            continue;
        }

        let Some(installed) = runtime else {
            let msg = format!(
                "runtime `{name}` is specified in devEngines but no matching runtime is active"
            );
            match mode {
                DevEngineOnFail::Warn => checks.push(Check::warning(
                    "devEngines: runtime",
                    msg,
                    "run `jolter sync` to install the runtime",
                )),
                _ => checks.push(Check::fail(
                    "devEngines: runtime",
                    msg,
                    "run `jolter sync` to install the runtime",
                )),
            }
            continue;
        };

        if installed.kind.to_string().eq_ignore_ascii_case(&name) {
            if let Some(req) = &selector {
                if let Ok(range) = NodeRange::parse(req) {
                    let version = NodeVersion::from((
                        installed.version.major,
                        installed.version.minor,
                        installed.version.patch,
                    ));
                    if range.satisfies(&version) {
                        checks.push(Check::pass(
                            "devEngines: runtime",
                            format!(
                                "runtime {}@{} satisfies devEngines requirement `{name}` `{req}`",
                                installed.kind, installed.version
                            ),
                        ));
                    } else {
                        let msg = format!(
                            "runtime {}@{} does not satisfy devEngines requirement `{req}`",
                            installed.kind, installed.version
                        );
                        let remed = "pin a compatible runtime version or update package.json#devEngines.runtime";
                        match mode {
                            DevEngineOnFail::Warn => {
                                checks.push(Check::warning("devEngines: runtime", msg, remed));
                            }
                            _ => checks.push(Check::fail("devEngines: runtime", msg, remed)),
                        }
                    }
                } else {
                    checks.push(Check::pass(
                        "devEngines: runtime",
                        format!(
                            "runtime {}@{} matches `{name}`",
                            installed.kind, installed.version
                        ),
                    ));
                }
            } else {
                checks.push(Check::pass(
                    "devEngines: runtime",
                    format!(
                        "runtime {}@{} matches `{name}`",
                        installed.kind, installed.version
                    ),
                ));
            }
        }
    }
}

fn dev_engines_package_manager_checks(items: &[DevEngineItem], checks: &mut Vec<Check>) {
    for item in items {
        let (name, _selector) = item.parsed_name_and_selector();
        let mode = item.on_fail_mode();
        if mode == DevEngineOnFail::Ignore {
            continue;
        }

        let _is_negated = name.starts_with('!');
        let clean_name = name.strip_prefix('!').unwrap_or(&name);

        checks.push(Check::pass(
            "devEngines: packageManager",
            format!("devEngines requirement specified for packageManager `{clean_name}`"),
        ));
    }
}

fn dev_engines_cpu_checks(items: &[DevEngineItem], checks: &mut Vec<Check>) {
    for item in items {
        let mode = item.on_fail_mode();
        if mode == DevEngineOnFail::Ignore {
            continue;
        }
        let current_arch = env::consts::ARCH;
        let is_negated = item.name.starts_with('!');
        let clean_name = item.name.strip_prefix('!').unwrap_or(&item.name);
        let base_match = match (current_arch, clean_name.to_ascii_lowercase().as_str()) {
            ("x86_64", "x64" | "x86_64" | "amd64")
            | ("aarch64", "arm64" | "aarch64")
            | ("x86", "ia32" | "x86" | "i686") => true,
            (actual, required) => actual.eq_ignore_ascii_case(required),
        };
        let is_match = if is_negated { !base_match } else { base_match };

        if is_match {
            checks.push(Check::pass(
                "devEngines: cpu",
                format!("system architecture `{current_arch}` satisfies devEngines cpu requirement `{}`", item.name),
            ));
        } else {
            let msg = format!(
                "system architecture `{current_arch}` does not match devEngines cpu requirement `{}`",
                item.name
            );
            let remed = "run on a supported CPU architecture or update package.json#devEngines.cpu";
            match mode {
                DevEngineOnFail::Warn => checks.push(Check::warning("devEngines: cpu", msg, remed)),
                _ => checks.push(Check::fail("devEngines: cpu", msg, remed)),
            }
        }
    }
}

fn dev_engines_os_checks(items: &[DevEngineItem], checks: &mut Vec<Check>) {
    for item in items {
        let mode = item.on_fail_mode();
        if mode == DevEngineOnFail::Ignore {
            continue;
        }
        let current_os = env::consts::OS;
        let is_negated = item.name.starts_with('!');
        let clean_name = item.name.strip_prefix('!').unwrap_or(&item.name);
        let base_match = match (current_os, clean_name.to_ascii_lowercase().as_str()) {
            ("windows", "win32" | "windows")
            | ("macos", "darwin" | "macos" | "osx")
            | ("linux", "linux") => true,
            (actual, required) => actual.eq_ignore_ascii_case(required),
        };
        let is_match = if is_negated { !base_match } else { base_match };

        if is_match {
            checks.push(Check::pass(
                "devEngines: os",
                format!(
                    "operating system `{current_os}` satisfies devEngines os requirement `{}`",
                    item.name
                ),
            ));
        } else {
            let msg = format!(
                "operating system `{current_os}` does not match devEngines os requirement `{}`",
                item.name
            );
            let remed = "run on a supported operating system or update package.json#devEngines.os";
            match mode {
                DevEngineOnFail::Warn => checks.push(Check::warning("devEngines: os", msg, remed)),
                _ => checks.push(Check::fail("devEngines: os", msg, remed)),
            }
        }
    }
}
