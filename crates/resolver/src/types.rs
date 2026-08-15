use std::path::PathBuf;

use jolter_runtime::{RuntimeRequest, ToolRequest};

use crate::dev_engines::DevEngines;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectResolution {
    pub root: PathBuf,
    pub runtime: Option<ResolvedRuntime>,
    pub tools: Vec<ResolvedTool>,
    pub plugin_tools: Vec<ResolvedPluginTool>,
    pub plugins: Vec<ResolvedPlugin>,
    pub dev_engines: Option<DevEngines>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuntime {
    pub request: RuntimeRequest,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTool {
    pub request: ToolRequest,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPluginTool {
    pub name: String,
    pub selector: String,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlugin {
    pub name: String,
    pub selector: String,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementSource {
    JolterConfig,
    PackageJson,
    NodeVersion,
    Nvmrc,
}
