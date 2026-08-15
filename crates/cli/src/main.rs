mod args;
mod commands;
mod error;
mod output;

#[cfg(test)]
mod tests;

use std::{env, path::Path, process::ExitCode, sync::Arc, time::Instant};

use clap::Parser;
use jolter_core::Jolter;

use crate::{
    args::{Cli, Command, UseTarget},
    commands::{
        install_shims, print_completions, print_inventory, print_inventory_json, print_setup,
        print_sync_outcome, resolve_setup_shell, run_cache, run_doctor, run_install, run_plugin,
        run_prune, run_setup_ci, run_uninstall, run_update, run_upgrade, run_use,
    },
    error::CliError,
    output::{
        ColorPreference, DetailLevel, OutputKind, OutputOptions, ProgressPreference, TerminalUi,
    },
};

fn main() -> ExitCode {
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
    let started = Instant::now();
    let code = match run(cli, &ui) {
        Ok(code) => code,
        Err(error) => {
            ui.failure(error.to_string());
            ExitCode::FAILURE
        }
    };
    ui.timing(started.elapsed());
    code
}

fn run(cli: Cli, ui: &Arc<TerminalUi>) -> Result<ExitCode, CliError> {
    let jolter = Jolter::discover_with_reporter(ui.clone())?;
    run_with_jolter(&jolter, cli, ui)
}

fn run_with_jolter(jolter: &Jolter, cli: Cli, ui: &Arc<TerminalUi>) -> Result<ExitCode, CliError> {
    let current_dir = env::current_dir().map_err(CliError::CurrentDirectory)?;
    run_with_jolter_in_dir(jolter, cli, &current_dir, ui)
}

fn run_with_jolter_in_dir(
    jolter: &Jolter,
    cli: Cli,
    current_dir: &Path,
    ui: &Arc<TerminalUi>,
) -> Result<ExitCode, CliError> {
    match cli.command {
        Command::Setup { shell } => {
            install_shims(jolter)?;
            print_setup(jolter, resolve_setup_shell(shell), ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Install { target } => run_install(jolter, target, ui),
        Command::Use { target } => run_use(jolter, target, ui),
        Command::Pin { target } => {
            let pinned = target
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            for target in target {
                match target {
                    UseTarget::Runtime(request) => jolter.pin_runtime(current_dir, &request)?,
                    UseTarget::Tool(request) => jolter.pin_tool(current_dir, &request)?,
                    UseTarget::PluginTool { name, selector } => {
                        jolter.pin_plugin_tool(current_dir, &name, &selector)?;
                    }
                }
            }
            ui.success(format!(
                "Pinned {pinned} in {}",
                current_dir.join("jolter.json").display()
            ));
            Ok(ExitCode::SUCCESS)
        }
        Command::Update { target, all } => run_update(jolter, target, all, ui),
        Command::List { json } => {
            if json {
                ui.finish_progress();
                print_inventory_json(jolter)?;
            } else {
                print_inventory(jolter, ui)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor { json } => run_doctor(jolter, current_dir, json, ui),
        Command::Sync { yes } => {
            let outcome = jolter.sync_with_plugin_install(current_dir, yes)?;
            install_shims(jolter)?;
            print_sync_outcome("Synchronized", &outcome, ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Repair { yes } => {
            let outcome = jolter.repair_with_plugin_install(current_dir, yes)?;
            install_shims(jolter)?;
            print_sync_outcome("Repaired", &outcome, ui);
            Ok(ExitCode::SUCCESS)
        }
        Command::Plugin { command } => run_plugin(jolter, command, ui),
        Command::Uninstall { target, force } => run_uninstall(jolter, target, force, ui),
        Command::Prune { keep, dry_run } => run_prune(jolter, current_dir, keep, dry_run, ui),
        Command::Cache { command } => run_cache(jolter, command, ui),
        Command::SetupCi { json, yes } => run_setup_ci(jolter, current_dir, json, yes, ui),
        Command::Completions { shell } => {
            ui.finish_progress();
            print_completions(shell);
            Ok(ExitCode::SUCCESS)
        }
        Command::Upgrade {
            nightly,
            latest,
            channel,
            force,
        } => {
            let selected_channel = if nightly {
                jolter_core::ReleaseChannel::Nightly
            } else if latest {
                jolter_core::ReleaseChannel::Stable
            } else {
                channel.into()
            };
            run_upgrade(jolter, selected_channel, force, ui)
        }
    }
}
