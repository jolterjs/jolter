use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

use jolter_core::{Jolter, SyncOutcome};

use super::setup::install_shims;
use crate::{error::CliError, output::TerminalUi};

pub fn run_setup_ci(
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

pub fn configure_ci_environment(
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

pub fn append_ci_line(variable: &'static str, value: &str) -> Result<(), CliError> {
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

pub fn detect_ci_provider() -> &'static str {
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

pub fn print_ci_json(
    jolter: &Jolter,
    outcome: &SyncOutcome,
    provider: &str,
) -> Result<(), CliError> {
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

pub fn print_sync_outcome(prefix: &str, outcome: &SyncOutcome, ui: &TerminalUi) {
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
