use std::path::{Path, PathBuf};

use crate::types::RequirementSource;

pub(crate) struct DiscoveredFiles {
    pub config: Option<PathBuf>,
    pub node_version: Option<(PathBuf, RequirementSource)>,
    pub package_json: Option<PathBuf>,
}

pub(crate) fn discover_project_files(start: &Path) -> DiscoveredFiles {
    let mut config = None;
    let mut node_version = None;
    let mut package_json = None;

    for directory in start.ancestors() {
        if config.is_none() {
            let candidate = directory.join(jolter_config::CONFIG_FILE_NAME);
            if candidate.is_file() {
                config = Some(candidate);
            }
        }
        if node_version.is_none() {
            let node_ver = directory.join(".node-version");
            if node_ver.is_file() {
                node_version = Some((node_ver, RequirementSource::NodeVersion));
            } else {
                let nvmrc = directory.join(".nvmrc");
                if nvmrc.is_file() {
                    node_version = Some((nvmrc, RequirementSource::Nvmrc));
                }
            }
        }
        if package_json.is_none() {
            let pkg = directory.join("package.json");
            if pkg.is_file() {
                package_json = Some(pkg);
            }
        }
        if config.is_some() && node_version.is_some() && package_json.is_some() {
            break;
        }
    }

    DiscoveredFiles {
        config,
        node_version,
        package_json,
    }
}
