use std::path::{Path, PathBuf};

use jolter_config::{CONFIG_FILE_NAME, ProjectConfig, RuntimeConfig};
use jolter_doctor::Report;
use jolter_installer::{InstallOutcome, Installer, ToolInstallOutcome};
use jolter_resolver::resolve;
use jolter_runtime::{PackageManagerRequest, RuntimeKind, RuntimeRequest};
use jolter_storage::{InstalledRuntime, InstalledTool, Storage};
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

    pub fn list_tools(&self) -> Result<Vec<InstalledTool>, CoreError> {
        Ok(self.storage.installed_tools()?)
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
        let package_manager = resolution
            .package_manager
            .map(|resolved| {
                if action.runtime.kind != RuntimeKind::Node {
                    return Err(CoreError::PackageManagerRequiresNode(resolved.request));
                }
                let tool = self.ensure_package_manager(
                    &resolved.request,
                    &action.runtime.version,
                    repair,
                )?;
                Ok(PackageManagerAction {
                    request: resolved.request,
                    tool: tool.tool,
                    downloaded: tool.downloaded,
                })
            })
            .transpose()?;

        Ok(SyncOutcome {
            runtime: action.runtime,
            downloaded: action.downloaded,
            package_manager,
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

    fn ensure_package_manager(
        &self,
        request: &PackageManagerRequest,
        node_version: &semver::Version,
        repair: bool,
    ) -> Result<ToolInstallOutcome, CoreError> {
        if !request.selector.eq_ignore_ascii_case("latest") && request.hash.is_none() {
            if let Some(tool) = self.storage.find_matching_tool(request)? {
                self.installer
                    .validate_installed_package_manager(&tool, node_version)?;
                return Ok(ToolInstallOutcome {
                    tool,
                    downloaded: false,
                });
            }
        }
        if repair {
            Ok(self
                .installer
                .repair_package_manager(request, node_version)?)
        } else {
            Ok(self
                .installer
                .install_package_manager(request, node_version)?)
        }
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
    pub package_manager: Option<PackageManagerAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManagerAction {
    pub request: PackageManagerRequest,
    pub tool: InstalledTool,
    pub downloaded: bool,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("no runtime requirement was found from {0}")]
    NoRuntimeRequirement(PathBuf),
    #[error("package manager {0} requires a Node.js runtime")]
    PackageManagerRequiresNode(PackageManagerRequest),
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

    #[test]
    fn sync_uses_an_existing_matching_package_manager_without_network() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        let runtime_version = semver::Version::new(24, 1, 0);
        let executable = storage.runtime_executable(RuntimeKind::Node, &runtime_version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"node").unwrap();
        let tool_version = semver::Version::new(10, 2, 0);
        let entrypoint = storage
            .tool_entrypoint(
                jolter_runtime::PackageManagerKind::Pnpm,
                &tool_version,
                "pnpm",
            )
            .unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"pnpm").unwrap();
        let jolter = Jolter::with_storage(storage).unwrap();

        let outcome = jolter.sync(project.path()).unwrap();

        assert_eq!(outcome.package_manager.unwrap().tool.version, tool_version);
    }

    #[test]
    fn sync_rejects_an_existing_package_manager_incompatible_with_node() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"20"},"packageManager":{"pnpm":"11"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        let runtime_version = semver::Version::new(20, 19, 0);
        let executable = storage.runtime_executable(RuntimeKind::Node, &runtime_version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"node").unwrap();
        let tool_version = semver::Version::new(11, 6, 0);
        let entrypoint = storage
            .tool_entrypoint(
                jolter_runtime::PackageManagerKind::Pnpm,
                &tool_version,
                "pnpm",
            )
            .unwrap();
        fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
        fs::write(entrypoint, b"pnpm").unwrap();
        fs::write(
            storage
                .tool_version_dir(jolter_runtime::PackageManagerKind::Pnpm, &tool_version)
                .join("package.json"),
            r#"{"engines":{"node":">=22.13"}}"#,
        )
        .unwrap();
        let jolter = Jolter::with_storage(storage).unwrap();

        let error = jolter.sync(project.path()).unwrap_err();

        assert!(matches!(
            error,
            CoreError::Installer(jolter_installer::InstallerError::IncompatibleNodeVersion { .. })
        ));
    }
}
