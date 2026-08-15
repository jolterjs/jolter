use jolter_core::Jolter;

use crate::{
    error::CliError,
    output::{TableRow, TerminalUi},
};

pub const fn installation_status(complete: bool) -> &'static str {
    if complete { "ready" } else { "incomplete" }
}

pub fn print_inventory(jolter: &Jolter, ui: &TerminalUi) -> Result<(), CliError> {
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

pub fn print_inventory_json(jolter: &Jolter) -> Result<(), CliError> {
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

pub fn print_plugins(jolter: &Jolter, ui: &TerminalUi) -> Result<(), CliError> {
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

pub fn print_plugins_json(jolter: &Jolter) -> Result<(), CliError> {
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
