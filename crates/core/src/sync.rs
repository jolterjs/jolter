use std::{collections::BTreeMap, path::Path};

use jolter_doctor::Report;
use jolter_plugin::PluginRequest;
use jolter_resolver::resolve;
use jolter_runtime::RuntimeKind;

use crate::{
    Jolter,
    error::CoreError,
    types::{PluginAction, PluginToolAction, SyncOutcome, ToolAction},
};

impl Jolter {
    pub fn doctor(&self, project: &Path) -> Result<Report, CoreError> {
        let target = project.display().to_string();
        self.report(jolter_installer::ProgressAction::Diagnose, &target);
        Ok(jolter_doctor::examine(project, &self.storage)?)
    }

    pub fn sync(&self, project: &Path) -> Result<SyncOutcome, CoreError> {
        self.sync_inner(project, false, false)
    }

    pub fn repair(&self, project: &Path) -> Result<SyncOutcome, CoreError> {
        self.sync_inner(project, true, false)
    }

    pub fn sync_with_plugin_install(
        &self,
        project: &Path,
        yes: bool,
    ) -> Result<SyncOutcome, CoreError> {
        self.sync_inner(project, false, yes)
    }

    pub fn repair_with_plugin_install(
        &self,
        project: &Path,
        yes: bool,
    ) -> Result<SyncOutcome, CoreError> {
        self.sync_inner(project, true, yes)
    }

    fn sync_inner(
        &self,
        project: &Path,
        repair: bool,
        yes: bool,
    ) -> Result<SyncOutcome, CoreError> {
        let resolution = resolve(project)?;
        let plugin_actions = self.ensure_project_plugins(&resolution, yes)?;
        let runtime = resolution
            .runtime
            .ok_or_else(|| CoreError::NoRuntimeRequirement(project.to_path_buf()))?;
        let action = self.ensure_runtime(&runtime.request, repair, true)?;
        let tools = resolution
            .tools
            .into_iter()
            .map(|resolved| {
                if action.runtime.kind != RuntimeKind::Node {
                    return Err(CoreError::ToolRequiresNode(resolved.request));
                }
                let tool = self.ensure_tool(&resolved.request, &action.runtime.version, repair)?;
                let target = format!("{}@{}", tool.tool.kind, tool.tool.version);
                self.report(jolter_installer::ProgressAction::Activate, &target);
                self.storage
                    .activate_tool(tool.tool.kind, &tool.tool.version)?;
                Ok(ToolAction {
                    request: resolved.request,
                    tool: tool.tool,
                    downloaded: tool.downloaded,
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()?;

        let project_plugins = resolution
            .plugins
            .iter()
            .map(|plugin| (plugin.name.clone(), plugin.selector.clone()))
            .collect::<BTreeMap<_, _>>();
        let plugin_tools = resolution
            .plugin_tools
            .into_iter()
            .map(|plugin_tool| {
                let provider =
                    self.find_plugin_for_tool(&plugin_tool.name, Some(&project_plugins))?;
                let outcome = self.ensure_plugin_tool(
                    &provider,
                    &plugin_tool.name,
                    &plugin_tool.selector,
                    repair,
                )?;
                Ok(PluginToolAction::from(outcome))
            })
            .collect::<Result<Vec<_>, CoreError>>()?;

        Ok(SyncOutcome {
            runtime: action.runtime,
            downloaded: action.downloaded,
            tools,
            plugins: plugin_actions,
            plugin_tools,
        })
    }

    fn ensure_project_plugins(
        &self,
        resolution: &jolter_resolver::ProjectResolution,
        yes: bool,
    ) -> Result<Vec<PluginAction>, CoreError> {
        let mut actions = Vec::new();
        for requirement in &resolution.plugins {
            let request = PluginRequest::new(&requirement.name, &requirement.selector)?;
            let canonical = if requirement.name.starts_with('@') {
                requirement.name.to_ascii_lowercase()
            } else {
                self.with_plugins(|plugins| Ok(plugins.resolve_name(&requirement.name)?))?
            };
            if let Some(plugin) = self
                .storage
                .find_matching_plugin(&canonical, &requirement.selector)?
            {
                self.report(
                    jolter_installer::ProgressAction::Reuse,
                    &format!("{}@{}", plugin.canonical_name, plugin.version),
                );
                actions.push(PluginAction { plugin });
                continue;
            }
            if !yes {
                return Err(CoreError::MissingProjectPlugin {
                    name: requirement.name.clone(),
                    selector: requirement.selector.clone(),
                });
            }
            actions.push(self.install_plugin(&request)?);
        }
        Ok(actions)
    }
}
