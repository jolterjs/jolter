use std::{
    fs,
    path::{Path, PathBuf},
};

use jolter_config::ProjectConfig;
use jolter_runtime::{
    RuntimeKind, RuntimeRequest, RuntimeRequestError, ToolRequest, ToolRequestError,
};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectResolution {
    pub root: PathBuf,
    pub runtime: Option<ResolvedRuntime>,
    pub tools: Vec<ResolvedTool>,
    pub plugin_tools: Vec<ResolvedPluginTool>,
    pub plugins: Vec<ResolvedPlugin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuntime {
    pub request: RuntimeRequest,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTool {
    pub request: ToolRequest,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPluginTool {
    pub name: String,
    pub selector: String,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlugin {
    pub name: String,
    pub selector: String,
    pub source: RequirementSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementSource {
    JolterConfig,
    PackageJson,
    NodeVersion,
    Nvmrc,
}

pub fn resolve(start: &Path) -> Result<ProjectResolution, ResolverError> {
    let start = start
        .canonicalize()
        .map_err(|source| ResolverError::Canonicalize {
            path: start.to_path_buf(),
            source,
        })?;

    let discovered = discover_project_files(&start);

    let config = discovered
        .config_path
        .as_deref()
        .map(ProjectConfig::from_path)
        .transpose()?;
    let project_root = discovered
        .config_path
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(&start)
        .to_path_buf();

    let runtime = match config.as_ref().and_then(runtime_from_config).transpose()? {
        Some(runtime) => Some(runtime),
        None => match discovered.node_version_path {
            Some((path, source)) => parse_node_file(&path, source)?,
            None => None,
        },
    };

    let tools = match config.as_ref().map(tools_from_config).transpose()? {
        Some(tools) if !tools.is_empty() => tools,
        _ => match discovered.package_json_path {
            Some(path) => parse_package_json(&path)?.into_iter().collect(),
            None => Vec::new(),
        },
    };
    let plugin_tools = config
        .as_ref()
        .map_or_else(Vec::new, plugin_tools_from_config);
    let plugins = config.as_ref().map_or_else(Vec::new, plugins_from_config);

    Ok(ProjectResolution {
        root: project_root,
        runtime,
        tools,
        plugin_tools,
        plugins,
    })
}

struct DiscoveredFiles {
    config_path: Option<PathBuf>,
    node_version_path: Option<(PathBuf, RequirementSource)>,
    package_json_path: Option<PathBuf>,
}

fn discover_project_files(start: &Path) -> DiscoveredFiles {
    let mut config_path = None;
    let mut node_version_path = None;
    let mut package_json_path = None;

    for directory in start.ancestors() {
        if config_path.is_none() {
            let candidate = directory.join(jolter_config::CONFIG_FILE_NAME);
            if candidate.is_file() {
                config_path = Some(candidate);
            }
        }
        if node_version_path.is_none() {
            let node_ver = directory.join(".node-version");
            if node_ver.is_file() {
                node_version_path = Some((node_ver, RequirementSource::NodeVersion));
            } else {
                let nvmrc = directory.join(".nvmrc");
                if nvmrc.is_file() {
                    node_version_path = Some((nvmrc, RequirementSource::Nvmrc));
                }
            }
        }
        if package_json_path.is_none() {
            let pkg = directory.join("package.json");
            if pkg.is_file() {
                package_json_path = Some(pkg);
            }
        }
        if config_path.is_some() && node_version_path.is_some() && package_json_path.is_some() {
            break;
        }
    }

    DiscoveredFiles {
        config_path,
        node_version_path,
        package_json_path,
    }
}

fn runtime_from_config(
    config: &ProjectConfig,
) -> Option<Result<ResolvedRuntime, RuntimeRequestError>> {
    config.runtime.entries().next().map(|(name, selector)| {
        RuntimeRequest::new(name.parse()?, selector).map(|request| ResolvedRuntime {
            request,
            source: RequirementSource::JolterConfig,
        })
    })
}

fn tools_from_config(config: &ProjectConfig) -> Result<Vec<ResolvedTool>, ToolRequestError> {
    config
        .tools
        .iter()
        .filter(|(name, _)| name.parse::<jolter_runtime::ToolKind>().is_ok())
        .map(|(name, selector)| {
            ToolRequest::new(name.parse()?, selector).map(|request| ResolvedTool {
                request,
                source: RequirementSource::JolterConfig,
            })
        })
        .collect()
}

fn plugin_tools_from_config(config: &ProjectConfig) -> Vec<ResolvedPluginTool> {
    if config.schema_version < 2 {
        return Vec::new();
    }
    config
        .tools
        .iter()
        .filter(|(name, _)| name.parse::<jolter_runtime::ToolKind>().is_err())
        .map(|(name, selector)| ResolvedPluginTool {
            name: name.clone(),
            selector: selector.clone(),
            source: RequirementSource::JolterConfig,
        })
        .collect()
}

fn plugins_from_config(config: &ProjectConfig) -> Vec<ResolvedPlugin> {
    if config.schema_version < 2 {
        return Vec::new();
    }
    config
        .plugins
        .iter()
        .map(|(name, selector)| ResolvedPlugin {
            name: name.clone(),
            selector: selector.clone(),
            source: RequirementSource::JolterConfig,
        })
        .collect()
}

fn parse_node_file(
    path: &Path,
    source: RequirementSource,
) -> Result<Option<ResolvedRuntime>, ResolverError> {
    let selector = fs::read_to_string(path)
        .map_err(|error| ResolverError::Read {
            path: path.to_path_buf(),
            source: error,
        })?
        .trim()
        .to_owned();
    let request = RuntimeRequest::new(RuntimeKind::Node, selector)?;
    Ok(Some(ResolvedRuntime { request, source }))
}

fn parse_package_json(path: &Path) -> Result<Option<ResolvedTool>, ResolverError> {
    let contents = fs::read_to_string(path).map_err(|source| ResolverError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let package: Value =
        serde_json::from_str(&contents).map_err(|source| ResolverError::PackageJson {
            path: path.to_path_buf(),
            source,
        })?;
    let Some(value) = package.get("packageManager").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some((name, selector)) = value.rsplit_once('@') else {
        return Err(ResolverError::InvalidPackageManager(value.to_owned()));
    };
    if name.is_empty() || selector.is_empty() {
        return Err(ResolverError::InvalidPackageManager(value.to_owned()));
    }
    let Ok(kind) = name.parse::<jolter_runtime::ToolKind>() else {
        return Ok(None);
    };
    Ok(Some(ResolvedTool {
        request: ToolRequest::new(kind, selector)?,
        source: RequirementSource::PackageJson,
    }))
}

#[derive(Debug, Error)]
pub enum ResolverError {
    #[error(transparent)]
    Config(#[from] jolter_config::ConfigError),
    #[error(transparent)]
    Runtime(#[from] RuntimeRequestError),
    #[error(transparent)]
    Tool(#[from] ToolRequestError),
    #[error("failed to resolve path {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid package.json at {path}: {source}")]
    PackageJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid packageManager value `{0}`; expected a value such as pnpm@10.0.0")]
    InvalidPackageManager(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jolter_config_takes_runtime_priority() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("jolter.json"),
            r#"{"runtime":{"node":"24.x"}}"#,
        )
        .unwrap();
        fs::write(temp.path().join(".node-version"), "22").unwrap();

        let resolution = resolve(temp.path()).unwrap();
        let runtime = resolution.runtime.unwrap();
        assert_eq!(runtime.request.selector, "24.x");
        assert_eq!(runtime.source, RequirementSource::JolterConfig);
    }

    #[test]
    fn resolves_package_json_tool_and_node_version_independently() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"pnpm@10.12.1"}"#,
        )
        .unwrap();
        fs::write(temp.path().join(".nvmrc"), "22").unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(resolution.runtime.unwrap().request.to_string(), "node@22");
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@10.12.1");
    }

    #[test]
    fn resolves_a_corepack_hashed_tool() {
        let temp = tempfile::tempdir().unwrap();
        let hash = "a".repeat(56);
        fs::write(
            temp.path().join("package.json"),
            format!(r#"{{"packageManager":"pnpm@10.12.1+sha224.{hash}"}}"#),
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();

        assert_eq!(
            resolution.tools[0].request.to_string(),
            format!("pnpm@10.12.1+sha224.{hash}")
        );
    }

    #[test]
    fn resolves_multiple_tools_from_jolter_config() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("jolter.json"),
            r#"{"tools":{"pnpm":"10","yarn":"4"}}"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        let requests = resolution
            .tools
            .iter()
            .map(|tool| tool.request.to_string())
            .collect::<Vec<_>>();

        assert_eq!(requests, ["pnpm@10", "yarn@4"]);
    }

    #[test]
    fn lower_priority_nvmrc_does_not_override_or_invalidate_node_version() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".node-version"), "24").unwrap();
        fs::write(temp.path().join(".nvmrc"), "not a valid selector @").unwrap();

        let resolution = resolve(temp.path()).unwrap();

        assert_eq!(resolution.runtime.unwrap().request.to_string(), "node@24");
    }

    #[test]
    fn ignores_unmanaged_package_manager_tools_in_package_json() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"bun@1.3.14"}"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert!(resolution.tools.is_empty());
    }

    #[test]
    fn jolter_config_with_no_tools_ignores_unmanaged_package_manager() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("jolter.json"),
            r#"{"runtime":{"node":"24.x"}}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"bun@1.3.14"}"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(resolution.runtime.unwrap().request.to_string(), "node@24.x");
        assert!(resolution.tools.is_empty());
    }
}
