use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use jolter_runtime::{RuntimeKind, ToolKind, ToolRequest};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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

    pub fn installed_runtimes(&self) -> Result<Vec<InstalledRuntime>, StorageError> {
        let mut installed = Vec::new();
        for kind in RuntimeKind::ALL {
            let directory = self.runtime_dir(kind);
            if !directory.exists() {
                continue;
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
        Ok(self
            .installed_runtimes()?
            .into_iter()
            .rev()
            .find(|runtime| {
                runtime.kind == request.kind
                    && request.matches_version(&runtime.version)
                    && runtime_executable_in(&runtime.path, runtime.kind).is_file()
            }))
    }

    pub fn installed_tools(&self) -> Result<Vec<InstalledTool>, StorageError> {
        let mut installed = Vec::new();
        for kind in ToolKind::ALL {
            let directory = self.tool_dir(kind);
            if !directory.exists() {
                continue;
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
        Ok(self.installed_tools()?.into_iter().rev().find(|tool| {
            tool.kind == request.kind
                && request.matches_version(&tool.version)
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
            let stats = directory_stats(&self.cache_dir().join(name))?;
            total.files = total.files.saturating_add(stats.files);
            total.bytes = total.bytes.saturating_add(stats.bytes);
        }
        Ok(total)
    }

    pub fn path_stats(&self, path: &Path) -> Result<CacheStats, StorageError> {
        directory_stats(path)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRuntime {
    pub kind: RuntimeKind,
    pub version: Version,
    pub path: PathBuf,
}

impl InstalledRuntime {
    #[must_use]
    pub fn executable(&self) -> PathBuf {
        runtime_executable_in(&self.path, self.kind)
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.executable().is_file()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTool {
    pub kind: ToolKind,
    pub version: Version,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub canonical_name: String,
    pub version: Version,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPluginTool {
    pub provider: String,
    pub tool: String,
    pub version: Version,
    pub path: PathBuf,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub files: u64,
    pub bytes: u64,
}

impl InstalledTool {
    #[must_use]
    pub fn primary_entrypoint(&self) -> Option<PathBuf> {
        self.kind
            .entrypoint(&self.kind.to_string())
            .map(|entrypoint| self.path.join(entrypoint))
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.primary_entrypoint()
            .is_some_and(|entrypoint| entrypoint.is_file())
    }
}

impl InstalledPluginTool {
    #[must_use]
    pub fn executable_for_command(&self, command: &str) -> PathBuf {
        #[cfg(windows)]
        {
            let exe = self.path.join(format!("{command}.exe"));
            if exe.is_file() {
                return exe;
            }
        }
        self.path.join(command)
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.commands
            .iter()
            .any(|command| self.executable_for_command(command).is_file())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ActiveVersions {
    #[serde(flatten)]
    versions: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ActivePluginTools {
    #[serde(default)]
    tools: BTreeMap<String, ActivePluginTool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActivePluginTool {
    provider: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct PluginToolInstallManifest {
    #[serde(default)]
    commands: Vec<String>,
}

fn read_installed_plugin_tool_versions(
    provider: &str,
    tool: &str,
    root: &Path,
) -> Result<Vec<InstalledPluginTool>, StorageError> {
    let mut installed = Vec::new();
    for entry in fs::read_dir(root).map_err(|source| StorageError::Read {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::Read {
            path: root.to_path_buf(),
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
        let version_name = entry.file_name().to_string_lossy().into_owned();
        let Ok(version) = Version::parse(version_name.trim_start_matches('v')) else {
            continue;
        };
        let path = entry.path();
        let commands = read_plugin_tool_commands(&path).unwrap_or_default();
        installed.push(InstalledPluginTool {
            provider: provider.to_owned(),
            tool: tool.to_owned(),
            version,
            path,
            commands,
        });
    }
    Ok(installed)
}

fn read_plugin_tool_commands(path: &Path) -> Result<Vec<String>, StorageError> {
    let manifest_path = path.join(".jolter-plugin-tool.json");
    let contents = fs::read_to_string(&manifest_path).map_err(|source| StorageError::ReadFile {
        path: manifest_path.clone(),
        source,
    })?;
    let manifest: PluginToolInstallManifest =
        serde_json::from_str(&contents).map_err(|source| StorageError::ParseActive {
            path: manifest_path,
            source,
        })?;
    Ok(manifest.commands)
}

#[must_use]
pub fn runtime_executable_in(root: &Path, kind: RuntimeKind) -> PathBuf {
    match kind {
        RuntimeKind::Node => {
            #[cfg(windows)]
            {
                root.join("node.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("bin").join("node")
            }
        }
        RuntimeKind::Bun => {
            #[cfg(windows)]
            {
                root.join("bun.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("bun")
            }
        }
        RuntimeKind::Deno => {
            #[cfg(windows)]
            {
                root.join("deno.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("deno")
            }
        }
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or_else(|| StorageError::InvalidPath {
        path: path.to_path_buf(),
    })?;
    fs::create_dir_all(parent).map_err(|source| StorageError::Create {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| StorageError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    std::io::Write::write_all(&mut temporary, contents).map_err(|source| {
        StorageError::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    })?;
    temporary
        .persist(path)
        .map_err(|error| StorageError::WriteFile {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    Ok(())
}

fn directory_stats(path: &Path) -> Result<CacheStats, StorageError> {
    if !path.exists() {
        return Ok(CacheStats::default());
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| StorageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return Ok(CacheStats {
            files: 1,
            bytes: metadata.len(),
        });
    }

    let mut stats = CacheStats::default();
    let entries = fs::read_dir(path).map_err(|source| StorageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| StorageError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let child = directory_stats(&entry.path())?;
        stats.files = stats.files.saturating_add(child.files);
        stats.bytes = stats.bytes.saturating_add(child.bytes);
    }
    Ok(stats)
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}

fn plugin_path_parts(canonical_name: &str) -> (String, String) {
    let normalized = canonical_name
        .trim()
        .trim_start_matches('@')
        .to_ascii_lowercase();
    let (scope, name) = normalized
        .split_once('/')
        .unwrap_or(("unknown", normalized.as_str()));
    (scope.to_owned(), name.to_owned())
}

fn selector_matches_version(selector: &str, version: &Version) -> bool {
    if selector.eq_ignore_ascii_case("latest")
        || selector == "*"
        || selector.eq_ignore_ascii_case("x")
    {
        return true;
    }
    let Ok(parts) = selector
        .split('.')
        .filter(|part| !part.eq_ignore_ascii_case("x") && *part != "*")
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    match parts.as_slice() {
        [major] => version.major == *major,
        [major, minor] => version.major == *major && version.minor == *minor,
        [major, minor, patch] => {
            version.major == *major && version.minor == *minor && version.patch == *patch
        }
        _ => false,
    }
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("could not determine the user home directory; set JOLTER_HOME explicitly")]
    HomeDirectoryUnavailable,
    #[error("failed to create storage directory {path}: {source}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read storage directory {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read storage file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write storage file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid active toolchain configuration at {path}: {source}")]
    ParseActive {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize active toolchain configuration: {0}")]
    SerializeActive(#[source] serde_json::Error),
    #[error("active {kind} version `{value}` is invalid: {source}")]
    InvalidActiveVersion {
        kind: RuntimeKind,
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("active {kind} tool version `{value}` is invalid: {source}")]
    InvalidActiveToolVersion {
        kind: ToolKind,
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("active plugin tool {tool} version `{value}` is invalid: {source}")]
    InvalidActivePluginToolVersion {
        tool: String,
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("invalid storage path {path}")]
    InvalidPath { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_expected_layout() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        assert!(storage.runtime_dir(RuntimeKind::Node).is_dir());
        assert!(storage.runtime_dir(RuntimeKind::Bun).is_dir());
        assert!(storage.runtime_dir(RuntimeKind::Deno).is_dir());
        assert!(storage.shims_dir().is_dir());
        assert!(storage.tool_dir(ToolKind::Pnpm).is_dir());
    }

    #[test]
    fn lists_only_semver_runtime_directories() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        fs::create_dir(storage.runtime_dir(RuntimeKind::Node).join("24.1.0")).unwrap();
        fs::create_dir(
            storage
                .runtime_dir(RuntimeKind::Node)
                .join("partial-download"),
        )
        .unwrap();

        let installed = storage.installed_runtimes().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].version, Version::new(24, 1, 0));
    }

    #[test]
    fn persists_active_runtime_versions() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        storage
            .activate(RuntimeKind::Node, &Version::new(24, 2, 0))
            .unwrap();
        storage
            .activate(RuntimeKind::Node, &Version::new(24, 3, 0))
            .unwrap();

        assert_eq!(
            storage.active_version(RuntimeKind::Node).unwrap(),
            Some(Version::new(24, 3, 0))
        );
    }

    #[test]
    fn persists_active_tool_versions_alongside_runtimes() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        storage
            .activate(RuntimeKind::Node, &Version::new(24, 2, 0))
            .unwrap();
        storage
            .activate_tool(ToolKind::Pnpm, &Version::new(10, 2, 0))
            .unwrap();

        assert_eq!(
            storage.active_version(RuntimeKind::Node).unwrap(),
            Some(Version::new(24, 2, 0))
        );
        assert_eq!(
            storage.active_tool_version(ToolKind::Pnpm).unwrap(),
            Some(Version::new(10, 2, 0))
        );
        assert!(
            storage
                .deactivate_tool(ToolKind::Pnpm, Some(&Version::new(10, 2, 0)))
                .unwrap()
        );
        assert_eq!(storage.active_tool_version(ToolKind::Pnpm).unwrap(), None);
        assert_eq!(
            storage.active_version(RuntimeKind::Node).unwrap(),
            Some(Version::new(24, 2, 0))
        );
    }

    #[test]
    fn finds_highest_matching_complete_tool() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        for version in [Version::new(10, 1, 0), Version::new(10, 2, 0)] {
            let entrypoint = storage
                .tool_entrypoint(ToolKind::Pnpm, &version, "pnpm")
                .unwrap();
            fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
            fs::write(entrypoint, b"pnpm").unwrap();
        }

        let found = storage
            .find_matching_tool(&"pnpm@10".parse().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(found.version, Version::new(10, 2, 0));
    }

    #[test]
    fn deactivates_only_the_expected_runtime_version() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        let active = Version::new(24, 2, 0);
        storage.activate(RuntimeKind::Node, &active).unwrap();

        assert!(
            !storage
                .deactivate(RuntimeKind::Node, Some(&Version::new(22, 0, 0)))
                .unwrap()
        );
        assert_eq!(
            storage.active_version(RuntimeKind::Node).unwrap(),
            Some(active.clone())
        );
        assert!(
            storage
                .deactivate(RuntimeKind::Node, Some(&active))
                .unwrap()
        );
        assert_eq!(storage.active_version(RuntimeKind::Node).unwrap(), None);
    }

    #[test]
    fn reports_recursive_cache_size() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        let cache_file = storage.cache_dir().join("downloads").join("archive");
        fs::create_dir_all(cache_file.parent().unwrap()).unwrap();
        fs::write(cache_file, b"12345").unwrap();

        assert_eq!(
            storage.cache_stats().unwrap(),
            CacheStats { files: 1, bytes: 5 }
        );
    }

    #[test]
    fn finds_highest_matching_complete_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        for version in [Version::new(24, 1, 0), Version::new(24, 2, 0)] {
            let executable = storage.runtime_executable(RuntimeKind::Node, &version);
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(executable, b"node").unwrap();
        }
        fs::create_dir_all(storage.runtime_dir(RuntimeKind::Node).join("24.3.0")).unwrap();

        let found = storage
            .find_matching(&"node@24".parse().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(found.version, Version::new(24, 2, 0));
        assert!(found.is_complete());
    }

    #[test]
    fn lists_semver_tool_directories_and_completion_state() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        let complete = Version::new(10, 2, 0);
        let entrypoint = storage
            .tool_entrypoint(ToolKind::Pnpm, &complete, "pnpm")
            .unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"pnpm").unwrap();
        fs::create_dir_all(storage.tool_dir(ToolKind::Yarn).join("4.1.0")).unwrap();
        fs::create_dir_all(storage.tool_dir(ToolKind::Npm).join("partial")).unwrap();

        let installed = storage.installed_tools().unwrap();

        assert_eq!(installed.len(), 2);
        assert!(installed.iter().any(InstalledTool::is_complete));
        assert!(installed.iter().any(|tool| !tool.is_complete()));
    }

    #[test]
    fn reports_invalid_active_versions_and_can_clear_any_version() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        let active_path = storage.config_dir().join("active.json");
        fs::write(&active_path, r#"{"node":"not-semver"}"#).unwrap();
        assert!(matches!(
            storage.active_version(RuntimeKind::Node),
            Err(StorageError::InvalidActiveVersion { .. })
        ));

        fs::remove_file(active_path).unwrap();
        storage
            .activate(RuntimeKind::Node, &Version::new(24, 1, 0))
            .unwrap();
        assert!(storage.deactivate(RuntimeKind::Node, None).unwrap());
        assert_eq!(storage.active_version(RuntimeKind::Node).unwrap(), None);
    }

    #[test]
    fn path_stats_handles_files_and_missing_paths() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        let file = temp.path().join("single");
        fs::write(&file, b"abc").unwrap();

        assert_eq!(
            storage.path_stats(&file).unwrap(),
            CacheStats { files: 1, bytes: 3 }
        );
        assert_eq!(
            storage.path_stats(&temp.path().join("missing")).unwrap(),
            CacheStats::default()
        );
    }
}
