use std::{collections::BTreeMap, path::Path};

use jolter_config::{CONFIG_FILE_NAME, ProjectConfig};
use jolter_installer::{
    ArchiveFormat, Artifact, ArtifactIntegrity, PluginToolArchive, PluginToolInstallOutcome,
    ProgressAction, RemovalOutcome,
};
use jolter_plugin::{PluginPlatform, PluginRequest, PluginToolRelease, read_installed_manifest};
use jolter_storage::{InstalledPlugin, InstalledPluginTool};
use semver::Version;

use crate::{
    Jolter,
    error::CoreError,
    types::{PluginAction, PluginToolAction},
};

impl Jolter {
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

    pub fn install_plugin_tool(
        &self,
        name: &str,
        selector: &str,
    ) -> Result<PluginToolAction, CoreError> {
        let requested = format!("{name}@{selector}");
        self.report(ProgressAction::Select, &requested);
        let provider = self.find_plugin_for_tool(name, None)?;
        let outcome = self.ensure_plugin_tool(&provider, name, selector, false)?;
        Ok(PluginToolAction::from(outcome))
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

    pub fn list_plugin_tools(&self) -> Result<Vec<InstalledPluginTool>, CoreError> {
        Ok(self.storage.installed_plugin_tools()?)
    }

    pub fn list_plugins(&self) -> Result<Vec<InstalledPlugin>, CoreError> {
        Ok(self.storage.installed_plugins()?)
    }

    pub fn install_plugin(&self, request: &PluginRequest) -> Result<PluginAction, CoreError> {
        let target = request.to_string();
        self.report(ProgressAction::Resolve, &target);
        let plugin = self.with_plugins(|plugins| Ok(plugins.install(request)?))?;
        Ok(PluginAction { plugin })
    }

    pub fn update_plugin(&self, name: &str) -> Result<PluginAction, CoreError> {
        self.install_plugin(&PluginRequest::new(name, "latest")?)
    }

    pub fn uninstall_plugin(&self, name: &str, force: bool) -> Result<RemovalOutcome, CoreError> {
        let canonical = if name.starts_with('@') {
            name.to_ascii_lowercase()
        } else {
            self.with_plugins(|plugins| Ok(plugins.resolve_name(name)?))?
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
        let outcome = self.with_installer(|installer| {
            Ok(installer.uninstall_plugin_tool(provider, tool, version)?)
        })?;
        if active
            .as_ref()
            .is_some_and(|active| active.provider == provider && active.version == *version)
        {
            self.storage.deactivate_plugin_tool(tool, Some(version))?;
        }
        Ok(outcome)
    }

    pub(crate) fn find_plugin_for_tool(
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
                    if self
                        .with_plugins(|plugins| Ok(plugins.resolve_name(name)?))
                        .ok()
                        .as_deref()
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

    pub(crate) fn ensure_plugin_tool(
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
        let outcome =
            self.with_installer(|installer| Ok(installer.install_plugin_tool(&archive, repair)?))?;
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
        self.with_plugin_executor(|executor| {
            Ok(executor.resolve_tool(
                &provider.path.join("plugin.wasm"),
                tool,
                selector,
                current_plugin_platform(),
            )?)
        })
    }

    fn validate_plugin_tool(
        &self,
        provider: &InstalledPlugin,
        tool: &InstalledPluginTool,
    ) -> Result<bool, CoreError> {
        self.with_plugin_executor(|executor| {
            Ok(executor.validate_installed(
                &provider.path.join("plugin.wasm"),
                &tool.tool,
                &tool.version,
                &tool.path,
            )?)
        })
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
