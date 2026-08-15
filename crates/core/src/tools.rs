use std::path::Path;

use jolter_config::{CONFIG_FILE_NAME, ProjectConfig};
use jolter_installer::{ProgressAction, RemovalOutcome, ToolInstallOutcome};
use jolter_runtime::{ToolKind, ToolRequest};
use jolter_storage::InstalledTool;
use semver::Version;

use crate::{Jolter, error::CoreError, types::ToolAction};

impl Jolter {
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

    pub fn install_tool(&self, request: &ToolRequest) -> Result<ToolAction, CoreError> {
        let requested = request.to_string();
        self.report(ProgressAction::Select, &requested);
        let node_version = self.active_node_version(request)?;
        let outcome = self.ensure_tool(request, &node_version, false)?;
        Ok(ToolAction {
            request: request.clone(),
            tool: outcome.tool,
            downloaded: outcome.downloaded,
        })
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

    pub fn update_tool(&self, request: &ToolRequest) -> Result<ToolAction, CoreError> {
        let requested = request.to_string();
        self.report(ProgressAction::Resolve, &requested);
        let node_version = self.active_node_version(request)?;
        if request.hash.is_none()
            && Version::parse(request.selector.trim_start_matches('v')).is_ok()
            && let Some(tool) = self.storage.find_matching_tool(request)?
        {
            self.with_installer(|installer| {
                Ok(installer.validate_installed_tool(&tool, &node_version)?)
            })?;
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
        let outcome =
            self.with_installer(|installer| Ok(installer.install_tool(request, &node_version)?))?;
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

    pub fn list_tools(&self) -> Result<Vec<InstalledTool>, CoreError> {
        Ok(self.storage.installed_tools()?)
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
        let outcome =
            self.with_installer(|installer| Ok(installer.uninstall_tool(kind, version)?))?;
        if active.as_ref() == Some(version) {
            self.storage.deactivate_tool(kind, Some(version))?;
        }
        Ok(outcome)
    }

    pub(crate) fn ensure_tool(
        &self,
        request: &ToolRequest,
        node_version: &semver::Version,
        repair: bool,
    ) -> Result<ToolInstallOutcome, CoreError> {
        if !request.selector.eq_ignore_ascii_case("latest") && request.hash.is_none() {
            if let Some(tool) = self.storage.find_matching_tool(request)? {
                let target = format!("{}@{}", tool.kind, tool.version);
                self.report(ProgressAction::Reuse, &target);
                self.with_installer(|installer| {
                    Ok(installer.validate_installed_tool(&tool, node_version)?)
                })?;
                return Ok(ToolInstallOutcome {
                    tool,
                    downloaded: false,
                });
            }
        }
        if repair {
            self.with_installer(|installer| Ok(installer.repair_tool(request, node_version)?))
        } else {
            self.with_installer(|installer| Ok(installer.install_tool(request, node_version)?))
        }
    }
}
