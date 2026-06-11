use std::{
    env,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
};

use clap::{Parser, Subcommand};
use jolter_core::{Jolter, SyncOutcome};
use jolter_doctor::CheckStatus;
use jolter_runtime::{RuntimeKind, RuntimeRequest};
use jolter_shim::{resolve_command, target_for_command};
use jolter_storage::Storage;
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(name = "jolter", version, about = "JavaScript toolchain manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Install and activate a runtime.
    Use {
        /// Runtime request, for example node@24.
        runtime: RuntimeRequest,
    },
    /// Write a runtime requirement to jolter.json.
    Pin {
        /// Runtime request, for example node@24.
        runtime: RuntimeRequest,
    },
    /// List locally installed runtimes.
    List,
    /// Check project toolchain health.
    Doctor,
    /// Repair detected toolchain problems.
    Repair,
    /// Synchronize the local toolchain with project requirements.
    Sync,
}

fn main() -> ExitCode {
    if let Some(shim) = invoked_shim() {
        return match run_shim(&shim) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("{shim}: {error}");
                ExitCode::FAILURE
            }
        };
    }

    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, CliError> {
    let jolter = Jolter::discover()?;
    let current_dir = env::current_dir().map_err(CliError::CurrentDirectory)?;

    match cli.command {
        Command::Use { runtime } => {
            let action = jolter.use_runtime(&runtime)?;
            install_shims(&jolter)?;
            let verb = if action.downloaded {
                "Installed and activated"
            } else {
                "Activated"
            };
            println!(
                "{verb} {}@{} at {}",
                action.runtime.kind,
                action.runtime.version,
                action.runtime.path.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Pin { runtime } => {
            jolter.pin(&current_dir, &runtime)?;
            println!(
                "Pinned {runtime} in {}",
                current_dir.join("jolter.json").display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::List => {
            let installed = jolter.list()?;
            if installed.is_empty() {
                println!("No runtimes installed.");
            } else {
                for runtime in installed {
                    let active = jolter.storage().active_version(runtime.kind)?;
                    let marker = if active.as_ref() == Some(&runtime.version) {
                        "*"
                    } else {
                        " "
                    };
                    println!(
                        "{marker} {}@{}\t{}",
                        runtime.kind,
                        runtime.version,
                        runtime.path.display()
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            let report = jolter.doctor(&current_dir)?;
            for check in &report.checks {
                let symbol = match check.status {
                    CheckStatus::Pass => "ok",
                    CheckStatus::Warning => "warn",
                    CheckStatus::Fail => "fail",
                };
                println!("[{symbol}] {}: {}", check.name, check.message);
            }
            Ok(if report.is_healthy() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Sync => {
            let outcome = jolter.sync(&current_dir)?;
            install_shims(&jolter)?;
            print_sync_outcome("Synchronized", &outcome);
            Ok(ExitCode::SUCCESS)
        }
        Command::Repair => {
            let outcome = jolter.repair(&current_dir)?;
            install_shims(&jolter)?;
            print_sync_outcome("Repaired", &outcome);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_sync_outcome(prefix: &str, outcome: &SyncOutcome) {
    println!(
        "{prefix} {}@{} at {}",
        outcome.runtime.kind,
        outcome.runtime.version,
        outcome.runtime.path.display()
    );
    if let Some(package_manager) = &outcome.package_manager {
        if outcome.package_manager_ready {
            println!(
                "Package manager {}@{} is available.",
                package_manager.name, package_manager.selector
            );
        } else {
            println!(
                "warning: package manager {}@{} is configured but not installed",
                package_manager.name, package_manager.selector
            );
        }
    }
}

fn install_shims(jolter: &Jolter) -> Result<(), CliError> {
    let executable = env::current_exe().map_err(CliError::CurrentExecutable)?;
    jolter.install_shims(&executable)?;
    Ok(())
}

fn invoked_shim() -> Option<String> {
    let argument = env::args_os().next()?;
    let name = Path::new(&argument)
        .file_stem()?
        .to_string_lossy()
        .to_ascii_lowercase();
    target_for_command(&name).is_some().then_some(name)
}

fn run_shim(shim: &str) -> Result<ExitCode, CliError> {
    let storage = Storage::discover()?;
    let current_dir = env::current_dir().map_err(CliError::CurrentDirectory)?;
    let resolved = resolve_command(shim, &current_dir, &storage)?;
    let mut command = runtime_command(&resolved.executable);
    command.args(env::args_os().skip(1));
    prepend_runtime_path(&mut command, resolved.runtime.kind, &resolved.runtime_root)?;

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        Err(CliError::Launch {
            path: resolved.executable,
            source: error,
        })
    }
    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|source| CliError::Launch {
            path: resolved.executable,
            source,
        })?;
        Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from))
    }
}

fn runtime_command(executable: &Path) -> ProcessCommand {
    ProcessCommand::new(executable)
}

fn prepend_runtime_path(
    command: &mut ProcessCommand,
    kind: RuntimeKind,
    runtime_root: &Path,
) -> Result<(), CliError> {
    let binary_directory = if kind == RuntimeKind::Node && !cfg!(windows) {
        runtime_root.join("bin")
    } else {
        runtime_root.to_path_buf()
    };
    let mut paths = vec![binary_directory];
    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }
    let path = env::join_paths(paths).map_err(CliError::JoinPath)?;
    command.env("PATH", path);
    command.env("JOLTER_RUNTIME_ROOT", runtime_root);
    Ok(())
}

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Core(#[from] jolter_core::CoreError),
    #[error(transparent)]
    Shim(#[from] jolter_shim::ShimError),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
    #[error("failed to determine the current directory: {0}")]
    CurrentDirectory(#[source] std::io::Error),
    #[error("failed to determine the Jolter executable: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("failed to launch {path}: {source}")]
    Launch {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to construct runtime PATH: {0}")]
    JoinPath(#[source] env::JoinPathsError),
}
