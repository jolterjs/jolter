use std::{collections::BTreeSet, path::Path};

use jolter_installer::{CacheCleanOutcome, ProgressAction, ReleaseChannel, SelfUpgradeOutcome};
use jolter_resolver::resolve;
use jolter_runtime::{RuntimeKind, ToolKind};
use jolter_storage::{CacheStats, InstalledPluginTool, InstalledRuntime, InstalledTool};
use semver::Version;

use crate::{
    Jolter,
    error::CoreError,
    types::{PruneItem, PruneItemKind, PruneOutcome},
};

impl Jolter {
    pub fn prune(
        &self,
        project: &Path,
        keep: usize,
        dry_run: bool,
    ) -> Result<PruneOutcome, CoreError> {
        let resolution = resolve(project)?;
        let runtimes = self.storage.installed_runtimes()?;
        let tools = self.storage.installed_tools()?;
        let plugin_tools = self.storage.installed_plugin_tools()?;
        let protected_runtimes = self.protected_runtimes(&resolution, &runtimes, keep)?;
        let protected_tools = self.protected_tools(&resolution, &tools, keep)?;
        let protected_plugin_tools =
            self.protected_plugin_tools(&resolution, &plugin_tools, keep)?;

        let runtime_removals = runtimes
            .into_iter()
            .filter(|runtime| {
                !protected_runtimes.contains(&(runtime.kind, runtime.version.clone()))
            })
            .map(|runtime| PruneItem {
                kind: PruneItemKind::Runtime(runtime.kind),
                version: runtime.version,
                path: runtime.path,
                reclaimed_bytes: 0,
            });
        let tool_removals = tools
            .into_iter()
            .filter(|tool| !protected_tools.contains(&(tool.kind, tool.version.clone())))
            .map(|tool| PruneItem {
                kind: PruneItemKind::Tool(tool.kind),
                version: tool.version,
                path: tool.path,
                reclaimed_bytes: 0,
            });
        let plugin_tool_removals = plugin_tools
            .into_iter()
            .filter(|tool| {
                !protected_plugin_tools.contains(&(
                    tool.provider.clone(),
                    tool.tool.clone(),
                    tool.version.clone(),
                ))
            })
            .map(|tool| PruneItem {
                kind: PruneItemKind::PluginTool {
                    provider: tool.provider,
                    tool: tool.tool,
                },
                version: tool.version,
                path: tool.path,
                reclaimed_bytes: 0,
            });
        let mut removed = runtime_removals
            .chain(tool_removals)
            .chain(plugin_tool_removals)
            .collect::<Vec<_>>();

        for item in &mut removed {
            if dry_run {
                item.reclaimed_bytes = self.storage.path_stats(&item.path)?.bytes;
                continue;
            }
            let target = format!("{}@{}", item.kind, item.version);
            self.report(ProgressAction::Remove, &target);
            let outcome = match &item.kind {
                PruneItemKind::Runtime(kind) => self.with_installer(|installer| {
                    Ok(installer.uninstall_runtime(*kind, &item.version)?)
                })?,
                PruneItemKind::Tool(kind) => self.with_installer(|installer| {
                    Ok(installer.uninstall_tool(*kind, &item.version)?)
                })?,
                PruneItemKind::PluginTool { provider, tool } => {
                    self.with_installer(|installer| {
                        Ok(installer.uninstall_plugin_tool(provider, tool, &item.version)?)
                    })?
                }
            };
            item.reclaimed_bytes = outcome.reclaimed_bytes;
        }

        Ok(PruneOutcome { removed, dry_run })
    }

    fn protected_runtimes(
        &self,
        resolution: &jolter_resolver::ProjectResolution,
        runtimes: &[InstalledRuntime],
        keep: usize,
    ) -> Result<BTreeSet<(RuntimeKind, Version)>, CoreError> {
        let mut protected = BTreeSet::new();
        for kind in RuntimeKind::ALL {
            if let Some(version) = self.storage.active_version(kind)? {
                protected.insert((kind, version));
            }
            protected.extend(
                runtimes
                    .iter()
                    .rev()
                    .filter(|runtime| runtime.kind == kind && runtime.is_complete())
                    .take(keep)
                    .map(|runtime| (runtime.kind, runtime.version.clone())),
            );
        }
        if let Some(runtime) = &resolution.runtime
            && let Some(installed) = self.storage.find_matching(&runtime.request)?
        {
            protected.insert((installed.kind, installed.version));
        }
        Ok(protected)
    }

    fn protected_tools(
        &self,
        resolution: &jolter_resolver::ProjectResolution,
        tools: &[InstalledTool],
        keep: usize,
    ) -> Result<BTreeSet<(ToolKind, Version)>, CoreError> {
        let mut protected = BTreeSet::new();
        for kind in ToolKind::ALL {
            if let Some(version) = self.storage.active_tool_version(kind)? {
                protected.insert((kind, version));
            }
            protected.extend(
                tools
                    .iter()
                    .rev()
                    .filter(|tool| tool.kind == kind && tool.is_complete())
                    .take(keep)
                    .map(|tool| (tool.kind, tool.version.clone())),
            );
        }
        for resolved in &resolution.tools {
            if let Some(installed) = self.storage.find_matching_tool(&resolved.request)? {
                protected.insert((installed.kind, installed.version));
            }
        }
        Ok(protected)
    }

    fn protected_plugin_tools(
        &self,
        resolution: &jolter_resolver::ProjectResolution,
        plugin_tools: &[InstalledPluginTool],
        keep: usize,
    ) -> Result<BTreeSet<(String, String, Version)>, CoreError> {
        let mut protected = BTreeSet::new();
        for resolved in &resolution.plugin_tools {
            let provider = self.find_plugin_for_tool(&resolved.name, None)?;
            if let Some(installed) = self.storage.find_matching_plugin_tool(
                &provider.canonical_name,
                &resolved.name,
                &resolved.selector,
            )? {
                protected.insert((installed.provider, installed.tool, installed.version));
            }
        }
        protected.extend(
            self.storage
                .active_plugin_tools()?
                .into_iter()
                .map(|tool| (tool.provider, tool.tool, tool.version)),
        );
        for installed in plugin_tools {
            protected.extend(
                plugin_tools
                    .iter()
                    .rev()
                    .filter(|candidate| {
                        candidate.provider == installed.provider
                            && candidate.tool == installed.tool
                            && candidate.is_complete()
                    })
                    .take(keep)
                    .map(|candidate| {
                        (
                            candidate.provider.clone(),
                            candidate.tool.clone(),
                            candidate.version.clone(),
                        )
                    }),
            );
        }
        Ok(protected)
    }

    pub fn cache_stats(&self) -> Result<CacheStats, CoreError> {
        Ok(self.storage.cache_stats()?)
    }

    pub fn clean_cache(&self) -> Result<CacheCleanOutcome, CoreError> {
        let target = self.storage.cache_dir().display().to_string();
        self.report(ProgressAction::Clean, &target);
        self.with_installer(|installer| Ok(installer.clean_cache()?))
    }

    pub fn upgrade(
        &self,
        channel: ReleaseChannel,
        force: bool,
    ) -> Result<SelfUpgradeOutcome, CoreError> {
        let label = format!("jolter ({channel})");
        self.report(ProgressAction::Resolve, &label);
        let outcome =
            self.with_installer(|installer| Ok(installer.upgrade_self(channel, force)?))?;
        if outcome.updated {
            let _ = self.install_shims(&outcome.executable_path);
        }
        Ok(outcome)
    }

    pub fn install_shims(&self, executable: &Path) -> Result<Vec<std::path::PathBuf>, CoreError> {
        let target = self.storage.shims_dir().display().to_string();
        self.report(ProgressAction::Shims, &target);
        Ok(jolter_shim::install_shims(executable, &self.storage)?)
    }
}
