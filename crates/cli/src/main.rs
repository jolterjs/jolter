use std::{
    env,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
};

use clap::{Parser, Subcommand, ValueEnum};
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
    /// Install shims and print shell-specific PATH setup commands.
    Setup {
        /// Shell to configure. Auto detects the current shell where possible.
        #[arg(long, value_enum, default_value_t = SetupShell::Auto)]
        shell: SetupShell,
    },
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
    /// List locally installed runtimes and package managers.
    List,
    /// Check project toolchain health.
    Doctor {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Repair detected toolchain problems.
    Repair,
    /// Synchronize the local toolchain with project requirements.
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SetupShell {
    Auto,
    Powershell,
    Cmd,
    Bash,
    Zsh,
    Fish,
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
        Command::Setup { shell } => {
            install_shims(&jolter)?;
            print_setup(&jolter, resolve_setup_shell(shell));
            Ok(ExitCode::SUCCESS)
        }
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
            print_inventory(&jolter)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor { json } => run_doctor(&jolter, &current_dir, json),
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

fn print_inventory(jolter: &Jolter) -> Result<(), CliError> {
    let runtimes = jolter.list()?;
    let tools = jolter.list_tools()?;
    if runtimes.is_empty() && tools.is_empty() {
        println!("No runtimes or package managers installed.");
        return Ok(());
    }
    if !runtimes.is_empty() {
        println!("Runtimes:");
        for runtime in runtimes {
            let active = jolter.storage().active_version(runtime.kind)?;
            let marker = if active.as_ref() == Some(&runtime.version) {
                "*"
            } else {
                " "
            };
            println!(
                "{marker} {}@{} [{}]\t{}",
                runtime.kind,
                runtime.version,
                installation_status(runtime.is_complete()),
                runtime.path.display()
            );
        }
    }
    if !tools.is_empty() {
        println!("Package managers:");
        for tool in tools {
            println!(
                "  {}@{} [{}]\t{}",
                tool.kind,
                tool.version,
                installation_status(tool.is_complete()),
                tool.path.display()
            );
        }
    }
    Ok(())
}

fn run_doctor(jolter: &Jolter, project: &Path, json: bool) -> Result<ExitCode, CliError> {
    let report = jolter.doctor(project)?;
    if json {
        let output = serde_json::json!({
            "healthy": report.is_healthy(),
            "checks": &report.checks,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).map_err(CliError::Json)?
        );
    } else {
        for check in &report.checks {
            let symbol = match check.status {
                CheckStatus::Pass => "ok",
                CheckStatus::Warning => "warn",
                CheckStatus::Fail => "fail",
            };
            println!("[{symbol}] {}: {}", check.name, check.message);
        }
    }
    Ok(if report.is_healthy() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

const fn installation_status(complete: bool) -> &'static str {
    if complete { "ready" } else { "incomplete" }
}

fn print_setup(jolter: &Jolter, shell: SetupShell) {
    let shims = jolter.storage().shims_dir();
    println!("Installed Jolter shims in {}", shims.display());
    if path_contains(&shims) {
        println!("Jolter shims are already available on PATH in this process.");
        return;
    }

    println!("Add the shims directory to PATH:");
    match shell {
        SetupShell::Powershell => {
            let path = powershell_quote(&shims);
            println!("\nCurrent PowerShell session:");
            println!("$env:PATH = '{path};' + $env:PATH");
            println!("\nPersist for the current user:");
            println!("{}", powershell_persist_command(&path));
        }
        SetupShell::Cmd => {
            let display = shims.display();
            let path = powershell_quote(&shims);
            println!("\nCurrent Command Prompt session:");
            println!("set \"PATH={display};%PATH%\"");
            println!("\nPersist for the current user:");
            println!(
                "powershell -NoProfile -Command \"{}\"",
                powershell_persist_script(&format!("'{path}'"))
            );
        }
        SetupShell::Bash | SetupShell::Zsh => {
            let path = posix_double_quote_content(&shims);
            let export = format!("export PATH=\"{path}:$PATH\"");
            let profile = if shell == SetupShell::Zsh {
                "~/.zshrc"
            } else {
                "~/.bashrc"
            };
            println!("\nCurrent shell session:");
            println!("{export}");
            println!("\nPersist for future sessions:");
            println!("printf '%s\\n' {} >> {profile}", posix_quote(&export));
        }
        SetupShell::Fish => {
            println!("\nCurrent and future Fish sessions:");
            println!("fish_add_path {}", posix_quote(&shims.to_string_lossy()));
        }
        SetupShell::Auto => unreachable!("auto shell must be resolved before printing setup"),
    }
    println!("\nRestart the shell after applying persistent PATH changes.");
}

fn resolve_setup_shell(shell: SetupShell) -> SetupShell {
    if shell != SetupShell::Auto {
        return shell;
    }
    if cfg!(windows) {
        return SetupShell::Powershell;
    }
    let detected = env::var_os("SHELL")
        .and_then(|value| {
            PathBuf::from(value)
                .file_stem()
                .map(std::borrow::ToOwned::to_owned)
        })
        .map(|name| name.to_string_lossy().to_ascii_lowercase());
    match detected.as_deref() {
        Some("zsh") => SetupShell::Zsh,
        Some("fish") => SetupShell::Fish,
        _ => SetupShell::Bash,
    }
}

fn path_contains(directory: &Path) -> bool {
    env::var_os("PATH")
        .is_some_and(|value| env::split_paths(&value).any(|entry| same_path(&entry, directory)))
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

fn powershell_quote(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

fn powershell_persist_command(path: &str) -> String {
    format!(
        "$jolter = '{path}'; {}",
        powershell_persist_script("$jolter")
    )
}

fn powershell_persist_script(path_expression: &str) -> String {
    format!(
        "$userPath = [Environment]::GetEnvironmentVariable('Path', 'User'); \
if (-not $userPath) {{ $userPath = '' }}; \
if (($userPath -split ';') -notcontains {path_expression}) {{ \
[Environment]::SetEnvironmentVariable('Path', \
(($userPath.TrimEnd(';') + ';' + {path_expression}).Trim(';')), 'User') }}"
    )
}

fn posix_double_quote_content(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn print_sync_outcome(prefix: &str, outcome: &SyncOutcome) {
    println!(
        "{prefix} {}@{} at {}",
        outcome.runtime.kind,
        outcome.runtime.version,
        outcome.runtime.path.display()
    );
    if let Some(package_manager) = &outcome.package_manager {
        let verb = if package_manager.downloaded {
            "Installed"
        } else {
            "Selected"
        };
        println!(
            "{verb} package manager {}@{} at {}",
            package_manager.tool.kind,
            package_manager.tool.version,
            package_manager.tool.path.display()
        );
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
    command.args(&resolved.arguments);
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
    #[error("failed to serialize command output: {0}")]
    Json(#[source] serde_json::Error),
}
