use std::process::ExitCode;

use jolter_core::Jolter;

use crate::{
    args::CacheCommand,
    error::CliError,
    output::{TerminalUi, format_bytes},
};

pub fn run_cache(
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
                format_bytes(stats.bytes),
                jolter.storage().cache_dir().display()
            ));
        }
        CacheCommand::Clean => {
            let outcome = jolter.clean_cache()?;
            ui.success(format!(
                "Removed {} cached file(s), reclaiming {}",
                outcome.removed_files,
                format_bytes(outcome.reclaimed_bytes)
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}
