use std::{path::Path, process::ExitCode};

use jolter_core::{Jolter, PruneOutcome};

use super::setup::install_shims;
use crate::{
    error::CliError,
    output::{TerminalUi, format_bytes},
};

pub fn run_prune(
    jolter: &Jolter,
    project: &Path,
    keep: usize,
    dry_run: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let outcome = jolter.prune(project, keep, dry_run)?;
    if !dry_run {
        install_shims(jolter)?;
    }
    print_prune_outcome(&outcome, ui);
    Ok(ExitCode::SUCCESS)
}

pub fn print_prune_outcome(outcome: &PruneOutcome, ui: &TerminalUi) {
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
            format_bytes(item.reclaimed_bytes)
        ));
    }
    ui.success(format!(
        "{} {} installation(s), reclaiming {}{}",
        if outcome.dry_run { "Planned" } else { "Pruned" },
        outcome.removed.len(),
        format_bytes(outcome.reclaimed_bytes()),
        if outcome.dry_run { " if applied" } else { "" }
    ));
}
