use std::{path::Path, process::ExitCode};

use jolter_core::Jolter;
use jolter_doctor::CheckStatus;

use crate::{error::CliError, output::TerminalUi};

pub fn run_doctor(
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
