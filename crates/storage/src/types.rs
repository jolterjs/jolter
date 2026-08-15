use std::path::PathBuf;

use jolter_runtime::{RuntimeKind, ToolKind};
use semver::Version;

use crate::paths::runtime_executable_in;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub files: u64,
    pub bytes: u64,
}
