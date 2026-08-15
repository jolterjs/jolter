pub mod error;
pub mod plugins;
pub mod prune;
pub mod runtimes;
pub mod sync;
pub mod tools;
pub mod types;

#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use jolter_plugin::{PluginExecutor, PluginManager};
use jolter_storage::Storage;

pub use error::CoreError;
pub use jolter_installer::{
    Installer, NoProgressReporter, ProgressAction, ProgressEvent, ProgressReporter, ReleaseChannel,
    SelfUpgradeOutcome,
};
pub use types::{
    PluginAction, PluginToolAction, PruneItem, PruneItemKind, PruneOutcome, RuntimeAction,
    SyncOutcome, ToolAction,
};

pub struct Jolter {
    storage: Storage,
    installer: Mutex<Option<Installer>>,
    plugins: Mutex<Option<PluginManager>>,
    plugin_executor: Mutex<Option<PluginExecutor>>,
    reporter: Arc<dyn ProgressReporter>,
}

impl Jolter {
    pub fn discover() -> Result<Self, CoreError> {
        Self::with_storage(Storage::discover()?)
    }

    pub fn discover_with_reporter(reporter: Arc<dyn ProgressReporter>) -> Result<Self, CoreError> {
        Self::with_storage_and_reporter(Storage::discover()?, reporter)
    }

    pub fn with_storage(storage: Storage) -> Result<Self, CoreError> {
        Self::with_storage_and_reporter(storage, Arc::new(NoProgressReporter))
    }

    pub fn with_storage_and_reporter(
        storage: Storage,
        reporter: Arc<dyn ProgressReporter>,
    ) -> Result<Self, CoreError> {
        Ok(Self {
            storage,
            installer: Mutex::new(None),
            plugins: Mutex::new(None),
            plugin_executor: Mutex::new(None),
            reporter,
        })
    }

    #[must_use]
    pub const fn storage(&self) -> &Storage {
        &self.storage
    }

    pub(crate) fn with_installer<T>(
        &self,
        action: impl FnOnce(&Installer) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut installer = self.installer.lock().unwrap();
        if installer.is_none() {
            *installer = Some(Installer::new_with_reporter(
                self.storage.clone(),
                self.reporter.clone(),
            )?);
        }
        action(installer.as_ref().expect("installer initialized above"))
    }

    pub(crate) fn with_plugins<T>(
        &self,
        action: impl FnOnce(&PluginManager) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut plugins = self.plugins.lock().unwrap();
        if plugins.is_none() {
            *plugins = Some(PluginManager::new(self.storage.clone())?);
        }
        action(plugins.as_ref().expect("plugin manager initialized above"))
    }

    pub(crate) fn with_plugin_executor<T>(
        &self,
        action: impl FnOnce(&PluginExecutor) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut executor = self.plugin_executor.lock().unwrap();
        if executor.is_none() {
            *executor = Some(PluginExecutor::new()?);
        }
        action(
            executor
                .as_ref()
                .expect("plugin executor initialized above"),
        )
    }

    pub(crate) fn report(&self, action: ProgressAction, target: &str) {
        self.reporter
            .report(ProgressEvent::Stage { action, target });
    }
}
