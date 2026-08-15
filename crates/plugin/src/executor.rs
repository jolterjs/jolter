use std::path::Path;

use semver::Version;
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, bindgen},
};

use crate::{
    error::PluginError,
    manifest::{PluginPlatform, PluginTool, PluginToolRelease},
};

bindgen!({
    path: "wit",
    world: "jolter-plugin",
});

pub struct PluginExecutor {
    engine: Engine,
}

impl PluginExecutor {
    pub fn new() -> Result<Self, PluginError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        Ok(Self {
            engine: Engine::new(&config).map_err(PluginError::Wasm)?,
        })
    }

    pub fn list_tools(&self, plugin_path: &Path) -> Result<Vec<PluginTool>, PluginError> {
        let (instance, mut store) = self.instantiate(plugin_path)?;
        let tools = instance
            .call_list_tools(&mut store)
            .map_err(PluginError::Wasm)?;
        Ok(tools
            .into_iter()
            .map(|tool| PluginTool {
                name: tool.name,
                commands: tool.commands,
            })
            .collect())
    }

    pub fn resolve_tool(
        &self,
        plugin_path: &Path,
        tool: &str,
        selector: &str,
        platform: PluginPlatform,
    ) -> Result<PluginToolRelease, PluginError> {
        let (instance, mut store) = self.instantiate(plugin_path)?;
        let release = instance
            .call_resolve_tool(
                &mut store,
                tool,
                selector,
                &Platform {
                    os: platform.os,
                    arch: platform.arch,
                },
            )
            .map_err(PluginError::Wasm)?;
        let version =
            Version::parse(release.version.trim_start_matches('v')).map_err(|source| {
                PluginError::InvalidPluginToolVersion {
                    value: release.version.clone(),
                    source,
                }
            })?;
        Ok(PluginToolRelease {
            version,
            url: release.url,
            sha256: release.sha256,
            archive_format: release.archive_format,
            strip_components: usize::try_from(release.strip_components).unwrap_or(usize::MAX),
            commands: release.commands,
        })
    }

    pub fn validate_installed(
        &self,
        plugin_path: &Path,
        tool: &str,
        version: &Version,
        root: &Path,
    ) -> Result<bool, PluginError> {
        let (instance, mut store) = self.instantiate(plugin_path)?;
        instance
            .call_validate_installed(
                &mut store,
                tool,
                &version.to_string(),
                &root.to_string_lossy(),
            )
            .map_err(PluginError::Wasm)
    }

    fn instantiate(&self, plugin_path: &Path) -> Result<(JolterPlugin, Store<()>), PluginError> {
        let component =
            Component::from_file(&self.engine, plugin_path).map_err(PluginError::Wasm)?;
        let linker = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());
        let instance = JolterPlugin::instantiate(&mut store, &component, &linker)
            .map_err(PluginError::Wasm)?;
        Ok((instance, store))
    }
}
