use std::path::PathBuf;

use jolter_runtime::RuntimeKind;
use jolter_storage::InstalledRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimTarget {
    Runtime(RuntimeKind),
    NodeTool(&'static str),
    PluginCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    pub executable: PathBuf,
    pub arguments: Vec<PathBuf>,
    pub runtime_root: Option<PathBuf>,
    pub runtime: Option<InstalledRuntime>,
}

#[must_use]
pub fn target_for_command(command: &str) -> Option<ShimTarget> {
    match command {
        "node" => Some(ShimTarget::Runtime(RuntimeKind::Node)),
        "bun" => Some(ShimTarget::Runtime(RuntimeKind::Bun)),
        "deno" => Some(ShimTarget::Runtime(RuntimeKind::Deno)),
        "npm" => Some(ShimTarget::NodeTool("npm")),
        "npx" => Some(ShimTarget::NodeTool("npx")),
        "pnpm" => Some(ShimTarget::NodeTool("pnpm")),
        "yarn" => Some(ShimTarget::NodeTool("yarn")),
        _ => None,
    }
}
