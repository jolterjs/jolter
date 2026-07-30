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

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DevEngines {
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub runtime: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub package_manager: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub cpu: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub os: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub libc: Vec<DevEngineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevEngineItem {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_fail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevEngineOnFail {
    Error,
    Warn,
    Ignore,
    Download,
}

impl DevEngineItem {
    #[must_use]
    pub fn parsed_name_and_selector(&self) -> (String, Option<String>) {
        if let Some((name, selector)) = self.name.rsplit_once('@') {
            if !name.is_empty() && !selector.is_empty() {
                return (name.to_owned(), Some(selector.to_owned()));
            }
        }
        (self.name.clone(), self.version.clone())
    }

    pub fn on_fail_mode(&self) -> DevEngineOnFail {
        match self
            .on_fail
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("warn") => DevEngineOnFail::Warn,
            Some("ignore") => DevEngineOnFail::Ignore,
            Some("download") => DevEngineOnFail::Download,
            _ => DevEngineOnFail::Error,
        }
    }
}

fn deserialize_one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        One(T),
        Many(Vec<T>),
    }

    Option::<OneOrMany<T>>::deserialize(deserializer).map(|opt| match opt {
        Some(OneOrMany::One(val)) => vec![val],
        Some(OneOrMany::Many(vec)) => vec,
        None => Vec::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectResolution {
    pub root: PathBuf,
    pub runtime: Option<ResolvedRuntime>,
    pub tools: Vec<ResolvedTool>,
    pub plugin_tools: Vec<ResolvedPluginTool>,
    pub plugins: Vec<ResolvedPlugin>,
    pub dev_engines: Option<DevEngines>,
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
        .config
        .as_deref()
        .map(ProjectConfig::from_path)
        .transpose()?;
    let project_root = discovered
        .config
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(&start)
        .to_path_buf();

    let (pkg_runtime, pkg_tools, dev_engines) = match discovered.package_json.as_deref() {
        Some(path) => parse_package_json(path)?,
        None => (None, Vec::new(), None),
    };

    let runtime = match config.as_ref().and_then(runtime_from_config).transpose()? {
        Some(runtime) => Some(runtime),
        None => match discovered.node_version {
            Some((path, source)) => parse_node_file(&path, source)?,
            None => pkg_runtime,
        },
    };

    let tools = match config.as_ref().map(tools_from_config).transpose()? {
        Some(tools) if !tools.is_empty() => tools,
        _ => pkg_tools,
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
        dev_engines,
    })
}

struct DiscoveredFiles {
    config: Option<PathBuf>,
    node_version: Option<(PathBuf, RequirementSource)>,
    package_json: Option<PathBuf>,
}

fn discover_project_files(start: &Path) -> DiscoveredFiles {
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

fn normalize_dev_engine_selector(raw: &str) -> String {
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

type PackageJsonParseResult = (
    Option<ResolvedRuntime>,
    Vec<ResolvedTool>,
    Option<DevEngines>,
);

fn parse_package_json(path: &Path) -> Result<PackageJsonParseResult, ResolverError> {
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
    fn resolves_dev_engines_runtime_and_package_manager() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{
                "devEngines": {
                    "runtime": { "name": "node", "version": "^20.0.0", "onFail": "error" },
                    "packageManager": { "name": "pnpm", "version": "^11.17.0", "onFail": "download" }
                }
            }"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        let runtime = resolution.runtime.unwrap();
        assert_eq!(runtime.request.to_string(), "node@20.0.0");
        assert_eq!(runtime.source, RequirementSource::PackageJson);

        assert_eq!(resolution.tools.len(), 1);
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@11.17.0");
        assert_eq!(resolution.tools[0].source, RequirementSource::PackageJson);

        let dev_engines = resolution.dev_engines.unwrap();
        assert_eq!(dev_engines.runtime.len(), 1);
        assert_eq!(
            dev_engines.runtime[0].on_fail_mode(),
            DevEngineOnFail::Error
        );
        assert_eq!(dev_engines.package_manager.len(), 1);
        assert_eq!(
            dev_engines.package_manager[0].on_fail_mode(),
            DevEngineOnFail::Download
        );
    }

    #[test]
    fn resolves_dev_engines_array_and_cpu_os() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{
                "devEngines": {
                    "runtime": [{ "name": "node", "version": "22.0.0" }],
                    "packageManager": [
                        { "name": "pnpm", "version": "10.12.1" },
                        { "name": "yarn", "version": "4.0.0" }
                    ],
                    "cpu": { "name": "x64" },
                    "os": [{ "name": "darwin" }, { "name": "linux" }, { "name": "win32" }]
                }
            }"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(
            resolution.runtime.unwrap().request.to_string(),
            "node@22.0.0"
        );
        assert_eq!(resolution.tools.len(), 2);
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@10.12.1");
        assert_eq!(resolution.tools[1].request.to_string(), "yarn@4.0.0");

        let dev_engines = resolution.dev_engines.unwrap();
        assert_eq!(dev_engines.cpu.len(), 1);
        assert_eq!(dev_engines.cpu[0].name, "x64");
        assert_eq!(dev_engines.os.len(), 3);
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

    #[test]
    fn resolves_dev_engines_semver_range_expression() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{
                "devEngines": {
                    "runtime": { "name": "node", "version": "^20.0.0 || >=22.0.0" },
                    "packageManager": { "name": "pnpm", "version": "^10.0.0" }
                }
            }"#,
        )
        .unwrap();

        let resolution = resolve(temp.path()).unwrap();
        assert_eq!(
            resolution.runtime.unwrap().request.to_string(),
            "node@20.0.0"
        );
        assert_eq!(resolution.tools[0].request.to_string(), "pnpm@10.0.0");
    }

    #[test]
    fn tests_resolver_error_display() {
        let err = ResolverError::Read {
            path: PathBuf::from("/read"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert!(err.to_string().contains("failed to read /read"));

        let err = ResolverError::PackageJson {
            path: PathBuf::from("/pj"),
            source: serde_json::from_str::<serde_json::Value>("bad json").unwrap_err(),
        };
        assert!(err.to_string().contains("invalid package.json at"));
    }
}
