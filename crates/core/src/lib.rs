use std::path::{Path, PathBuf};

use jolter_config::{CONFIG_FILE_NAME, ProjectConfig, RuntimeConfig};
use jolter_doctor::Report;
use jolter_installer::{InstallOutcome, Installer};
use jolter_resolver::{PackageManagerRequest, resolve};
use jolter_runtime::{RuntimeKind, RuntimeRequest};
use jolter_storage::{InstalledRuntime, Storage};
use thiserror::Error;

pub struct Jolter {
    storage: Storage,
    installer: Installer,
}

impl Jolter {
    pub fn discover() -> Result<Self, CoreError> {
        Self::with_storage(Storage::discover()?)
    }

    pub fn with_storage(storage: Storage) -> Result<Self, CoreError> {
        storage.ensure_layout()?;
        let installer = Installer::new(storage.clone())?;
        Ok(Self { storage, installer })
    }

    #[must_use]
    pub const fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn pin(&self, project: &Path, request: &RuntimeRequest) -> Result<(), CoreError> {
        let path = project.join(CONFIG_FILE_NAME);
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

    pub fn use_runtime(&self, request: &RuntimeRequest) -> Result<RuntimeAction, CoreError> {
        self.ensure_runtime(request, false)
    }

    pub fn list(&self) -> Result<Vec<InstalledRuntime>, CoreError> {
        Ok(self.storage.installed_runtimes()?)
    }

    pub fn doctor(&self, project: &Path) -> Result<Report, CoreError> {
        Ok(jolter_doctor::examine(project, &self.storage)?)
    }

    pub fn sync(&self, project: &Path) -> Result<SyncOutcome, CoreError> {
        self.sync_inner(project, false)
    }

    pub fn repair(&self, project: &Path) -> Result<SyncOutcome, CoreError> {
        self.sync_inner(project, true)
    }

    pub fn install_shims(&self, executable: &Path) -> Result<Vec<PathBuf>, CoreError> {
        Ok(jolter_shim::install_shims(executable, &self.storage)?)
    }

    fn sync_inner(&self, project: &Path, repair: bool) -> Result<SyncOutcome, CoreError> {
        let resolution = resolve(project)?;
        let runtime = resolution
            .runtime
            .ok_or_else(|| CoreError::NoRuntimeRequirement(project.to_path_buf()))?;
        let action = self.ensure_runtime(&runtime.request, repair)?;
        let package_manager_ready = resolution
            .package_manager
            .as_ref()
            .is_some_and(|request| package_manager_exists(&self.storage, &action.runtime, request));

        Ok(SyncOutcome {
            runtime: action.runtime,
            downloaded: action.downloaded,
            package_manager: resolution.package_manager,
            package_manager_ready,
        })
    }

    fn ensure_runtime(
        &self,
        request: &RuntimeRequest,
        repair: bool,
    ) -> Result<RuntimeAction, CoreError> {
        if !request.requires_release_metadata() {
            if let Some(runtime) = self.storage.find_matching(request)? {
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
        self.storage
            .activate(outcome.runtime.kind, &outcome.runtime.version)?;
        Ok(RuntimeAction::from(outcome))
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
    pub package_manager: Option<PackageManagerRequest>,
    pub package_manager_ready: bool,
}

fn package_manager_exists(
    storage: &Storage,
    runtime: &InstalledRuntime,
    request: &PackageManagerRequest,
) -> bool {
    runtime.kind == RuntimeKind::Node
        && matches!(request.name.as_str(), "npm" | "npx" | "pnpm" | "yarn")
        && storage
            .node_tool_executable(&runtime.version, &request.name)
            .is_file()
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("no runtime requirement was found from {0}")]
    NoRuntimeRequirement(PathBuf),
    #[error(transparent)]
    Config(#[from] jolter_config::ConfigError),
    #[error(transparent)]
    Doctor(#[from] jolter_doctor::DoctorError),
    #[error(transparent)]
    Installer(#[from] jolter_installer::InstallerError),
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
    fn pin_preserves_package_manager_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let mut package_manager = BTreeMap::new();
        package_manager.insert("pnpm".to_owned(), "10.x".to_owned());
        ProjectConfig {
            runtime: RuntimeConfig::default(),
            package_manager,
        }
        .write_to(&temp.path().join(CONFIG_FILE_NAME))
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let jolter = Jolter::with_storage(Storage::new(storage_temp.path())).unwrap();

        jolter
            .pin(temp.path(), &"node@24".parse().unwrap())
            .unwrap();

        let config = ProjectConfig::from_path(&temp.path().join(CONFIG_FILE_NAME)).unwrap();
        assert_eq!(config.runtime.node.as_deref(), Some("24"));
        assert_eq!(
            config.package_manager.get("pnpm").map(String::as_str),
            Some("10.x")
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
}
