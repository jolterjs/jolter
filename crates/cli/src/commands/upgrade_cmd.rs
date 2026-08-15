use std::process::ExitCode;

use jolter_core::Jolter;

use super::setup::install_shims;
use crate::{error::CliError, output::TerminalUi};

pub fn run_upgrade(
    jolter: &Jolter,
    channel: jolter_core::ReleaseChannel,
    force: bool,
    ui: &TerminalUi,
) -> Result<ExitCode, CliError> {
    let outcome = jolter.upgrade(channel, force)?;
    if outcome.updated {
        ui.success(format!(
            "Upgraded Jolter to {} ({}) at {}",
            outcome.version,
            outcome.channel,
            outcome.executable_path.display()
        ));
        install_shims(jolter)?;
    } else {
        ui.info(format!(
            "Jolter {} is already current ({}) at {}",
            outcome.version,
            outcome.channel,
            outcome.executable_path.display()
        ));
    }
    Ok(ExitCode::SUCCESS)
}
