use std::{
    fs,
    path::{Path, PathBuf},
};

use jolter_config::{ProjectConfig, discover};
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
    let config_path = discover(&start);
    let config = config_path
        .as_deref()
        .map(ProjectConfig::from_path)
        .transpose()?;
    let project_root = config_path
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(&start)
        .to_path_buf();

    let runtime = match config.as_ref().and_then(runtime_from_config).transpose()? {
        Some(runtime) => Some(runtime),
        None => match resolve_node_file(&start, ".node-version", RequirementSource::NodeVersion)? {
            Some(runtime) => Some(runtime),
            None => resolve_node_file(&start, ".nvmrc", RequirementSource::Nvmrc)?,
        },
    };

    let tools = match config.as_ref().map(tools_from_config).transpose()? {
        Some(tools) if !tools.is_empty() => tools,
        _ => resolve_package_json(&start)?.into_iter().collect(),
    };

    Ok(ProjectResolution {
        root: project_root,
        runtime,
        tools,
    })
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
        .map(|(name, selector)| {
            ToolRequest::new(name.parse()?, selector).map(|request| ResolvedTool {
                request,
                source: RequirementSource::JolterConfig,
            })
        })
        .collect()
}

fn resolved_tool(
    name: &str,
    selector: &str,
    source: RequirementSource,
) -> Result<ResolvedTool, ToolRequestError> {
    Ok(ResolvedTool {
        request: ToolRequest::new(name.parse()?, selector)?,
        source,
    })
}

fn resolve_node_file(
    start: &Path,
    file_name: &str,
    source: RequirementSource,
) -> Result<Option<ResolvedRuntime>, ResolverError> {
    let Some(path) = find_upward(start, file_name) else {
        return Ok(None);
    };
    let selector = fs::read_to_string(&path)
        .map_err(|error| ResolverError::Read {
            path: path.clone(),
            source: error,
        })?
        .trim()
        .to_owned();
    let request = RuntimeRequest::new(RuntimeKind::Node, selector)?;
    Ok(Some(ResolvedRuntime { request, source }))
}

fn resolve_package_json(start: &Path) -> Result<Option<ResolvedTool>, ResolverError> {
    let Some(path) = find_upward(start, "package.json") else {
        return Ok(None);
    };
    let contents = fs::read_to_string(&path).map_err(|source| ResolverError::Read {
        path: path.clone(),
        source,
    })?;
    let package: Value =
        serde_json::from_str(&contents).map_err(|source| ResolverError::PackageJson {
            path: path.clone(),
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
    Ok(Some(resolved_tool(
        name,
        selector,
        RequirementSource::PackageJson,
    )?))
}

fn find_upward(start: &Path, file_name: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|directory| directory.join(file_name))
        .find(|candidate| candidate.is_file())
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
}
