use std::path::PathBuf;

use jolter_installer::{InstallOutcome, PluginToolInstallOutcome};
use jolter_runtime::{RuntimeKind, ToolKind, ToolRequest};
use jolter_storage::{InstalledPlugin, InstalledPluginTool, InstalledRuntime, InstalledTool};
use semver::Version;

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
