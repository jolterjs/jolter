pub mod active;
pub mod clean;
pub mod error;
pub mod manifest;
pub mod paths;
pub mod types;

#[cfg(test)]
mod tests;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use jolter_runtime::{RuntimeKind, ToolKind, ToolRequest};
use semver::Version;

use active::{ActivePluginTool, ActivePluginTools, ActiveVersions, atomic_write};
pub use error::StorageError;
use manifest::{read_installed_plugin_tool_versions, read_plugin_tool_commands};
use paths::home_directory;
pub use paths::{plugin_path_parts, runtime_executable_in, selector_matches_version};
pub use types::{
    CacheStats, InstalledPlugin, InstalledPluginTool, InstalledRuntime, InstalledTool,
};

#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn discover() -> Result<Self, StorageError> {
        if let Some(root) = env::var_os("JOLTER_HOME").filter(|value| !value.is_empty()) {
            return Ok(Self::new(root));
        }
        let home = home_directory().ok_or(StorageError::HomeDirectoryUnavailable)?;
        Ok(Self::new(home.join(".jolter")))
    }

    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn runtimes_dir(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    #[must_use]
    pub fn runtime_dir(&self, kind: RuntimeKind) -> PathBuf {
        self.runtimes_dir().join(kind.to_string())
    }

    #[must_use]
    pub fn runtime_version_dir(&self, kind: RuntimeKind, version: &Version) -> PathBuf {
        self.runtime_dir(kind).join(version.to_string())
    }

    #[must_use]
    pub fn runtime_executable(&self, kind: RuntimeKind, version: &Version) -> PathBuf {
        let root = self.runtime_version_dir(kind, version);
        runtime_executable_in(&root, kind)
    }

    #[must_use]
    pub fn node_tool_executable(&self, version: &Version, tool: &str) -> PathBuf {
        let root = self.runtime_version_dir(RuntimeKind::Node, version);
        #[cfg(windows)]
        {
            root.join(format!("{tool}.cmd"))
        }
        #[cfg(not(windows))]
        {
            root.join("bin").join(tool)
        }
    }

    #[must_use]
    pub fn shims_dir(&self) -> PathBuf {
        self.root.join("shims")
    }

    #[must_use]
    pub fn tools_dir(&self) -> PathBuf {
        self.root.join("tools")
    }

    #[must_use]
    pub fn plugins_dir(&self) -> PathBuf {
        self.root.join("plugins")
    }

    #[must_use]
    pub fn plugin_tools_dir(&self) -> PathBuf {
        self.root.join("plugin-tools")
    }

    #[must_use]
    pub fn plugin_dir(&self, canonical_name: &str) -> PathBuf {
        let (scope, name) = plugin_path_parts(canonical_name);
        self.plugins_dir().join(scope).join(name)
    }

    #[must_use]
    pub fn plugin_version_dir(&self, canonical_name: &str, version: &Version) -> PathBuf {
        self.plugin_dir(canonical_name).join(version.to_string())
    }

    #[must_use]
    pub fn plugin_tool_dir(&self, canonical_name: &str, tool: &str) -> PathBuf {
        let (scope, name) = plugin_path_parts(canonical_name);
        self.plugin_tools_dir().join(scope).join(name).join(tool)
    }

    #[must_use]
    pub fn plugin_tool_version_dir(
        &self,
        canonical_name: &str,
        tool: &str,
        version: &Version,
    ) -> PathBuf {
        self.plugin_tool_dir(canonical_name, tool)
            .join(version.to_string())
    }

    #[must_use]
    pub fn tool_dir(&self, kind: ToolKind) -> PathBuf {
        self.tools_dir().join(kind.to_string())
    }

    #[must_use]
    pub fn tool_version_dir(&self, kind: ToolKind, version: &Version) -> PathBuf {
        self.tool_dir(kind).join(version.to_string())
    }

    #[must_use]
    pub fn tool_entrypoint(
        &self,
        kind: ToolKind,
        version: &Version,
        command: &str,
    ) -> Option<PathBuf> {
        kind.entrypoint(command)
            .map(|entrypoint| self.tool_version_dir(kind, version).join(entrypoint))
    }

    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    #[must_use]
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    pub fn ensure_layout(&self) -> Result<(), StorageError> {
        for path in [
            self.runtimes_dir(),
            self.tools_dir(),
            self.plugins_dir(),
            self.plugin_tools_dir(),
            self.shims_dir(),
            self.cache_dir(),
            self.config_dir(),
        ] {
            fs::create_dir_all(&path).map_err(|source| StorageError::Create {
                path: path.clone(),
                source,
            })?;
        }
        for kind in RuntimeKind::ALL {
            let path = self.runtime_dir(kind);
            fs::create_dir_all(&path).map_err(|source| StorageError::Create { path, source })?;
        }
        for kind in ToolKind::ALL {
            let path = self.tool_dir(kind);
            fs::create_dir_all(&path).map_err(|source| StorageError::Create { path, source })?;
        }
        Ok(())
    }

    pub fn installed_plugins(&self) -> Result<Vec<InstalledPlugin>, StorageError> {
        let mut installed = Vec::new();
        let root = self.plugins_dir();
        if !root.exists() {
            return Ok(installed);
        }
        for scope in fs::read_dir(&root).map_err(|source| StorageError::Read {
            path: root.clone(),
            source,
        })? {
            let scope = scope.map_err(|source| StorageError::Read {
                path: root.clone(),
                source,
            })?;
            if !scope
                .file_type()
                .map_err(|source| StorageError::Read {
                    path: scope.path(),
                    source,
                })?
                .is_dir()
            {
                continue;
            }
            let scope_name = scope.file_name().to_string_lossy().into_owned();
            for plugin in fs::read_dir(scope.path()).map_err(|source| StorageError::Read {
                path: scope.path(),
                source,
            })? {
                let plugin = plugin.map_err(|source| StorageError::Read {
                    path: scope.path(),
                    source,
                })?;
                if !plugin
                    .file_type()
                    .map_err(|source| StorageError::Read {
                        path: plugin.path(),
                        source,
                    })?
                    .is_dir()
                {
                    continue;
                }
                let plugin_name = plugin.file_name().to_string_lossy().into_owned();
                let canonical_name = format!("@{scope_name}/{plugin_name}");
                for version_entry in
                    fs::read_dir(plugin.path()).map_err(|source| StorageError::Read {
                        path: plugin.path(),
                        source,
                    })?
                {
                    let version_entry = version_entry.map_err(|source| StorageError::Read {
                        path: plugin.path(),
                        source,
                    })?;
                    if !version_entry
                        .file_type()
                        .map_err(|source| StorageError::Read {
                            path: version_entry.path(),
                            source,
                        })?
                        .is_dir()
                    {
                        continue;
                    }
                    let version_name = version_entry.file_name().to_string_lossy().into_owned();
                    if let Ok(version) = Version::parse(version_name.trim_start_matches('v')) {
                        installed.push(InstalledPlugin {
                            canonical_name: canonical_name.clone(),
                            version,
                            path: version_entry.path(),
                        });
                    }
                }
            }
        }
        installed.sort_by(|left, right| {
            left.canonical_name
                .cmp(&right.canonical_name)
                .then_with(|| left.version.cmp(&right.version))
        });
        Ok(installed)
    }

    pub fn find_matching_plugin(
        &self,
        canonical_name: &str,
        selector: &str,
    ) -> Result<Option<InstalledPlugin>, StorageError> {
        Ok(self.installed_plugins()?.into_iter().rev().find(|plugin| {
            plugin.canonical_name.eq_ignore_ascii_case(canonical_name)
                && selector_matches_version(selector, &plugin.version)
                && plugin.path.join(".jolter-plugin.json").is_file()
        }))
    }

    pub fn installed_plugin_tools(&self) -> Result<Vec<InstalledPluginTool>, StorageError> {
        let mut installed = Vec::new();
        let root = self.plugin_tools_dir();
        if !root.exists() {
            return Ok(installed);
        }
        for scope in fs::read_dir(&root).map_err(|source| StorageError::Read {
            path: root.clone(),
            source,
        })? {
            let scope = scope.map_err(|source| StorageError::Read {
                path: root.clone(),
                source,
            })?;
            if !scope
                .file_type()
                .map_err(|source| StorageError::Read {
                    path: scope.path(),
                    source,
                })?
                .is_dir()
            {
                continue;
            }
            let scope_name = scope.file_name().to_string_lossy().into_owned();
            for plugin in fs::read_dir(scope.path()).map_err(|source| StorageError::Read {
                path: scope.path(),
                source,
            })? {
                let plugin = plugin.map_err(|source| StorageError::Read {
                    path: scope.path(),
                    source,
                })?;
                if !plugin
                    .file_type()
                    .map_err(|source| StorageError::Read {
                        path: plugin.path(),
                        source,
                    })?
                    .is_dir()
                {
                    continue;
                }
                let plugin_name = plugin.file_name().to_string_lossy().into_owned();
                let provider = format!("@{scope_name}/{plugin_name}");
                for tool in fs::read_dir(plugin.path()).map_err(|source| StorageError::Read {
                    path: plugin.path(),
                    source,
                })? {
                    let tool = tool.map_err(|source| StorageError::Read {
                        path: plugin.path(),
                        source,
                    })?;
                    if !tool
                        .file_type()
                        .map_err(|source| StorageError::Read {
                            path: tool.path(),
                            source,
                        })?
                        .is_dir()
                    {
                        continue;
                    }
                    let tool_name = tool.file_name().to_string_lossy().into_owned();
                    installed.extend(read_installed_plugin_tool_versions(
                        &provider,
                        &tool_name,
                        &tool.path(),
                    )?);
                }
            }
        }
        installed.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.tool.cmp(&right.tool))
                .then_with(|| left.version.cmp(&right.version))
        });
        Ok(installed)
    }

    pub fn find_matching_plugin_tool(
        &self,
        provider: &str,
        tool: &str,
        selector: &str,
    ) -> Result<Option<InstalledPluginTool>, StorageError> {
        Ok(self
            .installed_plugin_tools()?
            .into_iter()
            .rev()
            .find(|installed| {
                installed.provider.eq_ignore_ascii_case(provider)
                    && installed.tool == tool
                    && selector_matches_version(selector, &installed.version)
                    && installed.is_complete()
            }))
    }

    pub fn installed_runtimes_for_kind(
        &self,
        kind: RuntimeKind,
    ) -> Result<Vec<InstalledRuntime>, StorageError> {
        let mut installed = Vec::new();
        let directory = self.runtime_dir(kind);
        if !directory.exists() {
            return Ok(installed);
        }
        let entries = fs::read_dir(&directory).map_err(|source| StorageError::Read {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| StorageError::Read {
                path: directory.clone(),
                source,
            })?;
            if !entry
                .file_type()
                .map_err(|source| StorageError::Read {
                    path: entry.path(),
                    source,
                })?
                .is_dir()
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(version) = Version::parse(name.trim_start_matches('v')) {
                installed.push(InstalledRuntime {
                    kind,
                    version,
                    path: entry.path(),
                });
            }
        }
        installed.sort_by(|left, right| left.version.cmp(&right.version));
        Ok(installed)
    }

    pub fn installed_runtimes(&self) -> Result<Vec<InstalledRuntime>, StorageError> {
        let mut installed = Vec::new();
        for kind in RuntimeKind::ALL {
            installed.extend(self.installed_runtimes_for_kind(kind)?);
        }
        installed.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.version.cmp(&right.version))
        });
        Ok(installed)
    }

    pub fn find_matching(
        &self,
        request: &jolter_runtime::RuntimeRequest,
    ) -> Result<Option<InstalledRuntime>, StorageError> {
        if let Ok(version) = Version::parse(request.selector.trim_start_matches('v')) {
            let candidate_dir = self.runtime_version_dir(request.kind, &version);
            if candidate_dir.is_dir()
                && runtime_executable_in(&candidate_dir, request.kind).is_file()
            {
                return Ok(Some(InstalledRuntime {
                    kind: request.kind,
                    version,
                    path: candidate_dir,
                }));
            }
        }

        Ok(self
            .installed_runtimes_for_kind(request.kind)?
            .into_iter()
            .rev()
            .find(|runtime| {
                request.matches_version(&runtime.version)
                    && runtime_executable_in(&runtime.path, runtime.kind).is_file()
            }))
    }

    pub fn installed_tools_for_kind(
        &self,
        kind: ToolKind,
    ) -> Result<Vec<InstalledTool>, StorageError> {
        let mut installed = Vec::new();
        let directory = self.tool_dir(kind);
        if !directory.exists() {
            return Ok(installed);
        }
        let entries = fs::read_dir(&directory).map_err(|source| StorageError::Read {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| StorageError::Read {
                path: directory.clone(),
                source,
            })?;
            if !entry
                .file_type()
                .map_err(|source| StorageError::Read {
                    path: entry.path(),
                    source,
                })?
                .is_dir()
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(version) = Version::parse(name.trim_start_matches('v')) {
                installed.push(InstalledTool {
                    kind,
                    version,
                    path: entry.path(),
                });
            }
        }
        installed.sort_by(|left, right| left.version.cmp(&right.version));
        Ok(installed)
    }

    pub fn installed_tools(&self) -> Result<Vec<InstalledTool>, StorageError> {
        let mut installed = Vec::new();
        for kind in ToolKind::ALL {
            installed.extend(self.installed_tools_for_kind(kind)?);
        }
        installed.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.version.cmp(&right.version))
        });
        Ok(installed)
    }

    pub fn find_matching_tool(
        &self,
        request: &ToolRequest,
    ) -> Result<Option<InstalledTool>, StorageError> {
        if let Ok(version) = Version::parse(request.selector.trim_start_matches('v')) {
            let candidate_dir = self.tool_version_dir(request.kind, &version);
            if candidate_dir.is_dir()
                && self
                    .tool_entrypoint(request.kind, &version, &request.kind.to_string())
                    .is_some_and(|entrypoint| entrypoint.is_file())
            {
                return Ok(Some(InstalledTool {
                    kind: request.kind,
                    version,
                    path: candidate_dir,
                }));
            }
        }

        Ok(self
            .installed_tools_for_kind(request.kind)?
            .into_iter()
            .rev()
            .find(|tool| {
                request.matches_version(&tool.version)
                    && self
                        .tool_entrypoint(tool.kind, &tool.version, &tool.kind.to_string())
                        .is_some_and(|entrypoint| entrypoint.is_file())
            }))
    }

    pub fn activate(&self, kind: RuntimeKind, version: &Version) -> Result<(), StorageError> {
        self.write_active_version(kind.to_string(), version)
    }

    pub fn active_version(&self, kind: RuntimeKind) -> Result<Option<Version>, StorageError> {
        let active = self.read_active_versions()?;
        active
            .versions
            .get(&kind.to_string())
            .map(|value| {
                Version::parse(value).map_err(|source| StorageError::InvalidActiveVersion {
                    kind,
                    value: value.clone(),
                    source,
                })
            })
            .transpose()
    }

    pub fn activate_tool(&self, kind: ToolKind, version: &Version) -> Result<(), StorageError> {
        self.write_active_version(kind.to_string(), version)
    }

    pub fn active_tool_version(&self, kind: ToolKind) -> Result<Option<Version>, StorageError> {
        let active = self.read_active_versions()?;
        active
            .versions
            .get(&kind.to_string())
            .map(|value| {
                Version::parse(value).map_err(|source| StorageError::InvalidActiveToolVersion {
                    kind,
                    value: value.clone(),
                    source,
                })
            })
            .transpose()
    }

    pub fn deactivate(
        &self,
        kind: RuntimeKind,
        expected: Option<&Version>,
    ) -> Result<bool, StorageError> {
        self.remove_active_version(&kind.to_string(), expected)
    }

    pub fn deactivate_tool(
        &self,
        kind: ToolKind,
        expected: Option<&Version>,
    ) -> Result<bool, StorageError> {
        self.remove_active_version(&kind.to_string(), expected)
    }

    pub fn activate_plugin_tool(
        &self,
        provider: &str,
        tool: &str,
        version: &Version,
    ) -> Result<(), StorageError> {
        let path = self.config_dir().join("active-plugin-tools.json");
        let mut active = self.read_active_plugin_tools()?;
        active.tools.insert(
            tool.to_owned(),
            ActivePluginTool {
                provider: provider.to_owned(),
                version: version.to_string(),
            },
        );
        let contents = serde_json::to_vec_pretty(&active).map_err(StorageError::SerializeActive)?;
        atomic_write(&path, &contents)
    }

    pub fn active_plugin_tool(
        &self,
        tool: &str,
    ) -> Result<Option<InstalledPluginTool>, StorageError> {
        let active = self.read_active_plugin_tools()?;
        let Some(entry) = active.tools.get(tool) else {
            return Ok(None);
        };
        let version = Version::parse(&entry.version).map_err(|source| {
            StorageError::InvalidActivePluginToolVersion {
                tool: tool.to_owned(),
                value: entry.version.clone(),
                source,
            }
        })?;
        let path = self.plugin_tool_version_dir(&entry.provider, tool, &version);
        let commands = read_plugin_tool_commands(&path).unwrap_or_default();
        Ok(Some(InstalledPluginTool {
            provider: entry.provider.clone(),
            tool: tool.to_owned(),
            version,
            path,
            commands,
        }))
    }

    pub fn active_plugin_tools(&self) -> Result<Vec<InstalledPluginTool>, StorageError> {
        let active = self.read_active_plugin_tools()?;
        active
            .tools
            .keys()
            .map(|tool| self.active_plugin_tool(tool))
            .filter_map(Result::transpose)
            .collect()
    }

    pub fn deactivate_plugin_tool(
        &self,
        tool: &str,
        expected: Option<&Version>,
    ) -> Result<bool, StorageError> {
        let path = self.config_dir().join("active-plugin-tools.json");
        if !path.is_file() {
            return Ok(false);
        }
        let mut active = self.read_active_plugin_tools()?;
        let should_remove = active.tools.get(tool).is_some_and(|value| {
            expected.is_none_or(|version| value.version == version.to_string())
        });
        if !should_remove {
            return Ok(false);
        }
        active.tools.remove(tool);
        let contents = serde_json::to_vec_pretty(&active).map_err(StorageError::SerializeActive)?;
        atomic_write(&path, &contents)?;
        Ok(true)
    }

    fn write_active_version(&self, key: String, version: &Version) -> Result<(), StorageError> {
        let path = self.config_dir().join("active.json");
        let mut active = self.read_active_versions()?;
        active.versions.insert(key, version.to_string());
        let contents = serde_json::to_vec_pretty(&active).map_err(StorageError::SerializeActive)?;
        atomic_write(&path, &contents)
    }

    fn remove_active_version(
        &self,
        key: &str,
        expected: Option<&Version>,
    ) -> Result<bool, StorageError> {
        let path = self.config_dir().join("active.json");
        if !path.is_file() {
            return Ok(false);
        }
        let mut active = self.read_active_versions()?;
        let should_remove = active
            .versions
            .get(key)
            .is_some_and(|value| expected.is_none_or(|version| value == &version.to_string()));
        if !should_remove {
            return Ok(false);
        }
        active.versions.remove(key);
        let contents = serde_json::to_vec_pretty(&active).map_err(StorageError::SerializeActive)?;
        atomic_write(&path, &contents)?;
        Ok(true)
    }

    pub fn cache_stats(&self) -> Result<CacheStats, StorageError> {
        let mut total = CacheStats::default();
        for name in ["downloads", "metadata"] {
            let stats = clean::directory_stats(&self.cache_dir().join(name))?;
            total.files = total.files.saturating_add(stats.files);
            total.bytes = total.bytes.saturating_add(stats.bytes);
        }
        Ok(total)
    }

    pub fn path_stats(&self, path: &Path) -> Result<CacheStats, StorageError> {
        clean::directory_stats(path)
    }

    fn read_active_versions(&self) -> Result<ActiveVersions, StorageError> {
        let path = self.config_dir().join("active.json");
        if !path.is_file() {
            return Ok(ActiveVersions::default());
        }
        let contents = fs::read_to_string(&path).map_err(|source| StorageError::ReadFile {
            path: path.clone(),
            source,
        })?;
        serde_json::from_str(&contents).map_err(|source| StorageError::ParseActive { path, source })
    }

    fn read_active_plugin_tools(&self) -> Result<ActivePluginTools, StorageError> {
        let path = self.config_dir().join("active-plugin-tools.json");
        if !path.is_file() {
            return Ok(ActivePluginTools::default());
        }
        let contents = fs::read_to_string(&path).map_err(|source| StorageError::ReadFile {
            path: path.clone(),
            source,
        })?;
        serde_json::from_str(&contents).map_err(|source| StorageError::ParseActive { path, source })
    }
}
