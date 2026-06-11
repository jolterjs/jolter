use std::{env, path::Path};

use jolter_resolver::resolve;
use jolter_runtime::RuntimeKind;
use jolter_shim::SHIM_COMMANDS;
use jolter_storage::{InstalledRuntime, Storage};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub status: CheckStatus,
    pub name: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.checks
            .iter()
            .all(|check| check.status != CheckStatus::Fail)
    }
}

pub fn examine(project: &Path, storage: &Storage) -> Result<Report, DoctorError> {
    let resolution = resolve(project)?;
    let mut checks = vec![
        Check {
            status: CheckStatus::Pass,
            name: "storage",
            message: format!("using {}", storage.root().display()),
        },
        Check {
            status: CheckStatus::Pass,
            name: "configuration",
            message: format!(
                "project requirements resolved from {}",
                resolution.root.display()
            ),
        },
    ];

    let matching_runtime = if let Some(runtime) = &resolution.runtime {
        let matching = storage.find_matching(&runtime.request)?;
        checks.push(match &matching {
            Some(candidate) => Check {
                status: CheckStatus::Pass,
                name: "runtime",
                message: format!(
                    "{} is satisfied by {}@{}",
                    runtime.request, candidate.kind, candidate.version
                ),
            },
            None => Check {
                status: CheckStatus::Fail,
                name: "runtime",
                message: format!(
                    "{} is required but no complete installation was found",
                    runtime.request
                ),
            },
        });
        matching
    } else {
        checks.push(Check {
            status: CheckStatus::Warning,
            name: "runtime",
            message: "no project runtime requirement was found".to_owned(),
        });
        None
    };

    checks.push(package_manager_check(
        storage,
        matching_runtime.as_ref(),
        resolution.package_manager.as_ref(),
    ));
    checks.push(shim_check(storage));
    checks.push(path_check(storage));

    Ok(Report { checks })
}

fn package_manager_check(
    storage: &Storage,
    runtime: Option<&InstalledRuntime>,
    package_manager: Option<&jolter_resolver::PackageManagerRequest>,
) -> Check {
    let Some(package_manager) = package_manager else {
        return Check {
            status: CheckStatus::Warning,
            name: "package manager",
            message: "no package manager requirement was found".to_owned(),
        };
    };
    let Some(runtime) = runtime.filter(|runtime| runtime.kind == RuntimeKind::Node) else {
        return Check {
            status: CheckStatus::Fail,
            name: "package manager",
            message: format!(
                "{}@{} requires an installed Node.js runtime",
                package_manager.name, package_manager.selector
            ),
        };
    };
    let executable = storage.node_tool_executable(&runtime.version, &package_manager.name);
    if executable.is_file() {
        Check {
            status: CheckStatus::Warning,
            name: "package manager",
            message: format!(
                "{}@{} executable is present at {}; version probing is not implemented yet",
                package_manager.name,
                package_manager.selector,
                executable.display()
            ),
        }
    } else {
        Check {
            status: CheckStatus::Fail,
            name: "package manager",
            message: format!(
                "{}@{} is configured but no executable exists at {}",
                package_manager.name,
                package_manager.selector,
                executable.display()
            ),
        }
    }
}

fn shim_check(storage: &Storage) -> Check {
    let missing = SHIM_COMMANDS
        .iter()
        .filter(|command| {
            let file_name = if cfg!(windows) {
                format!("{command}.exe")
            } else {
                command.to_string()
            };
            !storage.shims_dir().join(file_name).is_file()
        })
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Check {
            status: CheckStatus::Pass,
            name: "shims",
            message: format!(
                "all shims are installed in {}",
                storage.shims_dir().display()
            ),
        }
    } else {
        Check {
            status: CheckStatus::Warning,
            name: "shims",
            message: format!("missing shims: {}", missing.join(", ")),
        }
    }
}

fn path_check(storage: &Storage) -> Check {
    let on_path = env::var_os("PATH").is_some_and(|value| {
        env::split_paths(&value).any(|entry| same_path(&entry, &storage.shims_dir()))
    });
    if on_path {
        Check {
            status: CheckStatus::Pass,
            name: "PATH",
            message: format!("{} is on PATH", storage.shims_dir().display()),
        }
    } else {
        Check {
            status: CheckStatus::Warning,
            name: "PATH",
            message: format!("add {} to PATH", storage.shims_dir().display()),
        }
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

#[derive(Debug, Error)]
pub enum DoctorError {
    #[error(transparent)]
    Resolver(#[from] jolter_resolver::ResolverError),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
}
