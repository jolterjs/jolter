mod output;

use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
    sync::Arc,
};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{
    generate,
    shells::{Bash, Elvish, Fish, PowerShell, Zsh},
};
use jolter_core::{Jolter, PruneOutcome, SyncOutcome};
use jolter_doctor::CheckStatus;
use jolter_runtime::{PackageManagerKind, PackageManagerRequest, RuntimeKind, RuntimeRequest};
use jolter_shim::{resolve_command, target_for_command};
use jolter_storage::Storage;
use semver::Version;
use thiserror::Error;

use crate::output::{
    ColorPreference, DetailLevel, OutputKind, OutputOptions, ProgressPreference, TableRow,
    TerminalUi,
};

#[derive(Debug, Parser)]
#[command(name = "jolter", version, about = "JavaScript toolchain manager")]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Disable the live updating progress line.
    #[arg(long, global = true)]
    no_progress: bool,
    /// Disable ANSI colors.
    #[arg(long, global = true)]
    no_color: bool,
    /// Suppress operational progress while retaining command results.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    /// Include transfer timing and additional operational detail.
    #[arg(short, long, global = true, conflicts_with = "quiet")]
    verbose: bool,
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
    /// Install and activate a runtime or package manager.
    Use {
        /// Tool request, for example node@24 or pnpm@10.
        #[arg(value_parser = parse_use_target)]
        target: UseTarget,
    },
    /// Write a runtime requirement to jolter.json.
    Pin {
        /// Runtime request, for example node@24.
        runtime: RuntimeRequest,
    },
    /// List locally installed runtimes and package managers.
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
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
    /// Remove one exact runtime or package manager version.
    Uninstall {
        /// Exact version to remove, for example node@24.1.0 or pnpm@10.2.0.
        #[arg(value_parser = parse_uninstall_target)]
        target: UninstallTarget,
        /// Permit removal when the runtime or package manager is globally active.
        #[arg(long)]
        force: bool,
    },
    /// Remove old and incomplete installations.
    Prune {
        /// Number of newest complete versions to keep for each tool.
        #[arg(long, default_value_t = 1)]
        keep: usize,
        /// Print what would be removed without changing storage.
        #[arg(long)]
        dry_run: bool,
    },
    /// Inspect or clean Jolter's download and metadata cache.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// Synchronize the project and expose its exact toolchain to CI.
    SetupCi {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Generate shell completion scripts.
    Completions {
        /// Shell whose completion script should be generated.
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Debug, Clone, Copy, Subcommand)]
enum CacheCommand {
    /// Report the number and size of cached files.
    Status,
    /// Remove cached downloads and release metadata.
    Clean,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
}

#[derive(Debug, Clone)]
enum UninstallTarget {
    Runtime(RuntimeKind, Version),
    PackageManager(PackageManagerKind, Version),
}

#[derive(Debug, Clone)]
enum UseTarget {
    Runtime(RuntimeRequest),
    PackageManager(PackageManagerRequest),
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

    let cli = Cli::parse();
    let ui = Arc::new(TerminalUi::new(OutputOptions {
        progress: if cli.no_progress {
            ProgressPreference::Plain
        } else {
            ProgressPreference::Auto
        },
        color: if cli.no_color {
            ColorPreference::Never
        } else {
            ColorPreference::Auto
        },
        detail: if cli.quiet {
            DetailLevel::Quiet
        } else if cli.verbose {
            DetailLevel::Verbose
        } else {
            DetailLevel::Normal
        },
        kind: if cli.machine_output() {
            OutputKind::Machine
        } else {
            OutputKind::Human
        },
    }));
    match run(cli, &ui) {
        Ok(code) => code,
        Err(error) => {
            ui.failure(error.to_string());
            ExitCode::FAILURE
        }
    }
}

impl Cli {
    const fn machine_output(&self) -> bool {
        matches!(
            &self.command,
            Command::List { json: true }
                | Command::Doctor { json: true }
                | Command::SetupCi { json: true }
                | Command::Completions { .. }
        )
    }
}

fn run(cli: Cli, ui: &Arc<TerminalUi>) -> Result<ExitCode, CliError> {
    let jolter = Jolter::discover_with_reporter(ui.clone())?;
    let current_dir = env::current_dir().map_err(CliError::CurrentDirectory)?;

    match cli.command {
        Command::Setup { shell } => {
            install_shims(&jolter)?;
            print_setup(&jolter, resolve_setup_shell(shell), ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Use { target } => run_use(&jolter, target, ui),
        Command::Pin { runtime } => {
            jolter.pin(&current_dir, &runtime)?;
            ui.success(format!(
                "Pinned {runtime} in {}",
                current_dir.join("jolter.json").display()
            ));
            Ok(ExitCode::SUCCESS)
        }
        Command::List { json } => {
            if json {
                ui.finish_progress();
                print_inventory_json(&jolter)?;
            } else {
                print_inventory(&jolter, ui)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor { json } => run_doctor(&jolter, &current_dir, json, ui),
        Command::Sync => {
            let outcome = jolter.sync(&current_dir)?;
            install_shims(&jolter)?;
            print_sync_outcome("Synchronized", &outcome, ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Repair => {
            let outcome = jolter.repair(&current_dir)?;
            install_shims(&jolter)?;
            print_sync_outcome("Repaired", &outcome, ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Uninstall { target, force } => run_uninstall(&jolter, target, force, ui),
        Command::Prune { keep, dry_run } => run_prune(&jolter, &current_dir, keep, dry_run, ui),
        Command::Cache { command } => run_cache(&jolter, command, ui),
        Command::SetupCi { json } => run_setup_ci(&jolter, &current_dir, json, ui),
        Command::Completions { shell } => {
            ui.finish_progress();
            print_completions(shell);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_use(jolter: &Jolter, target: UseTarget, ui: &TerminalUi) -> Result<ExitCode, CliError> {
    match target {
        UseTarget::Runtime(request) => {
            let action = jolter.use_runtime(&request)?;
            install_shims(jolter)?;
            let verb = if action.downloaded {
                "Installed and activated"
            } else {
                "Activated"
            };
            ui.success(format!(
                "{verb} {}@{} at {}",
                action.runtime.kind,
                action.runtime.version,
                action.runtime.path.display()
            ));
        }
        UseTarget::PackageManager(request) => {
            let action = jolter.use_package_manager(&request)?;
            install_shims(jolter)?;
            let verb = if action.downloaded {
                "Installed and activated"
            } else {
                "Activated"
            };
            ui.success(format!(
                "{verb} package manager {}@{} at {}",
                action.tool.kind,
                action.tool.version,
                action.tool.path.display()
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_uninstall(
    jolter: &Jolter,
    target: UninstallTarget,
    force: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let (name, version, outcome) = match target {
        UninstallTarget::Runtime(kind, version) => {
            let outcome = jolter.uninstall_runtime(kind, &version, force)?;
            (kind.to_string(), version, outcome)
        }
        UninstallTarget::PackageManager(kind, version) => {
            let outcome = jolter.uninstall_package_manager(kind, &version, force)?;
            (kind.to_string(), version, outcome)
        }
    };
    ui.success(format!(
        "Uninstalled {name}@{version} from {} ({})",
        outcome.path.display(),
        human_bytes(outcome.reclaimed_bytes)
    ));
    Ok(ExitCode::SUCCESS)
}

fn run_prune(
    jolter: &Jolter,
    project: &Path,
    keep: usize,
    dry_run: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let outcome = jolter.prune(project, keep, dry_run)?;
    print_prune_outcome(&outcome, ui);
    Ok(ExitCode::SUCCESS)
}

fn run_cache(
    jolter: &Jolter,
    command: CacheCommand,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    match command {
        CacheCommand::Status => {
            let stats = jolter.cache_stats()?;
            ui.info(format!(
                "Cache: {} file(s), {} at {}",
                stats.files,
                human_bytes(stats.bytes),
                jolter.storage().cache_dir().display()
            ));
        }
        CacheCommand::Clean => {
            let outcome = jolter.clean_cache()?;
            ui.success(format!(
                "Removed {} cached file(s), reclaiming {}",
                outcome.removed_files,
                human_bytes(outcome.reclaimed_bytes)
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_setup_ci(
    jolter: &Jolter,
    project: &Path,
    json: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let outcome = jolter.sync(project)?;
    install_shims(jolter)?;
    let provider = configure_ci_environment(jolter, &outcome)?;
    if json {
        ui.finish_progress();
        print_ci_json(jolter, &outcome, provider)?;
    } else {
        ui.info(format!("CI provider: {provider}"));
        print_sync_outcome("Synchronized", &outcome, ui);
        ui.detail(format!("Shims: {}", jolter.storage().shims_dir().display()));
        ui.detail(format!("Cache: {}", jolter.storage().cache_dir().display()));
    }
    Ok(ExitCode::SUCCESS)
}

fn print_inventory(jolter: &Jolter, ui: &TerminalUi) -> Result<(), CliError> {
    let runtimes = jolter.list()?;
    let tools = jolter.list_tools()?;
    if runtimes.is_empty() && tools.is_empty() {
        ui.info("No runtimes or package managers installed.");
        return Ok(());
    }
    if !runtimes.is_empty() {
        ui.heading("Runtimes");
        let mut rows = Vec::with_capacity(runtimes.len());
        for runtime in runtimes {
            let active = jolter.storage().active_version(runtime.kind)?;
            let marker = if active.as_ref() == Some(&runtime.version) {
                '*'
            } else {
                ' '
            };
            rows.push(TableRow::new(
                marker,
                format!("{}@{}", runtime.kind, runtime.version),
                format!("[{}]", installation_status(runtime.is_complete())),
                runtime.path.display().to_string(),
            ));
        }
        ui.table(&rows);
    }
    if !tools.is_empty() {
        ui.heading("Package managers");
        let mut rows = Vec::with_capacity(tools.len());
        for tool in tools {
            let active = jolter.storage().active_tool_version(tool.kind)?;
            let marker = if active.as_ref() == Some(&tool.version) {
                '*'
            } else {
                ' '
            };
            rows.push(TableRow::new(
                marker,
                format!("{}@{}", tool.kind, tool.version),
                format!("[{}]", installation_status(tool.is_complete())),
                tool.path.display().to_string(),
            ));
        }
        ui.table(&rows);
    }
    Ok(())
}

fn print_inventory_json(jolter: &Jolter) -> Result<(), CliError> {
    let runtimes = jolter
        .list()?
        .into_iter()
        .map(|runtime| {
            let active = jolter.storage().active_version(runtime.kind)?;
            Ok(serde_json::json!({
                "kind": runtime.kind.to_string(),
                "version": runtime.version.to_string(),
                "path": runtime.path,
                "ready": runtime.is_complete(),
                "active": active.as_ref() == Some(&runtime.version),
            }))
        })
        .collect::<Result<Vec<_>, jolter_storage::StorageError>>()?;
    let package_managers = jolter
        .list_tools()?
        .into_iter()
        .map(|tool| {
            let active = jolter.storage().active_tool_version(tool.kind)?;
            Ok(serde_json::json!({
                "kind": tool.kind.to_string(),
                "version": tool.version.to_string(),
                "path": tool.path,
                "ready": tool.is_complete(),
                "active": active.as_ref() == Some(&tool.version),
            }))
        })
        .collect::<Result<Vec<_>, jolter_storage::StorageError>>()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "runtimes": runtimes,
            "packageManagers": package_managers,
        }))
        .map_err(CliError::Json)?
    );
    Ok(())
}

fn run_doctor(
    jolter: &Jolter,
    project: &Path,
    json: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let report = jolter.doctor(project)?;
    if json {
        ui.finish_progress();
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
            let message = format!("{}: {}", check.name, check.message);
            match check.status {
                CheckStatus::Pass => ui.success(message),
                CheckStatus::Warning => ui.warning(message),
                CheckStatus::Fail => ui.failure(message),
            }
            if let Some(remediation) = &check.remediation {
                ui.line(format!("       action: {remediation}"));
            }
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

fn print_setup(jolter: &Jolter, shell: SetupShell, ui: &TerminalUi) {
    let shims = jolter.storage().shims_dir();
    ui.success(format!("Installed Jolter shims in {}", shims.display()));
    if path_contains(&shims) {
        ui.info("Jolter shims are already available on PATH in this process.");
        return;
    }

    ui.info("Add the shims directory to PATH:");
    match shell {
        SetupShell::Powershell => {
            let path = powershell_quote(&shims);
            ui.heading("Current PowerShell session");
            ui.line(format!("$env:PATH = '{path};' + $env:PATH"));
            ui.heading("Persist for the current user");
            ui.line(powershell_persist_command(&path));
        }
        SetupShell::Cmd => {
            let display = shims.display();
            let path = powershell_quote(&shims);
            ui.heading("Current Command Prompt session");
            ui.line(format!("set \"PATH={display};%PATH%\""));
            ui.heading("Persist for the current user");
            ui.line(format!(
                "powershell -NoProfile -Command \"{}\"",
                powershell_persist_script(&format!("'{path}'"))
            ));
        }
        SetupShell::Bash | SetupShell::Zsh => {
            let path = posix_double_quote_content(&shims);
            let export = format!("export PATH=\"{path}:$PATH\"");
            let profile = if shell == SetupShell::Zsh {
                "~/.zshrc"
            } else {
                "~/.bashrc"
            };
            ui.heading("Current shell session");
            ui.line(&export);
            ui.heading("Persist for future sessions");
            ui.line(format!(
                "printf '%s\\n' {} >> {profile}",
                posix_quote(&export)
            ));
        }
        SetupShell::Fish => {
            ui.heading("Current and future Fish sessions");
            ui.line(format!(
                "fish_add_path {}",
                posix_quote(&shims.to_string_lossy())
            ));
        }
        SetupShell::Auto => unreachable!("auto shell must be resolved before printing setup"),
    }
    ui.detail("Restart the shell after applying persistent PATH changes.");
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

fn print_sync_outcome(prefix: &str, outcome: &SyncOutcome, ui: &TerminalUi) {
    ui.success(format!(
        "{prefix} {}@{} at {}",
        outcome.runtime.kind,
        outcome.runtime.version,
        outcome.runtime.path.display()
    ));
    if let Some(package_manager) = &outcome.package_manager {
        let verb = if package_manager.downloaded {
            "Installed"
        } else {
            "Selected"
        };
        ui.success(format!(
            "{verb} package manager {}@{} at {}",
            package_manager.tool.kind,
            package_manager.tool.version,
            package_manager.tool.path.display()
        ));
    }
}

fn print_prune_outcome(outcome: &PruneOutcome, ui: &TerminalUi) {
    if outcome.removed.is_empty() {
        ui.info("Nothing to prune.");
        return;
    }
    let verb = if outcome.dry_run {
        "Would remove"
    } else {
        "Removed"
    };
    for item in &outcome.removed {
        ui.line(format!(
            "{verb} {}@{} from {} ({})",
            item.kind,
            item.version,
            item.path.display(),
            human_bytes(item.reclaimed_bytes)
        ));
    }
    ui.success(format!(
        "{} {} installation(s), reclaiming {}{}",
        if outcome.dry_run { "Planned" } else { "Pruned" },
        outcome.removed.len(),
        human_bytes(outcome.reclaimed_bytes()),
        if outcome.dry_run { " if applied" } else { "" }
    ));
}

fn configure_ci_environment(
    jolter: &Jolter,
    outcome: &SyncOutcome,
) -> Result<&'static str, CliError> {
    let provider = detect_ci_provider();
    if provider == "github-actions" {
        append_ci_line(
            "GITHUB_PATH",
            &jolter.storage().shims_dir().to_string_lossy(),
        )?;
        append_ci_line(
            "GITHUB_OUTPUT",
            &format!(
                "runtime={}@{}",
                outcome.runtime.kind, outcome.runtime.version
            ),
        )?;
        if let Some(package_manager) = &outcome.package_manager {
            append_ci_line(
                "GITHUB_OUTPUT",
                &format!(
                    "package_manager={}@{}",
                    package_manager.tool.kind, package_manager.tool.version
                ),
            )?;
        }
        append_ci_line(
            "GITHUB_OUTPUT",
            &format!("cache={}", jolter.storage().cache_dir().display()),
        )?;
    }
    Ok(provider)
}

fn append_ci_line(variable: &'static str, value: &str) -> Result<(), CliError> {
    let Some(path) = env::var_os(variable) else {
        return Ok(());
    };
    let path = PathBuf::from(path);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| CliError::CiEnvironment {
            variable,
            path: path.clone(),
            source,
        })?;
    writeln!(file, "{value}").map_err(|source| CliError::CiEnvironment {
        variable,
        path,
        source,
    })
}

fn detect_ci_provider() -> &'static str {
    if env_flag("GITHUB_ACTIONS") {
        "github-actions"
    } else if env_flag("GITLAB_CI") {
        "gitlab-ci"
    } else if env_flag("CIRCLECI") {
        "circleci"
    } else if env_flag("TF_BUILD") {
        "azure-pipelines"
    } else if env_flag("BUILDKITE") {
        "buildkite"
    } else {
        "generic"
    }
}

fn env_flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| {
        matches!(
            value.to_string_lossy().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

fn print_ci_json(jolter: &Jolter, outcome: &SyncOutcome, provider: &str) -> Result<(), CliError> {
    let package_manager = outcome.package_manager.as_ref().map(|action| {
        serde_json::json!({
            "kind": action.tool.kind.to_string(),
            "version": action.tool.version.to_string(),
            "path": action.tool.path,
        })
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "provider": provider,
            "runtime": {
                "kind": outcome.runtime.kind.to_string(),
                "version": outcome.runtime.version.to_string(),
                "path": outcome.runtime.path,
            },
            "packageManager": package_manager,
            "shims": jolter.storage().shims_dir(),
            "cache": jolter.storage().cache_dir(),
        }))
        .map_err(CliError::Json)?
    );
    Ok(())
}

fn print_completions(shell: CompletionShell) {
    let mut command = Cli::command();
    let name = command.get_name().to_owned();
    match shell {
        CompletionShell::Bash => generate(Bash, &mut command, name, &mut std::io::stdout()),
        CompletionShell::Elvish => generate(Elvish, &mut command, name, &mut std::io::stdout()),
        CompletionShell::Fish => generate(Fish, &mut command, name, &mut std::io::stdout()),
        CompletionShell::Powershell => {
            generate(PowerShell, &mut command, name, &mut std::io::stdout());
        }
        CompletionShell::Zsh => generate(Zsh, &mut command, name, &mut std::io::stdout()),
    }
}

fn parse_use_target(value: &str) -> Result<UseTarget, String> {
    let (name, _) = value
        .rsplit_once('@')
        .ok_or_else(|| "expected <runtime-or-manager>@<version>".to_owned())?;
    match name.to_ascii_lowercase().as_str() {
        "node" | "nodejs" | "bun" | "deno" => value
            .parse()
            .map(UseTarget::Runtime)
            .map_err(|error: jolter_runtime::RuntimeRequestError| error.to_string()),
        "npm" | "pnpm" | "yarn" | "yarnpkg" => value
            .parse()
            .map(UseTarget::PackageManager)
            .map_err(|error: jolter_runtime::PackageManagerRequestError| error.to_string()),
        _ => Err(format!(
            "unsupported tool `{name}`; expected node, bun, deno, npm, pnpm, or yarn"
        )),
    }
}

fn parse_uninstall_target(value: &str) -> Result<UninstallTarget, String> {
    let (name, selector) = value
        .rsplit_once('@')
        .ok_or_else(|| "expected <runtime-or-manager>@<exact-version>".to_owned())?;
    let version = Version::parse(selector.trim_start_matches('v'))
        .map_err(|_| "uninstall requires an exact semantic version".to_owned())?;
    let normalized = name.to_ascii_lowercase();
    match normalized.as_str() {
        "node" | "nodejs" | "bun" | "deno" => name
            .parse()
            .map(|kind| UninstallTarget::Runtime(kind, version))
            .map_err(|error: jolter_runtime::RuntimeRequestError| error.to_string()),
        "npm" | "pnpm" | "yarn" | "yarnpkg" => name
            .parse()
            .map(|kind| UninstallTarget::PackageManager(kind, version))
            .map_err(|error: jolter_runtime::PackageManagerRequestError| error.to_string()),
        _ => Err(format!(
            "unsupported tool `{name}`; expected node, bun, deno, npm, pnpm, or yarn"
        )),
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut divisor = 1_u64;
    let mut unit = 0;
    while bytes / divisor >= 1024 && unit < UNITS.len() - 1 {
        divisor = divisor.saturating_mul(1024);
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        let whole = bytes / divisor;
        let decimal = (bytes % divisor).saturating_mul(10) / divisor;
        format!("{whole}.{decimal} {}", UNITS[unit])
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
    #[error("failed to update {variable} file {path}: {source}")]
    CiEnvironment {
        variable: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
