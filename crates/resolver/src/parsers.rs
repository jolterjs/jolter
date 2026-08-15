use std::{fs, path::Path};

use jolter_config::ProjectConfig;
use jolter_runtime::{
    RuntimeKind, RuntimeRequest, RuntimeRequestError, ToolRequest, ToolRequestError,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    dev_engines::DevEngines,
    error::ResolverError,
    types::{RequirementSource, ResolvedPlugin, ResolvedPluginTool, ResolvedRuntime, ResolvedTool},
};

pub(crate) fn runtime_from_config(
    config: &ProjectConfig,
) -> Option<Result<ResolvedRuntime, RuntimeRequestError>> {
    config.runtime.entries().next().map(|(name, selector)| {
        RuntimeRequest::new(name.parse()?, selector).map(|request| ResolvedRuntime {
            request,
            source: RequirementSource::JolterConfig,
        })
    })
}

pub(crate) fn tools_from_config(
    config: &ProjectConfig,
) -> Result<Vec<ResolvedTool>, ToolRequestError> {
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

pub(crate) fn plugin_tools_from_config(config: &ProjectConfig) -> Vec<ResolvedPluginTool> {
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

pub(crate) fn plugins_from_config(config: &ProjectConfig) -> Vec<ResolvedPlugin> {
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

pub(crate) fn parse_node_file(
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

pub(crate) fn normalize_dev_engine_selector(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return "*".to_owned();
    }
    if trimmed.eq_ignore_ascii_case("latest") {
        return "latest".to_owned();
    }
    if trimmed.eq_ignore_ascii_case("lts") {
        return "lts".to_owned();
    }

    let first_clause = trimmed.split("||").next().unwrap_or(trimmed).trim();
    let cleaned = first_clause
        .trim_start_matches('^')
        .trim_start_matches('~')
        .trim_start_matches(">=")
        .trim_start_matches("<=")
        .trim_start_matches('>')
        .trim_start_matches('<')
        .trim_start_matches('=')
        .trim_start_matches('v')
        .trim();

    let token = cleaned.split_whitespace().next().unwrap_or(cleaned);
    let valid_selector: String = token
        .chars()
        .take_while(|c| {
            c.is_ascii_alphanumeric()
                || *c == '.'
                || *c == 'x'
                || *c == 'X'
                || *c == '*'
                || *c == '+'
                || *c == '-'
        })
        .collect();

    if valid_selector.is_empty() {
        "*".to_owned()
    } else {
        valid_selector
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParsedPackageJson {
    #[serde(default)]
    dev_engines: Option<DevEngines>,
    #[serde(default)]
    package_manager: Option<Value>,
}

pub(crate) type PackageJsonParseResult = (
    Option<ResolvedRuntime>,
    Vec<ResolvedTool>,
    Option<DevEngines>,
);

pub(crate) fn parse_package_json(path: &Path) -> Result<PackageJsonParseResult, ResolverError> {
    let contents = fs::read_to_string(path).map_err(|source| ResolverError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: ParsedPackageJson =
        serde_json::from_str(&contents).map_err(|source| ResolverError::PackageJson {
            path: path.to_path_buf(),
            source,
        })?;

    let mut resolved_runtime = None;
    let mut resolved_tools = Vec::new();

    if let Some(dev_engines) = &parsed.dev_engines {
        for item in &dev_engines.runtime {
            let (name, selector) = item.parsed_name_and_selector();
            if let Ok(kind) = name.parse::<RuntimeKind>() {
                let sel =
                    selector.map_or_else(|| "*".to_owned(), |s| normalize_dev_engine_selector(&s));
                if let Ok(request) = RuntimeRequest::new(kind, sel) {
                    resolved_runtime = Some(ResolvedRuntime {
                        request,
                        source: RequirementSource::PackageJson,
                    });
                    break;
                }
            }
        }

        for item in &dev_engines.package_manager {
            let (name, selector) = item.parsed_name_and_selector();
            if let Ok(kind) = name.parse::<jolter_runtime::ToolKind>() {
                let sel =
                    selector.map_or_else(|| "*".to_owned(), |s| normalize_dev_engine_selector(&s));
                if let Ok(request) = ToolRequest::new(kind, sel) {
                    resolved_tools.push(ResolvedTool {
                        request,
                        source: RequirementSource::PackageJson,
                    });
                }
            }
        }
    }

    if resolved_tools.is_empty() {
        if let Some(value) = parsed.package_manager.as_ref().and_then(Value::as_str) {
            let Some((name, selector)) = value.rsplit_once('@') else {
                return Err(ResolverError::InvalidPackageManager(value.to_owned()));
            };
            if name.is_empty() || selector.is_empty() {
                return Err(ResolverError::InvalidPackageManager(value.to_owned()));
            }
            if let Ok(kind) = name.parse::<jolter_runtime::ToolKind>() {
                resolved_tools.push(ResolvedTool {
                    request: ToolRequest::new(kind, selector)?,
                    source: RequirementSource::PackageJson,
                });
            }
        }
    }

    Ok((resolved_runtime, resolved_tools, parsed.dev_engines))
}
