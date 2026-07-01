use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use jolter_config::{CONFIG_FILE_NAME, ProjectConfig, RuntimeConfig};
use jolter_doctor::Report;
use jolter_installer::{
    ArchiveFormat, Artifact, ArtifactIntegrity, CacheCleanOutcome, InstallOutcome, Installer,
    NoProgressReporter, PluginToolArchive, PluginToolInstallOutcome, RemovalOutcome,
    ToolInstallOutcome,
};
use jolter_plugin::{
    PluginExecutor, PluginManager, PluginPlatform, PluginRequest, PluginToolRelease,
    read_installed_manifest,
};
use jolter_resolver::resolve;
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use jolter_storage::{
    CacheStats, InstalledPlugin, InstalledPluginTool, InstalledRuntime, InstalledTool, Storage,
};
use semver::Version;
use thiserror::Error;

pub use jolter_installer::{ProgressAction, ProgressEvent, ProgressReporter};

pub struct Jolter {
    storage: Storage,
    installer: Installer,
    plugins: PluginManager,
    plugin_executor: PluginExecutor,
    reporter: Arc<dyn ProgressReporter>,
}

impl Jolter {
    pub fn discover() -> Result<Self, CoreError> {
        Self::with_storage(Storage::discover()?)
    }

    pub fn discover_with_reporter(reporter: Arc<dyn ProgressReporter>) -> Result<Self, CoreError> {
        Self::with_storage_and_reporter(Storage::discover()?, reporter)
    }

    pub fn with_storage(storage: Storage) -> Result<Self, CoreError> {
        Self::with_storage_and_reporter(storage, Arc::new(NoProgressReporter))
    }

    pub fn with_storage_and_reporter(
        storage: Storage,
        reporter: Arc<dyn ProgressReporter>,
    ) -> Result<Self, CoreError> {
        storage.ensure_layout()?;
        let installer = Installer::new_with_reporter(storage.clone(), reporter.clone())?;
        let plugins = PluginManager::new(storage.clone())?;
        let plugin_executor = PluginExecutor::new()?;
        Ok(Self {
            storage,
            installer,
            plugins,
            plugin_executor,
            reporter,
        })
    }

    #[must_use]
    pub const fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn pin_runtime(&self, project: &Path, request: &RuntimeRequest) -> Result<(), CoreError> {
        let path = project.join(CONFIG_FILE_NAME);
        let target = path.display().to_string();
        self.report(ProgressAction::Configure, &target);
        let mut config = if path.is_file() {
            ProjectConfig::from_path(&path)?
        } else {
            ProjectConfig::default()
        };
        config.runtime = RuntimeConfig::default();
        match request.kind {
            RuntimeKind::Node => config.runtime.node = Some(request.selector.clone()),
            RuntimeKind::Bun => config.runtime.bun = Some(request.selector.clone()),
            RuntimeKind::Deno => config.runtime.deno = Some(request.selector.clone()),
        }
        config.write_to(&path)?;
        Ok(())
    }

    pub fn pin_tool(&self, project: &Path, request: &ToolRequest) -> Result<(), CoreError> {
        let path = project.join(CONFIG_FILE_NAME);
        let target = path.display().to_string();
        self.report(ProgressAction::Configure, &target);
        let mut config = if path.is_file() {
            ProjectConfig::from_path(&path)?
        } else {
            ProjectConfig::default()
        };
        let descriptor = request.to_string();
        let selector = descriptor
            .split_once('@')
            .map_or(request.selector.as_str(), |(_, selector)| selector);
        config
            .tools
            .insert(request.kind.to_string(), selector.to_owned());
        config.write_to(&path)?;
        Ok(())
    }

    pub fn pin_plugin_tool(
        &self,
        project: &Path,
        name: &str,
        selector: &str,
    ) -> Result<(), CoreError> {
        let path = project.join(CONFIG_FILE_NAME);
        let target = path.display().to_string();
        self.report(ProgressAction::Configure, &target);
        let mut config = if path.is_file() {
            ProjectConfig::from_path(&path)?
        } else {
            ProjectConfig::default()
        };
        config.schema_version = jolter_config::CURRENT_SCHEMA_VERSION;
        config.tools.insert(name.to_owned(), selector.to_owned());
        let provider = self.find_plugin_for_tool(name, Some(&config.plugins))?;
        config
            .plugins
            .insert(provider.canonical_name, provider.version.to_string());
        config.write_to(&path)?;
        Ok(())
    }

    pub fn pin_plugin(&self, project: &Path, name: &str, selector: &str) -> Result<(), CoreError> {
        let path = project.join(CONFIG_FILE_NAME);
        let target = path.display().to_string();
        self.report(ProgressAction::Configure, &target);
        let mut config = if path.is_file() {
            ProjectConfig::from_path(&path)?
        } else {
            ProjectConfig::default()
        };
        config.schema_version = jolter_config::CURRENT_SCHEMA_VERSION;
        config.plugins.insert(name.to_owned(), selector.to_owned());
        config.write_to(&path)?;
        Ok(())
    }

    pub fn use_runtime(&self, request: &RuntimeRequest) -> Result<RuntimeAction, CoreError> {
        let target = request.to_string();
        self.report(ProgressAction::Select, &target);
        self.ensure_runtime(request, false)
    }

    pub fn use_tool(&self, request: &ToolRequest) -> Result<ToolAction, CoreError> {
        let requested = request.to_string();
        self.report(ProgressAction::Select, &requested);
        let node_version = self.active_node_version(request)?;
        let outcome = self.ensure_tool(request, &node_version, false)?;
        let target = format!("{}@{}", outcome.tool.kind, outcome.tool.version);
        self.report(ProgressAction::Activate, &target);
        self.storage
            .activate_tool(outcome.tool.kind, &outcome.tool.version)?;
        Ok(ToolAction {
            request: request.clone(),
            tool: outcome.tool,
            downloaded: outcome.downloaded,
        })
    }

    pub fn use_plugin_tool(
        &self,
        name: &str,
        selector: &str,
    ) -> Result<PluginToolAction, CoreError> {
        let requested = format!("{name}@{selector}");
        self.report(ProgressAction::Select, &requested);
        let provider = self.find_plugin_for_tool(name, None)?;
        let outcome = self.ensure_plugin_tool(&provider, name, selector, false)?;
        let target = format!(
            "{}@{} via {}@{}",
            outcome.tool, outcome.version, outcome.provider, provider.version
        );
        self.report(ProgressAction::Activate, &target);
        self.storage
            .activate_plugin_tool(&outcome.provider, &outcome.tool, &outcome.version)?;
        Ok(PluginToolAction::from(outcome))
    }

    pub fn update_runtime(&self, request: &RuntimeRequest) -> Result<RuntimeAction, CoreError> {
        let requested = request.to_string();
        self.report(ProgressAction::Resolve, &requested);
        if Version::parse(request.selector.trim_start_matches('v')).is_ok()
            && let Some(runtime) = self.storage.find_matching(request)?
        {
            let target = format!("{}@{}", runtime.kind, runtime.version);
            self.report(ProgressAction::Reuse, &target);
            self.report(ProgressAction::Activate, &target);
            self.storage.activate(runtime.kind, &runtime.version)?;
            return Ok(RuntimeAction {
                runtime,
                downloaded: false,
            });
        }
        let outcome = self.installer.install(request)?;
        let target = format!("{}@{}", outcome.runtime.kind, outcome.runtime.version);
        self.report(ProgressAction::Activate, &target);
        self.storage
            .activate(outcome.runtime.kind, &outcome.runtime.version)?;
        Ok(RuntimeAction::from(outcome))
    }

    pub fn update_tool(&self, request: &ToolRequest) -> Result<ToolAction, CoreError> {
        let requested = request.to_string();
        self.report(ProgressAction::Resolve, &requested);
        let node_version = self.active_node_version(request)?;
        if request.hash.is_none()
            && Version::parse(request.selector.trim_start_matches('v')).is_ok()
            && let Some(tool) = self.storage.find_matching_tool(request)?
        {
            self.installer
                .validate_installed_tool(&tool, &node_version)?;
            let target = format!("{}@{}", tool.kind, tool.version);
            self.report(ProgressAction::Reuse, &target);
            self.report(ProgressAction::Activate, &target);
            self.storage.activate_tool(tool.kind, &tool.version)?;
            return Ok(ToolAction {
                request: request.clone(),
                tool,
                downloaded: false,
            });
        }
        let outcome = self.installer.install_tool(request, &node_version)?;
        let target = format!("{}@{}", outcome.tool.kind, outcome.tool.version);
        self.report(ProgressAction::Activate, &target);
        self.storage
            .activate_tool(outcome.tool.kind, &outcome.tool.version)?;
        Ok(ToolAction {
            request: request.clone(),
            tool: outcome.tool,
            downloaded: outcome.downloaded,
        })
    }

    pub fn update_plugin_tool(
        &self,
        name: &str,
        selector: Option<&str>,
    ) -> Result<PluginToolAction, CoreError> {
        let active = self
            .storage
            .active_plugin_tool(name)?
            .ok_or_else(|| CoreError::NoActivePluginTool(name.to_owned()))?;
        let provider = self
            .storage
            .installed_plugins()?
            .into_iter()
            .find(|plugin| plugin.canonical_name == active.provider)
            .ok_or_else(|| CoreError::PluginNotInstalled(active.provider.clone()))?;
        let selector = selector.map_or_else(|| active.version.major.to_string(), ToOwned::to_owned);
        let outcome = self.ensure_plugin_tool(&provider, name, &selector, false)?;
        let target = format!(
            "{}@{} via {}@{}",
            outcome.tool, outcome.version, outcome.provider, provider.version
        );
        self.report(ProgressAction::Activate, &target);
        self.storage
            .activate_plugin_tool(&outcome.provider, &outcome.tool, &outcome.version)?;
        Ok(PluginToolAction::from(outcome))
    }

    pub fn list(&self) -> Result<Vec<InstalledRuntime>, CoreError> {
        Ok(self.storage.installed_runtimes()?)
    }

    pub fn list_tools(&self) -> Result<Vec<InstalledTool>, CoreError> {
        Ok(self.storage.installed_tools()?)
    }

    pub fn list_plugin_tools(&self) -> Result<Vec<InstalledPluginTool>, CoreError> {
        Ok(self.storage.installed_plugin_tools()?)
    }

    pub fn list_plugins(&self) -> Result<Vec<InstalledPlugin>, CoreError> {
        Ok(self.storage.installed_plugins()?)
    }

    pub fn doctor(&self, project: &Path) -> Result<Report, CoreError> {
        let target = project.display().to_string();
        self.report(ProgressAction::Diagnose, &target);
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

    pub fn install_plugin(&self, request: &PluginRequest) -> Result<PluginAction, CoreError> {
        let target = request.to_string();
        self.report(ProgressAction::Resolve, &target);
        let plugin = self.plugins.install(request)?;
        Ok(PluginAction { plugin })
    }

    pub fn update_plugin(&self, name: &str) -> Result<PluginAction, CoreError> {
        self.install_plugin(&PluginRequest::new(name, "latest")?)
    }

    pub fn uninstall_plugin(&self, name: &str, force: bool) -> Result<RemovalOutcome, CoreError> {
        let canonical = if name.starts_with('@') {
            name.to_ascii_lowercase()
        } else {
            self.plugins.resolve_name(name)?
        };
        if !force && self.plugin_commands_in_active_shims(&canonical)? {
            return Err(CoreError::ActivePluginRemoval(canonical));
        }
        let path = self.storage.plugin_dir(&canonical);
        let reclaimed_bytes = self.storage.path_stats(&path)?.bytes;
        if !path.exists() {
            return Err(CoreError::PluginNotInstalled(canonical));
        }
        std::fs::remove_dir_all(&path).map_err(|source| CoreError::PluginRemoval {
            path: path.clone(),
            source,
        })?;
        Ok(RemovalOutcome {
            path,
            reclaimed_bytes,
        })
    }

    pub fn uninstall_runtime(
        &self,
        kind: RuntimeKind,
        version: &Version,
        force: bool,
    ) -> Result<RemovalOutcome, CoreError> {
        let target = format!("{kind}@{version}");
        self.report(ProgressAction::Remove, &target);
        let active = self.storage.active_version(kind)?;
        if active.as_ref() == Some(version) && !force {
            return Err(CoreError::ActiveRuntimeRemoval {
                kind,
                version: version.clone(),
            });
        }
        let outcome = self.installer.uninstall_runtime(kind, version)?;
        if active.as_ref() == Some(version) {
            self.storage.deactivate(kind, Some(version))?;
        }
        Ok(outcome)
    }

    pub fn uninstall_tool(
        &self,
        kind: ToolKind,
        version: &Version,
        force: bool,
    ) -> Result<RemovalOutcome, CoreError> {
        let target = format!("{kind}@{version}");
        self.report(ProgressAction::Remove, &target);
        let active = self.storage.active_tool_version(kind)?;
        if active.as_ref() == Some(version) && !force {
            return Err(CoreError::ActiveToolRemoval {
                kind,
                version: version.clone(),
            });
        }
        let outcome = self.installer.uninstall_tool(kind, version)?;
        if active.as_ref() == Some(version) {
            self.storage.deactivate_tool(kind, Some(version))?;
        }
        Ok(outcome)
    }

    pub fn uninstall_plugin_tool(
        &self,
        provider: &str,
        tool: &str,
        version: &Version,
        force: bool,
    ) -> Result<RemovalOutcome, CoreError> {
        let target = format!("{tool}@{version} via {provider}");
        self.report(ProgressAction::Remove, &target);
        let active = self.storage.active_plugin_tool(tool)?;
        if active
            .as_ref()
            .is_some_and(|active| active.provider == provider && active.version == *version)
            && !force
        {
            return Err(CoreError::ActivePluginToolRemoval {
                provider: provider.to_owned(),
                tool: tool.to_owned(),
                version: version.clone(),
            });
        }
        let outcome = self
            .installer
            .uninstall_plugin_tool(provider, tool, version)?;
        if active
            .as_ref()
            .is_some_and(|active| active.provider == provider && active.version == *version)
        {
            self.storage.deactivate_plugin_tool(tool, Some(version))?;
        }
        Ok(outcome)
    }

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
                PruneItemKind::Runtime(kind) => {
                    self.installer.uninstall_runtime(*kind, &item.version)?
                }
                PruneItemKind::Tool(kind) => self.installer.uninstall_tool(*kind, &item.version)?,
                PruneItemKind::PluginTool { provider, tool } => self
                    .installer
                    .uninstall_plugin_tool(provider, tool, &item.version)?,
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
        Ok(self.installer.clean_cache()?)
    }

    pub fn install_shims(&self, executable: &Path) -> Result<Vec<PathBuf>, CoreError> {
        let target = self.storage.shims_dir().display().to_string();
        self.report(ProgressAction::Shims, &target);
        Ok(jolter_shim::install_shims(executable, &self.storage)?)
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
        let action = self.ensure_runtime(&runtime.request, repair)?;
        let tools = resolution
            .tools
            .into_iter()
            .map(|resolved| {
                if action.runtime.kind != RuntimeKind::Node {
                    return Err(CoreError::ToolRequiresNode(resolved.request));
                }
                let tool = self.ensure_tool(&resolved.request, &action.runtime.version, repair)?;
                let target = format!("{}@{}", tool.tool.kind, tool.tool.version);
                self.report(ProgressAction::Activate, &target);
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
                self.plugins.resolve_name(&requirement.name)?
            };
            if let Some(plugin) = self
                .storage
                .find_matching_plugin(&canonical, &requirement.selector)?
            {
                self.report(
                    ProgressAction::Reuse,
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

    fn find_plugin_for_tool(
        &self,
        tool: &str,
        project_plugins: Option<&BTreeMap<String, String>>,
    ) -> Result<InstalledPlugin, CoreError> {
        let mut providers = Vec::new();
        for plugin in self.storage.installed_plugins()? {
            let manifest = read_installed_manifest(&plugin.path)?;
            if manifest.provides.tools.contains_key(tool) {
                providers.push(plugin);
            }
        }
        if providers.is_empty() {
            return Err(CoreError::PluginToolProviderMissing(tool.to_owned()));
        }
        if let Some(project_plugins) = project_plugins {
            let mut configured = Vec::new();
            for provider in providers.iter().cloned() {
                if project_plugins.contains_key(&provider.canonical_name) {
                    configured.push(provider);
                    continue;
                }
                for name in project_plugins.keys() {
                    if name.starts_with('@') {
                        continue;
                    }
                    if self.plugins.resolve_name(name).ok().as_deref()
                        == Some(provider.canonical_name.as_str())
                    {
                        configured.push(provider.clone());
                        break;
                    }
                }
            }
            if configured.len() == 1 {
                return configured
                    .pop()
                    .ok_or_else(|| CoreError::PluginToolProviderMissing(tool.to_owned()));
            }
        }
        if providers.len() == 1 {
            return providers
                .pop()
                .ok_or_else(|| CoreError::PluginToolProviderMissing(tool.to_owned()));
        }
        Err(CoreError::AmbiguousPluginToolProvider {
            tool: tool.to_owned(),
            providers: providers
                .into_iter()
                .map(|provider| provider.canonical_name)
                .collect::<Vec<_>>()
                .join(", "),
        })
    }

    fn plugin_commands_in_active_shims(&self, canonical: &str) -> Result<bool, CoreError> {
        Ok(self
            .storage
            .installed_plugins()?
            .into_iter()
            .filter(|plugin| plugin.canonical_name == canonical)
            .any(|plugin| {
                read_installed_manifest(&plugin.path)
                    .is_ok_and(|manifest| !manifest.commands.is_empty())
            }))
    }

    fn ensure_runtime(
        &self,
        request: &RuntimeRequest,
        repair: bool,
    ) -> Result<RuntimeAction, CoreError> {
        if !request.requires_release_metadata() {
            if let Some(runtime) = self.storage.find_matching(request)? {
                let target = format!("{}@{}", runtime.kind, runtime.version);
                self.report(ProgressAction::Reuse, &target);
                self.report(ProgressAction::Activate, &target);
                self.storage.activate(runtime.kind, &runtime.version)?;
                return Ok(RuntimeAction {
                    runtime,
                    downloaded: false,
                });
            }
        }

        let outcome = if repair {
            self.installer.repair(request)?
        } else {
            self.installer.install(request)?
        };
        let target = format!("{}@{}", outcome.runtime.kind, outcome.runtime.version);
        self.report(ProgressAction::Activate, &target);
        self.storage
            .activate(outcome.runtime.kind, &outcome.runtime.version)?;
        Ok(RuntimeAction::from(outcome))
    }

    fn ensure_tool(
        &self,
        request: &ToolRequest,
        node_version: &semver::Version,
        repair: bool,
    ) -> Result<ToolInstallOutcome, CoreError> {
        if !request.selector.eq_ignore_ascii_case("latest") && request.hash.is_none() {
            if let Some(tool) = self.storage.find_matching_tool(request)? {
                let target = format!("{}@{}", tool.kind, tool.version);
                self.report(ProgressAction::Reuse, &target);
                self.installer
                    .validate_installed_tool(&tool, node_version)?;
                return Ok(ToolInstallOutcome {
                    tool,
                    downloaded: false,
                });
            }
        }
        if repair {
            Ok(self.installer.repair_tool(request, node_version)?)
        } else {
            Ok(self.installer.install_tool(request, node_version)?)
        }
    }

    fn ensure_plugin_tool(
        &self,
        provider: &InstalledPlugin,
        tool: &str,
        selector: &str,
        repair: bool,
    ) -> Result<PluginToolInstallOutcome, CoreError> {
        if !selector.eq_ignore_ascii_case("latest")
            && let Some(installed) =
                self.storage
                    .find_matching_plugin_tool(&provider.canonical_name, tool, selector)?
            && self.validate_plugin_tool(provider, &installed)?
        {
            let target = format!(
                "{}@{} via {}@{}",
                installed.tool, installed.version, provider.canonical_name, provider.version
            );
            self.report(ProgressAction::Reuse, &target);
            return Ok(PluginToolInstallOutcome {
                provider: installed.provider,
                tool: installed.tool,
                version: installed.version,
                path: installed.path,
                commands: installed.commands,
                downloaded: false,
            });
        }

        let release = self.resolve_plugin_tool(provider, tool, selector)?;
        let archive = plugin_tool_archive(provider, tool, release)?;
        let outcome = self.installer.install_plugin_tool(&archive, repair)?;
        let installed = InstalledPluginTool {
            provider: outcome.provider.clone(),
            tool: outcome.tool.clone(),
            version: outcome.version.clone(),
            path: outcome.path.clone(),
            commands: outcome.commands.clone(),
        };
        if !self.validate_plugin_tool(provider, &installed)? {
            return Err(CoreError::InvalidPluginToolInstallation {
                provider: provider.canonical_name.clone(),
                tool: tool.to_owned(),
                version: installed.version,
                path: installed.path,
            });
        }
        Ok(outcome)
    }

    fn resolve_plugin_tool(
        &self,
        provider: &InstalledPlugin,
        tool: &str,
        selector: &str,
    ) -> Result<PluginToolRelease, CoreError> {
        let target = format!(
            "{}@{} via {}@{}",
            tool, selector, provider.canonical_name, provider.version
        );
        self.report(ProgressAction::Resolve, &target);
        Ok(self.plugin_executor.resolve_tool(
            &provider.path.join("plugin.wasm"),
            tool,
            selector,
            current_plugin_platform(),
        )?)
    }

    fn validate_plugin_tool(
        &self,
        provider: &InstalledPlugin,
        tool: &InstalledPluginTool,
    ) -> Result<bool, CoreError> {
        Ok(self.plugin_executor.validate_installed(
            &provider.path.join("plugin.wasm"),
            &tool.tool,
            &tool.version,
            &tool.path,
        )?)
    }

    fn active_node_version(&self, request: &ToolRequest) -> Result<Version, CoreError> {
        let version = self
            .storage
            .active_version(RuntimeKind::Node)?
            .ok_or_else(|| CoreError::ToolRequiresActiveNode(request.clone()))?;
        let path = self
            .storage
            .runtime_version_dir(RuntimeKind::Node, &version);
        if !self
            .storage
            .runtime_executable(RuntimeKind::Node, &version)
            .is_file()
        {
            return Err(CoreError::ActiveNodeRuntimeMissing { version, path });
        }
        Ok(version)
    }

    fn report(&self, action: ProgressAction, target: &str) {
        self.reporter
            .report(ProgressEvent::Stage { action, target });
    }
}

fn plugin_tool_archive(
    provider: &InstalledPlugin,
    tool: &str,
    release: PluginToolRelease,
) -> Result<PluginToolArchive, CoreError> {
    let format = match release.archive_format.as_str() {
        "zip" => ArchiveFormat::Zip,
        "tar.gz" | "tgz" => ArchiveFormat::TarGz,
        value => return Err(CoreError::UnsupportedPluginToolArchive(value.to_owned())),
    };
    let file_name = release
        .url
        .split(['/', '?', '#'])
        .rfind(|part| !part.is_empty())
        .unwrap_or("plugin-tool-archive")
        .to_owned();
    Ok(PluginToolArchive {
        provider: provider.canonical_name.clone(),
        tool: tool.to_owned(),
        version: release.version,
        artifact: Artifact {
            url: release.url,
            integrity: ArtifactIntegrity::Sha256(release.sha256),
            file_name,
            format,
            strip_components: release.strip_components,
        },
        commands: release.commands,
    })
}

fn current_plugin_platform() -> PluginPlatform {
    PluginPlatform {
        os: match std::env::consts::OS {
            "macos" => "darwin",
            other => other,
        }
        .to_owned(),
        arch: match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            other => other,
        }
        .to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAction {
    pub runtime: InstalledRuntime,
    pub downloaded: bool,
}

impl From<InstallOutcome> for RuntimeAction {
    fn from(value: InstallOutcome) -> Self {
        Self {
            runtime: value.runtime,
            downloaded: value.downloaded,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    pub runtime: InstalledRuntime,
    pub downloaded: bool,
    pub tools: Vec<ToolAction>,
    pub plugins: Vec<PluginAction>,
    pub plugin_tools: Vec<PluginToolAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAction {
    pub request: ToolRequest,
    pub tool: InstalledTool,
    pub downloaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAction {
    pub plugin: InstalledPlugin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginToolAction {
    pub provider: String,
    pub tool: InstalledPluginTool,
    pub downloaded: bool,
}

impl From<PluginToolInstallOutcome> for PluginToolAction {
    fn from(value: PluginToolInstallOutcome) -> Self {
        Self {
            provider: value.provider.clone(),
            tool: InstalledPluginTool {
                provider: value.provider,
                tool: value.tool,
                version: value.version,
                path: value.path,
                commands: value.commands,
            },
            downloaded: value.downloaded,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruneItemKind {
    Runtime(RuntimeKind),
    Tool(ToolKind),
    PluginTool { provider: String, tool: String },
}

impl std::fmt::Display for PruneItemKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(kind) => kind.fmt(formatter),
            Self::Tool(kind) => kind.fmt(formatter),
            Self::PluginTool { provider, tool } => write!(formatter, "{tool} via {provider}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneItem {
    pub kind: PruneItemKind,
    pub version: Version,
    pub path: PathBuf,
    pub reclaimed_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneOutcome {
    pub removed: Vec<PruneItem>,
    pub dry_run: bool,
}

impl PruneOutcome {
    #[must_use]
    pub fn reclaimed_bytes(&self) -> u64 {
        self.removed.iter().map(|item| item.reclaimed_bytes).sum()
    }
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("no runtime requirement was found from {0}")]
    NoRuntimeRequirement(PathBuf),
    #[error("tool {0} requires a Node.js runtime")]
    ToolRequiresNode(ToolRequest),
    #[error("tool {0} requires an active Node.js runtime; run `jolter use node@<version>` first")]
    ToolRequiresActiveNode(ToolRequest),
    #[error("active node@{version} runtime is missing from {path}")]
    ActiveNodeRuntimeMissing { version: Version, path: PathBuf },
    #[error(
        "refusing to uninstall active {kind}@{version}; activate another version or pass --force"
    )]
    ActiveRuntimeRemoval { kind: RuntimeKind, version: Version },
    #[error(
        "refusing to uninstall active {kind}@{version}; activate another version or pass --force"
    )]
    ActiveToolRemoval { kind: ToolKind, version: Version },
    #[error(
        "refusing to uninstall active plugin tool {tool}@{version} via {provider}; activate another version or pass --force"
    )]
    ActivePluginToolRemoval {
        provider: String,
        tool: String,
        version: Version,
    },
    #[error("no active plugin tool `{0}`; pass an explicit selector such as {0}@latest")]
    NoActivePluginTool(String),
    #[error("project requires plugin {name}@{selector}; rerun with `--yes` to install it")]
    MissingProjectPlugin { name: String, selector: String },
    #[error("no installed plugin provides tool `{0}`")]
    PluginToolProviderMissing(String),
    #[error(
        "multiple installed plugins provide tool `{tool}` ({providers}); declare one provider in jolter.json"
    )]
    AmbiguousPluginToolProvider { tool: String, providers: String },
    #[error("plugin tool archive format `{0}` is not supported")]
    UnsupportedPluginToolArchive(String),
    #[error("plugin `{provider}` reported invalid installation for `{tool}@{version}` at {path}")]
    InvalidPluginToolInstallation {
        provider: String,
        tool: String,
        version: Version,
        path: PathBuf,
    },
    #[error(
        "direct use of plugin-provided tool `{0}` is not available yet; add it to jolter.json and run `jolter sync --yes`"
    )]
    DirectPluginToolUseUnsupported(String),
    #[error("plugin `{0}` is installed with active shim commands; pass --force to remove it")]
    ActivePluginRemoval(String),
    #[error("plugin `{0}` is not installed")]
    PluginNotInstalled(String),
    #[error("failed to remove plugin at {path}: {source}")]
    PluginRemoval {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Config(#[from] jolter_config::ConfigError),
    #[error(transparent)]
    Doctor(#[from] jolter_doctor::DoctorError),
    #[error(transparent)]
    Installer(#[from] jolter_installer::InstallerError),
    #[error(transparent)]
    Plugin(#[from] jolter_plugin::PluginError),
    #[error(transparent)]
    Resolver(#[from] jolter_resolver::ResolverError),
    #[error(transparent)]
    Shim(#[from] jolter_shim::ShimError),
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs};

    #[test]
    fn pin_runtime_preserves_tool_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let mut tools = BTreeMap::new();
        tools.insert("pnpm".to_owned(), "10.x".to_owned());
        ProjectConfig {
            schema_url: Some(
                jolter_config::schema_url_for_version(jolter_config::CURRENT_SCHEMA_VERSION)
                    .to_owned(),
            ),
            schema_version: jolter_config::CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig::default(),
            tools,
            plugins: BTreeMap::new(),
        }
        .write_to(&temp.path().join(CONFIG_FILE_NAME))
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let jolter = Jolter::with_storage(Storage::new(storage_temp.path())).unwrap();

        jolter
            .pin_runtime(temp.path(), &"node@24".parse().unwrap())
            .unwrap();

        let config = ProjectConfig::from_path(&temp.path().join(CONFIG_FILE_NAME)).unwrap();
        assert_eq!(config.runtime.node.as_deref(), Some("24"));
        assert_eq!(config.tools.get("pnpm").map(String::as_str), Some("10.x"));
    }

    #[test]
    fn pin_tool_preserves_runtime_and_other_tools() {
        let temp = tempfile::tempdir().unwrap();
        ProjectConfig {
            schema_url: Some(
                jolter_config::schema_url_for_version(jolter_config::CURRENT_SCHEMA_VERSION)
                    .to_owned(),
            ),
            schema_version: jolter_config::CURRENT_SCHEMA_VERSION,
            runtime: RuntimeConfig {
                node: Some("24".to_owned()),
                bun: None,
                deno: None,
            },
            tools: BTreeMap::from([("pnpm".to_owned(), "10".to_owned())]),
            plugins: BTreeMap::new(),
        }
        .write_to(&temp.path().join(CONFIG_FILE_NAME))
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let jolter = Jolter::with_storage(Storage::new(storage_temp.path())).unwrap();

        jolter
            .pin_tool(temp.path(), &"yarn@4".parse().unwrap())
            .unwrap();

        let config = ProjectConfig::from_path(&temp.path().join(CONFIG_FILE_NAME)).unwrap();
        assert_eq!(config.runtime.node.as_deref(), Some("24"));
        assert_eq!(config.tools.get("pnpm").map(String::as_str), Some("10"));
        assert_eq!(config.tools.get("yarn").map(String::as_str), Some("4"));
    }

    #[test]
    fn pin_plugin_tool_records_tool_and_exact_provider() {
        let project = tempfile::tempdir().unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        storage.ensure_layout().unwrap();
        let provider_version = Version::new(1, 2, 3);
        let plugin = storage.plugin_version_dir("@jolter/jolter", &provider_version);
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            plugin.join(".jolter-plugin.json"),
            r#"{
                "canonicalName":"@jolter/jolter",
                "requestedName":"jolter",
                "version":"1.2.3",
                "registryUrl":"https://registry.jolter.dev",
                "wasmSha256":"0000000000000000000000000000000000000000000000000000000000000000",
                "commands":["jolter"],
                "provides":{"tools":{"jolter":{"commands":["jolter"]}}}
            }"#,
        )
        .unwrap();
        let jolter = Jolter::with_storage(storage).unwrap();

        jolter
            .pin_plugin_tool(project.path(), "jolter", "latest")
            .unwrap();

        let config = ProjectConfig::from_path(&project.path().join(CONFIG_FILE_NAME)).unwrap();
        assert_eq!(config.schema_version, jolter_config::CURRENT_SCHEMA_VERSION);
        assert_eq!(
            config.tools.get("jolter").map(String::as_str),
            Some("latest")
        );
        assert_eq!(
            config.plugins.get("@jolter/jolter").map(String::as_str),
            Some("1.2.3")
        );
    }

    #[test]
    fn sync_uses_an_existing_matching_runtime_without_network() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"24"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        let version = semver::Version::new(24, 1, 0);
        let executable = storage.runtime_executable(RuntimeKind::Node, &version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"node").unwrap();
        let jolter = Jolter::with_storage(storage).unwrap();

        let outcome = jolter.sync(project.path()).unwrap();

        assert_eq!(outcome.runtime.version, version);
        assert!(!outcome.downloaded);
    }

    #[test]
    fn sync_uses_an_existing_matching_tool_without_network() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        let runtime_version = semver::Version::new(24, 1, 0);
        let executable = storage.runtime_executable(RuntimeKind::Node, &runtime_version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"node").unwrap();
        let tool_version = semver::Version::new(10, 2, 0);
        let entrypoint = storage
            .tool_entrypoint(jolter_runtime::ToolKind::Pnpm, &tool_version, "pnpm")
            .unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"pnpm").unwrap();
        let jolter = Jolter::with_storage(storage.clone()).unwrap();

        let outcome = jolter.sync(project.path()).unwrap();

        assert_eq!(outcome.tools[0].tool.version, tool_version);
        assert_eq!(
            storage.active_tool_version(ToolKind::Pnpm).unwrap(),
            Some(tool_version)
        );
    }

    #[test]
    fn sync_activates_multiple_configured_tools() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"24"},"tools":{"pnpm":"10","yarn":"4"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        storage.ensure_layout().unwrap();
        let node_version = Version::new(24, 1, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(node, b"node").unwrap();
        for (kind, version, command) in [
            (ToolKind::Pnpm, Version::new(10, 2, 0), "pnpm"),
            (ToolKind::Yarn, Version::new(4, 1, 0), "yarn"),
        ] {
            let entrypoint = storage.tool_entrypoint(kind, &version, command).unwrap();
            fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
            fs::write(entrypoint, b"tool").unwrap();
        }
        let jolter = Jolter::with_storage(storage.clone()).unwrap();

        let outcome = jolter.sync(project.path()).unwrap();

        assert_eq!(outcome.tools.len(), 2);
        assert_eq!(
            storage.active_tool_version(ToolKind::Pnpm).unwrap(),
            Some(Version::new(10, 2, 0))
        );
        assert_eq!(
            storage.active_tool_version(ToolKind::Yarn).unwrap(),
            Some(Version::new(4, 1, 0))
        );
    }

    #[test]
    fn use_reuses_and_activates_a_tool_with_active_node() {
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        let node_version = semver::Version::new(24, 1, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(node, b"node").unwrap();
        storage.activate(RuntimeKind::Node, &node_version).unwrap();
        let tool_version = semver::Version::new(10, 2, 0);
        let entrypoint = storage
            .tool_entrypoint(ToolKind::Pnpm, &tool_version, "pnpm")
            .unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"pnpm").unwrap();
        let jolter = Jolter::with_storage(storage.clone()).unwrap();

        let action = jolter.use_tool(&"pnpm@10".parse().unwrap()).unwrap();

        assert_eq!(action.tool.version, tool_version);
        assert!(!action.downloaded);
        assert_eq!(
            storage.active_tool_version(ToolKind::Pnpm).unwrap(),
            Some(tool_version)
        );
    }

    #[test]
    fn use_tool_requires_an_active_node_runtime() {
        let storage_temp = tempfile::tempdir().unwrap();
        let jolter = Jolter::with_storage(Storage::new(storage_temp.path())).unwrap();

        let error = jolter.use_tool(&"pnpm@10".parse().unwrap()).unwrap_err();

        assert!(matches!(error, CoreError::ToolRequiresActiveNode(_)));
    }

    #[test]
    fn sync_rejects_an_existing_tool_incompatible_with_node() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"20"},"packageManager":{"pnpm":"11"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        let runtime_version = semver::Version::new(20, 19, 0);
        let executable = storage.runtime_executable(RuntimeKind::Node, &runtime_version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"node").unwrap();
        let tool_version = semver::Version::new(11, 6, 0);
        let entrypoint = storage
            .tool_entrypoint(jolter_runtime::ToolKind::Pnpm, &tool_version, "pnpm")
            .unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"pnpm").unwrap();
        fs::write(
            storage
                .tool_version_dir(jolter_runtime::ToolKind::Pnpm, &tool_version)
                .join("package.json"),
            r#"{"engines":{"node":">=22.13"}}"#,
        )
        .unwrap();
        let jolter = Jolter::with_storage(storage).unwrap();

        let error = jolter.sync(project.path()).unwrap_err();

        assert!(matches!(
            error,
            CoreError::Installer(jolter_installer::InstallerError::IncompatibleNodeVersion { .. })
        ));
    }

    #[test]
    fn prune_preserves_active_and_project_versions() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"24"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        storage.ensure_layout().unwrap();
        for version in [
            semver::Version::new(20, 1, 0),
            semver::Version::new(22, 1, 0),
            semver::Version::new(24, 1, 0),
        ] {
            let executable = storage.runtime_executable(RuntimeKind::Node, &version);
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(executable, b"node").unwrap();
        }
        storage
            .activate(RuntimeKind::Node, &semver::Version::new(22, 1, 0))
            .unwrap();
        let jolter = Jolter::with_storage(storage.clone()).unwrap();

        let preview = jolter.prune(project.path(), 0, true).unwrap();
        assert_eq!(preview.removed.len(), 1);
        assert_eq!(preview.removed[0].version, semver::Version::new(20, 1, 0));
        assert!(preview.reclaimed_bytes() > 0);

        let applied = jolter.prune(project.path(), 0, false).unwrap();
        assert_eq!(applied.removed.len(), 1);
        assert!(
            !storage
                .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(20, 1, 0))
                .exists()
        );
        assert!(
            storage
                .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(22, 1, 0))
                .exists()
        );
        assert!(
            storage
                .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(24, 1, 0))
                .exists()
        );
    }

    #[test]
    fn uninstall_and_cache_lifecycle_are_exposed_by_core() {
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        storage.ensure_layout().unwrap();
        let version = semver::Version::new(2, 1, 0);
        let executable = storage.runtime_executable(RuntimeKind::Deno, &version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"deno").unwrap();
        let cache = storage.cache_dir().join("downloads").join("archive.zip");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"archive").unwrap();
        let jolter = Jolter::with_storage(storage).unwrap();

        assert!(jolter.cache_stats().unwrap().files > 0);
        assert!(
            jolter
                .uninstall_runtime(RuntimeKind::Deno, &version, false)
                .unwrap()
                .reclaimed_bytes
                > 0
        );
        assert!(!executable.exists());
        assert!(jolter.clean_cache().unwrap().removed_files > 0);
    }

    #[test]
    fn active_tools_are_protected_from_prune_and_uninstall() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"24"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        storage.ensure_layout().unwrap();
        let node_version = semver::Version::new(24, 1, 0);
        let node = storage.runtime_executable(RuntimeKind::Node, &node_version);
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(node, b"node").unwrap();
        for version in [
            semver::Version::new(9, 1, 0),
            semver::Version::new(10, 2, 0),
        ] {
            let entrypoint = storage
                .tool_entrypoint(ToolKind::Pnpm, &version, "pnpm")
                .unwrap();
            fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
            fs::write(entrypoint, b"pnpm").unwrap();
        }
        let active = semver::Version::new(9, 1, 0);
        storage.activate_tool(ToolKind::Pnpm, &active).unwrap();
        let jolter = Jolter::with_storage(storage.clone()).unwrap();

        let preview = jolter.prune(project.path(), 0, true).unwrap();
        assert_eq!(preview.removed.len(), 1);
        assert_eq!(preview.removed[0].version, semver::Version::new(10, 2, 0));
        assert!(matches!(
            jolter.uninstall_tool(ToolKind::Pnpm, &active, false),
            Err(CoreError::ActiveToolRemoval { .. })
        ));
        jolter
            .uninstall_tool(ToolKind::Pnpm, &active, true)
            .unwrap();
        assert_eq!(storage.active_tool_version(ToolKind::Pnpm).unwrap(), None);
    }
}
