mod output;

use std::{
    env,
    fs::OpenOptions,
    io::{IsTerminal, Write},
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
use jolter_plugin::PluginRequest;
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
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
    /// Install and activate a runtime or tool.
    Use {
        /// Tool request, for example node@24 or pnpm@10.
        #[arg(value_parser = parse_use_target, num_args = 0..)]
        target: Vec<UseTarget>,
    },
    /// Write a runtime or tool requirement to jolter.json.
    Pin {
        /// Runtime or tool request, for example node@24 or pnpm@10.
        #[arg(value_parser = parse_use_target, num_args = 1..)]
        target: Vec<UseTarget>,
    },
    /// Update an active runtime or tool.
    #[command(visible_alias = "up")]
    Update {
        /// Runtime or tool name, optionally with a selector.
        #[arg(
            value_parser = parse_update_target,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        target: Option<UpdateTarget>,
        /// Update every active runtime and tool within its current major line.
        #[arg(long)]
        all: bool,
    },
    /// List locally installed runtimes and tools.
    #[command(visible_alias = "ls")]
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
    Repair {
        /// Install missing project plugins without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Synchronize the local toolchain with project requirements.
    Sync {
        /// Install missing project plugins without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Install, update, list, or remove Jolter plugins.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// Remove one exact runtime or tool version.
    #[command(visible_aliases = ["remove", "rm"])]
    Uninstall {
        /// Exact version to remove, for example node@24.1.0 or pnpm@10.2.0.
        #[arg(value_parser = parse_uninstall_target)]
        target: UninstallTarget,
        /// Permit removal when the runtime or tool is globally active.
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
        /// Install missing project plugins without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Generate shell completion scripts.
    #[command(visible_aliases = ["c", "comp"])]
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

#[derive(Debug, Clone, Subcommand)]
enum PluginCommand {
    /// Install a plugin globally.
    #[command(visible_aliases = ["add", "i"])]
    Install {
        /// Plugin request, for example eslint, eslint@1, or @eslint/eslint@1.
        #[arg(value_parser = parse_plugin_request)]
        target: PluginRequest,
    },
    /// List installed plugins.
    #[command(visible_alias = "ls")]
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Update installed plugins.
    #[command(visible_aliases = ["up"])]
    Update {
        /// Plugin name or alias to update.
        target: Option<String>,
        /// Update every installed plugin.
        #[arg(long)]
        all: bool,
    },
    /// Remove an installed plugin.
    #[command(visible_aliases = ["remove", "rm"])]
    Uninstall {
        /// Plugin name or alias.
        name: String,
        /// Permit removal when plugin commands are shimmed.
        #[arg(long)]
        force: bool,
    },
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
    Tool(ToolKind, Version),
    PluginTool { name: String, version: Version },
}

#[derive(Debug, Clone)]
enum UseTarget {
    Runtime(RuntimeRequest),
    Tool(ToolRequest),
    PluginTool { name: String, selector: String },
}

#[derive(Debug, Clone)]
enum UpdateTarget {
    Runtime(RuntimeKind, Option<RuntimeRequest>),
    Tool(ToolKind, Option<ToolRequest>),
    PluginTool {
        name: String,
        selector: Option<String>,
    },
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
    fn machine_output(&self) -> bool {
        matches!(
            &self.command,
            Command::List { json: true }
                | Command::Doctor { json: true }
                | Command::SetupCi { json: true, .. }
                | Command::Plugin {
                    command: PluginCommand::List { json: true },
                }
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
        Command::Pin { target } => {
            let pinned = target
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            for target in target {
                match target {
                    UseTarget::Runtime(request) => jolter.pin_runtime(&current_dir, &request)?,
                    UseTarget::Tool(request) => jolter.pin_tool(&current_dir, &request)?,
                    UseTarget::PluginTool { name, selector } => {
                        jolter.pin_plugin_tool(&current_dir, &name, &selector)?;
                    }
                }
            }
            ui.success(format!(
                "Pinned {pinned} in {}",
                current_dir.join("jolter.json").display()
            ));
            Ok(ExitCode::SUCCESS)
        }
        Command::Update { target, all } => run_update(&jolter, target, all, ui),
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
        Command::Sync { yes } => {
            let outcome = jolter.sync_with_plugin_install(&current_dir, yes)?;
            install_shims(&jolter)?;
            print_sync_outcome("Synchronized", &outcome, ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Repair { yes } => {
            let outcome = jolter.repair_with_plugin_install(&current_dir, yes)?;
            install_shims(&jolter)?;
            print_sync_outcome("Repaired", &outcome, ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Plugin { command } => run_plugin(&jolter, command, ui),
        Command::Uninstall { target, force } => run_uninstall(&jolter, target, force, ui),
        Command::Prune { keep, dry_run } => run_prune(&jolter, &current_dir, keep, dry_run, ui),
        Command::Cache { command } => run_cache(&jolter, command, ui),
        Command::SetupCi { json, yes } => run_setup_ci(&jolter, &current_dir, json, yes, ui),
        Command::Completions { shell } => {
            ui.finish_progress();
            print_completions(shell);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_use(
    jolter: &Jolter,
    targets: Vec<UseTarget>,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let targets = if targets.is_empty() {
        vec![interactive_use_target(jolter, ui)?]
    } else {
        targets
    };
    for target in targets
        .iter()
        .filter(|target| matches!(target, UseTarget::Runtime(_)))
        .chain(
            targets
                .iter()
                .filter(|target| !matches!(target, UseTarget::Runtime(_))),
        )
    {
        match target.clone() {
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
            UseTarget::Tool(request) => {
                let action = jolter.use_tool(&request)?;
                install_shims(jolter)?;
                let verb = if action.downloaded {
                    "Installed and activated"
                } else {
                    "Activated"
                };
                ui.success(format!(
                    "{verb} tool {}@{} at {}",
                    action.tool.kind,
                    action.tool.version,
                    action.tool.path.display()
                ));
            }
            UseTarget::PluginTool { name, selector } => {
                let action = jolter.use_plugin_tool(&name, &selector)?;
                install_shims(jolter)?;
                let verb = if action.downloaded {
                    "Installed and activated"
                } else {
                    "Activated"
                };
                ui.success(format!(
                    "{verb} plugin tool {}@{} via {} at {}",
                    action.tool.tool,
                    action.tool.version,
                    action.provider,
                    action.tool.path.display()
                ));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn interactive_use_target(jolter: &Jolter, ui: &TerminalUi) -> Result<UseTarget, CliError> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(CliError::InteractiveUseRequiresTty);
    }
    ui.finish_progress();
    let mut choices = vec![
        "node@latest".to_owned(),
        "bun@latest".to_owned(),
        "deno@latest".to_owned(),
        "npm@latest".to_owned(),
        "pnpm@latest".to_owned(),
        "yarn@latest".to_owned(),
    ];
    for plugin_tool in jolter.list_plugin_tools()? {
        choices.push(format!("{}@latest", plugin_tool.tool));
    }
    choices.sort();
    choices.dedup();
    let selected = inquire::Select::new("Use runtime or tool", choices)
        .with_help_message("Type to filter, then press Enter")
        .prompt()
        .map_err(|error| CliError::InteractivePrompt(error.to_string()))?;
    parse_use_target(&selected).map_err(CliError::InteractiveSelection)
}

fn run_plugin(
    jolter: &Jolter,
    command: PluginCommand,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    match command {
        PluginCommand::Install { target } => {
            let action = jolter.install_plugin(&target)?;
            install_shims(jolter)?;
            ui.success(format!(
                "Installed plugin {}@{} at {}",
                action.plugin.canonical_name,
                action.plugin.version,
                action.plugin.path.display()
            ));
        }
        PluginCommand::List { json } => {
            if json {
                ui.finish_progress();
                print_plugins_json(jolter)?;
            } else {
                print_plugins(jolter, ui)?;
            }
        }
        PluginCommand::Update { target, all } => {
            if all {
                let mut names = jolter
                    .list_plugins()?
                    .into_iter()
                    .map(|plugin| plugin.canonical_name)
                    .collect::<Vec<_>>();
                names.sort();
                names.dedup();
                if names.is_empty() {
                    ui.info("No plugins installed.");
                    return Ok(ExitCode::SUCCESS);
                }
                for name in names {
                    let action = jolter.update_plugin(&name)?;
                    ui.success(format!(
                        "Updated plugin {}@{}",
                        action.plugin.canonical_name, action.plugin.version
                    ));
                }
                install_shims(jolter)?;
            } else {
                let target = target.ok_or(CliError::PluginUpdateTargetRequired)?;
                let action = jolter.update_plugin(&target)?;
                install_shims(jolter)?;
                ui.success(format!(
                    "Updated plugin {}@{}",
                    action.plugin.canonical_name, action.plugin.version
                ));
            }
        }
        PluginCommand::Uninstall { name, force } => {
            let outcome = jolter.uninstall_plugin(&name, force)?;
            install_shims(jolter)?;
            ui.success(format!(
                "Uninstalled plugin from {} ({})",
                outcome.path.display(),
                human_bytes(outcome.reclaimed_bytes)
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_update(
    jolter: &Jolter,
    target: Option<UpdateTarget>,
    all: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let targets = if all {
        active_update_targets(jolter)?
    } else {
        target.into_iter().collect()
    };
    if targets.is_empty() {
        ui.info("No active runtimes or tools to update.");
        return Ok(ExitCode::SUCCESS);
    }

    for target in targets {
        match target {
            UpdateTarget::Runtime(kind, request) => {
                let previous = jolter.storage().active_version(kind)?;
                let request = update_runtime_request(kind, request, previous.as_ref())?;
                let action = jolter.update_runtime(&request)?;
                print_update_result(
                    &kind.to_string(),
                    previous.as_ref(),
                    &action.runtime.version,
                    &action.runtime.path,
                    ui,
                );
            }
            UpdateTarget::Tool(kind, request) => {
                let previous = jolter.storage().active_tool_version(kind)?;
                let request = update_tool_request(kind, request, previous.as_ref())?;
                let action = jolter.update_tool(&request)?;
                print_update_result(
                    &kind.to_string(),
                    previous.as_ref(),
                    &action.tool.version,
                    &action.tool.path,
                    ui,
                );
            }
            UpdateTarget::PluginTool { name, selector } => {
                let previous = jolter.storage().active_plugin_tool(&name)?;
                let action = jolter.update_plugin_tool(&name, selector.as_deref())?;
                print_update_result(
                    &name,
                    previous.as_ref().map(|tool| &tool.version),
                    &action.tool.version,
                    &action.tool.path,
                    ui,
                );
            }
        }
    }
    install_shims(jolter)?;
    Ok(ExitCode::SUCCESS)
}

fn active_update_targets(jolter: &Jolter) -> Result<Vec<UpdateTarget>, CliError> {
    let mut targets = Vec::new();
    for kind in RuntimeKind::ALL {
        if jolter.storage().active_version(kind)?.is_some() {
            targets.push(UpdateTarget::Runtime(kind, None));
        }
    }
    for kind in ToolKind::ALL {
        if jolter.storage().active_tool_version(kind)?.is_some() {
            targets.push(UpdateTarget::Tool(kind, None));
        }
    }
    for tool in jolter.storage().active_plugin_tools()? {
        targets.push(UpdateTarget::PluginTool {
            name: tool.tool,
            selector: None,
        });
    }
    Ok(targets)
}

fn update_runtime_request(
    kind: RuntimeKind,
    request: Option<RuntimeRequest>,
    active: Option<&Version>,
) -> Result<RuntimeRequest, CliError> {
    request.map_or_else(
        || {
            let active = active.ok_or_else(|| CliError::NoActiveUpdateTarget(kind.to_string()))?;
            RuntimeRequest::new(kind, active.major.to_string())
                .map_err(|error| CliError::UpdateRequest(error.to_string()))
        },
        Ok,
    )
}

fn update_tool_request(
    kind: ToolKind,
    request: Option<ToolRequest>,
    active: Option<&Version>,
) -> Result<ToolRequest, CliError> {
    request.map_or_else(
        || {
            let active = active.ok_or_else(|| CliError::NoActiveUpdateTarget(kind.to_string()))?;
            ToolRequest::new(kind, active.major.to_string())
                .map_err(|error| CliError::UpdateRequest(error.to_string()))
        },
        Ok,
    )
}

fn print_update_result(
    name: &str,
    previous: Option<&Version>,
    current: &Version,
    path: &Path,
    ui: &TerminalUi,
) {
    match previous {
        Some(previous) if previous == current => {
            ui.info(format!(
                "{name}@{current} is already current at {}",
                path.display()
            ));
        }
        Some(previous) => {
            ui.success(format!(
                "Updated {name} from {previous} to {current} at {}",
                path.display()
            ));
        }
        None => {
            ui.success(format!(
                "Installed and activated {name}@{current} at {}",
                path.display()
            ));
        }
    }
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
        UninstallTarget::Tool(kind, version) => {
            let outcome = jolter.uninstall_tool(kind, &version, force)?;
            (kind.to_string(), version, outcome)
        }
        UninstallTarget::PluginTool { name, version } => {
            let matches = jolter
                .list_plugin_tools()?
                .into_iter()
                .filter(|tool| tool.tool == name && tool.version == version)
                .collect::<Vec<_>>();
            let tool = match matches.as_slice() {
                [tool] => tool,
                [] => return Err(CliError::PluginToolUninstallTargetMissing(name, version)),
                _ => return Err(CliError::PluginToolUninstallTargetAmbiguous(name, version)),
            };
            let provider = tool.provider.clone();
            let outcome = jolter.uninstall_plugin_tool(&provider, &name, &version, force)?;
            (format!("{name} via {provider}"), version, outcome)
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
    yes: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let outcome = jolter.sync_with_plugin_install(project, yes)?;
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
    let plugin_tools = jolter.list_plugin_tools()?;
    let plugins = jolter.list_plugins()?;
    if runtimes.is_empty() && tools.is_empty() && plugin_tools.is_empty() && plugins.is_empty() {
        ui.info("No runtimes or tools installed.");
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
        ui.heading("Tools");
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
    if !plugin_tools.is_empty() {
        ui.heading("Plugin Tools");
        let mut rows = Vec::with_capacity(plugin_tools.len());
        for tool in plugin_tools {
            let active = jolter.storage().active_plugin_tool(&tool.tool)?;
            let marker = if active.as_ref().is_some_and(|active| {
                active.provider == tool.provider && active.version == tool.version
            }) {
                '*'
            } else {
                ' '
            };
            rows.push(TableRow::new(
                marker,
                format!("{}@{}", tool.tool, tool.version),
                format!("[{}]", installation_status(tool.is_complete())),
                format!("{} via {}", tool.path.display(), tool.provider),
            ));
        }
        ui.table(&rows);
    }
    if !plugins.is_empty() {
        ui.heading("Plugins");
        let rows = plugins
            .into_iter()
            .map(|plugin| {
                TableRow::new(
                    ' ',
                    format!("{}@{}", plugin.canonical_name, plugin.version),
                    if plugin.path.join(".jolter-plugin.json").is_file() {
                        "[ready]"
                    } else {
                        "[incomplete]"
                    },
                    plugin.path.display().to_string(),
                )
            })
            .collect::<Vec<_>>();
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
    let tools = jolter
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
    let plugin_tools = jolter
        .list_plugin_tools()?
        .into_iter()
        .map(|tool| {
            let active = jolter.storage().active_plugin_tool(&tool.tool)?;
            Ok(serde_json::json!({
                "kind": "plugin-tool",
                "name": tool.tool,
                "provider": tool.provider,
                "version": tool.version.to_string(),
                "path": tool.path,
                "ready": tool.is_complete(),
                "active": active.as_ref().is_some_and(|active| {
                    active.provider == tool.provider && active.version == tool.version
                }),
            }))
        })
        .collect::<Result<Vec<_>, jolter_storage::StorageError>>()?;
    let plugins = jolter
        .list_plugins()?
        .into_iter()
        .map(|plugin| {
            serde_json::json!({
                "name": plugin.canonical_name,
                "version": plugin.version.to_string(),
                "path": plugin.path,
                "ready": plugin.path.join(".jolter-plugin.json").is_file(),
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "runtimes": runtimes,
            "tools": tools,
            "pluginTools": plugin_tools,
            "plugins": plugins,
        }))
        .map_err(CliError::Json)?
    );
    Ok(())
}

fn print_plugins(jolter: &Jolter, ui: &TerminalUi) -> Result<(), CliError> {
    let plugins = jolter.list_plugins()?;
    if plugins.is_empty() {
        ui.info("No plugins installed.");
        return Ok(());
    }
    ui.heading("Plugins");
    let rows = plugins
        .into_iter()
        .map(|plugin| {
            TableRow::new(
                ' ',
                format!("{}@{}", plugin.canonical_name, plugin.version),
                if plugin.path.join(".jolter-plugin.json").is_file() {
                    "[ready]"
                } else {
                    "[incomplete]"
                },
                plugin.path.display().to_string(),
            )
        })
        .collect::<Vec<_>>();
    ui.table(&rows);
    Ok(())
}

fn print_plugins_json(jolter: &Jolter) -> Result<(), CliError> {
    let plugins = jolter
        .list_plugins()?
        .into_iter()
        .map(|plugin| {
            serde_json::json!({
                "name": plugin.canonical_name,
                "version": plugin.version.to_string(),
                "path": plugin.path,
                "ready": plugin.path.join(".jolter-plugin.json").is_file(),
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "plugins": plugins }))
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
    for tool in &outcome.tools {
        let verb = if tool.downloaded {
            "Installed"
        } else {
            "Selected"
        };
        ui.success(format!(
            "{verb} tool {}@{} at {}",
            tool.tool.kind,
            tool.tool.version,
            tool.tool.path.display()
        ));
    }
    for tool in &outcome.plugin_tools {
        let verb = if tool.downloaded {
            "Installed"
        } else {
            "Selected"
        };
        ui.success(format!(
            "{verb} plugin tool {}@{} via {} at {}",
            tool.tool.tool,
            tool.tool.version,
            tool.provider,
            tool.tool.path.display()
        ));
    }
    for plugin in &outcome.plugins {
        ui.success(format!(
            "Selected plugin {}@{} at {}",
            plugin.plugin.canonical_name,
            plugin.plugin.version,
            plugin.plugin.path.display()
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
        if !outcome.tools.is_empty() {
            let tools = outcome
                .tools
                .iter()
                .map(|action| format!("{}@{}", action.tool.kind, action.tool.version))
                .collect::<Vec<_>>()
                .join(",");
            append_ci_line("GITHUB_OUTPUT", &format!("tools={tools}"))?;
        }
        if !outcome.plugins.is_empty() {
            let plugins = outcome
                .plugins
                .iter()
                .map(|action| format!("{}@{}", action.plugin.canonical_name, action.plugin.version))
                .collect::<Vec<_>>()
                .join(",");
            append_ci_line("GITHUB_OUTPUT", &format!("plugins={plugins}"))?;
        }
        if !outcome.plugin_tools.is_empty() {
            let plugin_tools = outcome
                .plugin_tools
                .iter()
                .map(|action| format!("{}@{}", action.tool.tool, action.tool.version))
                .collect::<Vec<_>>()
                .join(",");
            append_ci_line("GITHUB_OUTPUT", &format!("plugin_tools={plugin_tools}"))?;
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
    let tools = outcome
        .tools
        .iter()
        .map(|action| {
            serde_json::json!({
                "kind": action.tool.kind.to_string(),
                "version": action.tool.version.to_string(),
                "path": action.tool.path,
            })
        })
        .collect::<Vec<_>>();
    let plugins = outcome
        .plugins
        .iter()
        .map(|action| {
            serde_json::json!({
                "name": action.plugin.canonical_name,
                "version": action.plugin.version.to_string(),
                "path": action.plugin.path,
            })
        })
        .collect::<Vec<_>>();
    let plugin_tools = outcome
        .plugin_tools
        .iter()
        .map(|action| {
            serde_json::json!({
                "name": action.tool.tool,
                "provider": action.provider,
                "version": action.tool.version.to_string(),
                "path": action.tool.path,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "provider": provider,
            "runtime": {
                "kind": outcome.runtime.kind.to_string(),
                "version": outcome.runtime.version.to_string(),
                "path": outcome.runtime.path,
            },
            "tools": tools,
            "pluginTools": plugin_tools,
            "plugins": plugins,
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
    let (name, selector) = value
        .rsplit_once('@')
        .ok_or_else(|| "expected <runtime-or-tool>@<version>".to_owned())?;
    match name.to_ascii_lowercase().as_str() {
        "node" | "nodejs" | "bun" | "deno" => value
            .parse()
            .map(UseTarget::Runtime)
            .map_err(|error: jolter_runtime::RuntimeRequestError| error.to_string()),
        "npm" | "pnpm" | "yarn" | "yarnpkg" => value
            .parse()
            .map(UseTarget::Tool)
            .map_err(|error: jolter_runtime::ToolRequestError| error.to_string()),
        _ => {
            validate_plugin_tool_target(name, selector)?;
            Ok(UseTarget::PluginTool {
                name: name.to_owned(),
                selector: selector.to_owned(),
            })
        }
    }
}

fn validate_plugin_tool_target(name: &str, selector: &str) -> Result<(), String> {
    let valid_name = !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        });
    let valid_selector = selector.eq_ignore_ascii_case("latest")
        || selector.split('.').all(|part| {
            !part.is_empty()
                && (part == "*"
                    || part.eq_ignore_ascii_case("x")
                    || part.chars().all(|character| character.is_ascii_digit()))
        });
    if valid_name && valid_selector {
        Ok(())
    } else {
        Err(format!(
            "unsupported tool `{name}`; expected a built-in tool or a lowercase plugin-provided tool"
        ))
    }
}

fn parse_plugin_request(value: &str) -> Result<PluginRequest, String> {
    value
        .parse()
        .map_err(|error: jolter_plugin::PluginError| error.to_string())
}

impl std::fmt::Display for UseTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(request) => request.fmt(formatter),
            Self::Tool(request) => request.fmt(formatter),
            Self::PluginTool { name, selector } => write!(formatter, "{name}@{selector}"),
        }
    }
}

fn parse_update_target(value: &str) -> Result<UpdateTarget, String> {
    if value.contains('@') {
        return match parse_use_target(value)? {
            UseTarget::Runtime(request) => Ok(UpdateTarget::Runtime(request.kind, Some(request))),
            UseTarget::Tool(request) => Ok(UpdateTarget::Tool(request.kind, Some(request))),
            UseTarget::PluginTool { name, selector } => Ok(UpdateTarget::PluginTool {
                name,
                selector: Some(selector),
            }),
        };
    }
    match value.to_ascii_lowercase().as_str() {
        "node" | "nodejs" | "bun" | "deno" => value
            .parse()
            .map(|kind| UpdateTarget::Runtime(kind, None))
            .map_err(|error: jolter_runtime::RuntimeRequestError| error.to_string()),
        "npm" | "pnpm" | "yarn" | "yarnpkg" => value
            .parse()
            .map(|kind| UpdateTarget::Tool(kind, None))
            .map_err(|error: jolter_runtime::ToolRequestError| error.to_string()),
        _ => {
            validate_plugin_tool_target(value, "latest")?;
            Ok(UpdateTarget::PluginTool {
                name: value.to_owned(),
                selector: None,
            })
        }
    }
}

fn parse_uninstall_target(value: &str) -> Result<UninstallTarget, String> {
    let (name, selector) = value
        .rsplit_once('@')
        .ok_or_else(|| "expected <runtime-or-tool>@<exact-version>".to_owned())?;
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
            .map(|kind| UninstallTarget::Tool(kind, version))
            .map_err(|error: jolter_runtime::ToolRequestError| error.to_string()),
        _ => {
            validate_plugin_tool_target(name, selector)?;
            Ok(UninstallTarget::PluginTool {
                name: name.to_owned(),
                version,
            })
        }
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
    (name != "jolter" && (target_for_command(&name).is_some() || !name.is_empty())).then_some(name)
}

fn run_shim(shim: &str) -> Result<ExitCode, CliError> {
    let storage = Storage::discover()?;
    let current_dir = env::current_dir().map_err(CliError::CurrentDirectory)?;
    let resolved = resolve_command(shim, &current_dir, &storage)?;
    let mut command = runtime_command(&resolved.executable);
    command.args(&resolved.arguments);
    command.args(env::args_os().skip(1));
    if let (Some(runtime), Some(runtime_root)) = (&resolved.runtime, &resolved.runtime_root) {
        prepend_runtime_path(&mut command, runtime.kind, runtime_root)?;
    }

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
    #[error("no active {0} version; pass an explicit selector such as {0}@latest")]
    NoActiveUpdateTarget(String),
    #[error("pass a plugin name or use `jolter plugin update --all`")]
    PluginUpdateTargetRequired,
    #[error("`jolter use` without a target requires an interactive terminal")]
    InteractiveUseRequiresTty,
    #[error("interactive selection failed: {0}")]
    InteractivePrompt(String),
    #[error("interactive selection produced an invalid target: {0}")]
    InteractiveSelection(String),
    #[error("plugin tool `{0}@{1}` is not installed")]
    PluginToolUninstallTargetMissing(String, Version),
    #[error(
        "plugin tool `{0}@{1}` is installed from multiple providers; remove by pruning or uninstall one provider first"
    )]
    PluginToolUninstallTargetAmbiguous(String, Version),
    #[error("invalid update request: {0}")]
    UpdateRequest(String),
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
