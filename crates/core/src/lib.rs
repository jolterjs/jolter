use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use jolter_config::{CONFIG_FILE_NAME, ProjectConfig, RuntimeConfig};
use jolter_doctor::Report;
use jolter_installer::{
    CacheCleanOutcome, InstallOutcome, Installer, RemovalOutcome, ToolInstallOutcome,
};
use jolter_resolver::resolve;
use jolter_runtime::{PackageManagerKind, PackageManagerRequest, RuntimeKind, RuntimeRequest};
use jolter_storage::{CacheStats, InstalledRuntime, InstalledTool, Storage};
use semver::Version;
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

    pub fn uninstall_runtime(
        &self,
        kind: RuntimeKind,
        version: &Version,
        force: bool,
    ) -> Result<RemovalOutcome, CoreError> {
        let active = self.storage.active_version(kind)?;
        if active.as_ref() == Some(version) && !force {
            return Err(CoreError::ActiveRuntimeRemoval {
                kind,
                version: version.clone(),
            });
        }
        let outcome = self.installer.uninstall_runtime(kind, version)?;
        if active.as_ref() == Some(version) {
            self.storage.deactivate(kind, Some(version))?;
        }
        Ok(outcome)
    }

    pub fn uninstall_package_manager(
        &self,
        kind: PackageManagerKind,
        version: &Version,
    ) -> Result<RemovalOutcome, CoreError> {
        Ok(self.installer.uninstall_package_manager(kind, version)?)
    }

    pub fn prune(
        &self,
        project: &Path,
        keep: usize,
        dry_run: bool,
    ) -> Result<PruneOutcome, CoreError> {
        let resolution = resolve(project)?;
        let runtimes = self.storage.installed_runtimes()?;
        let tools = self.storage.installed_tools()?;
        let mut protected_runtimes = BTreeSet::new();
        let mut protected_tools = BTreeSet::new();

        for kind in RuntimeKind::ALL {
            if let Some(version) = self.storage.active_version(kind)? {
                protected_runtimes.insert((kind, version));
            }
            protected_runtimes.extend(
                runtimes
                    .iter()
                    .rev()
                    .filter(|runtime| runtime.kind == kind && runtime.is_complete())
                    .take(keep)
                    .map(|runtime| (runtime.kind, runtime.version.clone())),
            );
        }
        if let Some(runtime) = resolution.runtime {
            if let Some(installed) = self.storage.find_matching(&runtime.request)? {
                protected_runtimes.insert((installed.kind, installed.version));
            }
        }

        for kind in PackageManagerKind::ALL {
            protected_tools.extend(
                tools
                    .iter()
                    .rev()
                    .filter(|tool| tool.kind == kind && tool.is_complete())
                    .take(keep)
                    .map(|tool| (tool.kind, tool.version.clone())),
            );
        }
        if let Some(package_manager) = resolution.package_manager {
            if let Some(installed) = self.storage.find_matching_tool(&package_manager.request)? {
                protected_tools.insert((installed.kind, installed.version));
            }
        }

        let runtime_removals = runtimes
            .into_iter()
            .filter(|runtime| {
                !protected_runtimes.contains(&(runtime.kind, runtime.version.clone()))
            })
            .map(|runtime| PruneItem {
                kind: PruneItemKind::Runtime(runtime.kind),
                version: runtime.version,
                path: runtime.path,
                reclaimed_bytes: 0,
            });
        let tool_removals = tools
            .into_iter()
            .filter(|tool| !protected_tools.contains(&(tool.kind, tool.version.clone())))
            .map(|tool| PruneItem {
                kind: PruneItemKind::PackageManager(tool.kind),
                version: tool.version,
                path: tool.path,
                reclaimed_bytes: 0,
            });
        let mut removed = runtime_removals.chain(tool_removals).collect::<Vec<_>>();

        for item in &mut removed {
            if dry_run {
                item.reclaimed_bytes = self.storage.path_stats(&item.path)?.bytes;
                continue;
            }
            let outcome = match item.kind {
                PruneItemKind::Runtime(kind) => {
                    self.installer.uninstall_runtime(kind, &item.version)?
                }
                PruneItemKind::PackageManager(kind) => self
                    .installer
                    .uninstall_package_manager(kind, &item.version)?,
            };
            item.reclaimed_bytes = outcome.reclaimed_bytes;
        }

        Ok(PruneOutcome { removed, dry_run })
    }

    pub fn cache_stats(&self) -> Result<CacheStats, CoreError> {
        Ok(self.storage.cache_stats()?)
    }

    pub fn clean_cache(&self) -> Result<CacheCleanOutcome, CoreError> {
        Ok(self.installer.clean_cache()?)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneItemKind {
    Runtime(RuntimeKind),
    PackageManager(PackageManagerKind),
}

impl std::fmt::Display for PruneItemKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(kind) => kind.fmt(formatter),
            Self::PackageManager(kind) => kind.fmt(formatter),
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

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("no runtime requirement was found from {0}")]
    NoRuntimeRequirement(PathBuf),
    #[error("package manager {0} requires a Node.js runtime")]
    PackageManagerRequiresNode(PackageManagerRequest),
    #[error(
        "refusing to uninstall active {kind}@{version}; activate another version or pass --force"
    )]
    ActiveRuntimeRemoval { kind: RuntimeKind, version: Version },
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
            schema_version: jolter_config::CURRENT_SCHEMA_VERSION,
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

    #[test]
    fn prune_preserves_active_and_project_versions() {
        let project = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(CONFIG_FILE_NAME),
            r#"{"runtime":{"node":"24"}}"#,
        )
        .unwrap();
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        storage.ensure_layout().unwrap();
        for version in [
            semver::Version::new(20, 1, 0),
            semver::Version::new(22, 1, 0),
            semver::Version::new(24, 1, 0),
        ] {
            let executable = storage.runtime_executable(RuntimeKind::Node, &version);
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(executable, b"node").unwrap();
        }
        storage
            .activate(RuntimeKind::Node, &semver::Version::new(22, 1, 0))
            .unwrap();
        let jolter = Jolter::with_storage(storage.clone()).unwrap();

        let preview = jolter.prune(project.path(), 0, true).unwrap();
        assert_eq!(preview.removed.len(), 1);
        assert_eq!(preview.removed[0].version, semver::Version::new(20, 1, 0));
        assert!(preview.reclaimed_bytes() > 0);

        let applied = jolter.prune(project.path(), 0, false).unwrap();
        assert_eq!(applied.removed.len(), 1);
        assert!(
            !storage
                .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(20, 1, 0))
                .exists()
        );
        assert!(
            storage
                .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(22, 1, 0))
                .exists()
        );
        assert!(
            storage
                .runtime_version_dir(RuntimeKind::Node, &semver::Version::new(24, 1, 0))
                .exists()
        );
    }

    #[test]
    fn uninstall_and_cache_lifecycle_are_exposed_by_core() {
        let storage_temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(storage_temp.path());
        storage.ensure_layout().unwrap();
        let version = semver::Version::new(2, 1, 0);
        let executable = storage.runtime_executable(RuntimeKind::Deno, &version);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"deno").unwrap();
        let cache = storage.cache_dir().join("downloads").join("archive.zip");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"archive").unwrap();
        let jolter = Jolter::with_storage(storage).unwrap();

        assert!(jolter.cache_stats().unwrap().files > 0);
        assert!(
            jolter
                .uninstall_runtime(RuntimeKind::Deno, &version, false)
                .unwrap()
                .reclaimed_bytes
                > 0
        );
        assert!(!executable.exists());
        assert!(jolter.clean_cache().unwrap().removed_files > 0);
    }
}
