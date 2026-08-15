use std::path::Path;

use jolter_config::{CONFIG_FILE_NAME, ProjectConfig, RuntimeConfig};
use jolter_installer::{ProgressAction, RemovalOutcome};
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolRequest};
use jolter_storage::InstalledRuntime;
use semver::Version;

use crate::{Jolter, error::CoreError, types::RuntimeAction};

impl Jolter {
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

    pub fn install_runtime(&self, request: &RuntimeRequest) -> Result<RuntimeAction, CoreError> {
        let target = request.to_string();
        self.report(ProgressAction::Select, &target);
        self.ensure_runtime(request, false, false)
    }

    pub fn use_runtime(&self, request: &RuntimeRequest) -> Result<RuntimeAction, CoreError> {
        let target = request.to_string();
        self.report(ProgressAction::Select, &target);
        self.ensure_runtime(request, false, true)
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
        let outcome = self.with_installer(|installer| Ok(installer.install(request)?))?;
        let target = format!("{}@{}", outcome.runtime.kind, outcome.runtime.version);
        self.report(ProgressAction::Activate, &target);
        self.storage
            .activate(outcome.runtime.kind, &outcome.runtime.version)?;
        Ok(RuntimeAction::from(outcome))
    }

    pub fn list(&self) -> Result<Vec<InstalledRuntime>, CoreError> {
        Ok(self.storage.installed_runtimes()?)
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
        let outcome =
            self.with_installer(|installer| Ok(installer.uninstall_runtime(kind, version)?))?;
        if active.as_ref() == Some(version) {
            self.storage.deactivate(kind, Some(version))?;
        }
        Ok(outcome)
    }

    pub(crate) fn ensure_runtime(
        &self,
        request: &RuntimeRequest,
        repair: bool,
        activate: bool,
    ) -> Result<RuntimeAction, CoreError> {
        if !request.requires_release_metadata() {
            if let Some(runtime) = self.storage.find_matching(request)? {
                let target = format!("{}@{}", runtime.kind, runtime.version);
                self.report(ProgressAction::Reuse, &target);
                if activate {
                    self.report(ProgressAction::Activate, &target);
                    self.storage.activate(runtime.kind, &runtime.version)?;
                }
                return Ok(RuntimeAction {
                    runtime,
                    downloaded: false,
                });
            }
        }

        let outcome = if repair {
            self.with_installer(|installer| Ok(installer.repair(request)?))?
        } else {
            self.with_installer(|installer| Ok(installer.install(request)?))?
        };
        let target = format!("{}@{}", outcome.runtime.kind, outcome.runtime.version);
        if activate {
            self.report(ProgressAction::Activate, &target);
            self.storage
                .activate(outcome.runtime.kind, &outcome.runtime.version)?;
        }
        Ok(RuntimeAction::from(outcome))
    }

    pub(crate) fn active_node_version(&self, request: &ToolRequest) -> Result<Version, CoreError> {
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
}
