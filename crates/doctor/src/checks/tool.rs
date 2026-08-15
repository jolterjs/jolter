use std::{fs, process::Command};

use jolter_resolver::{ProjectResolution, ResolvedTool};
use jolter_runtime::RuntimeKind;
use jolter_storage::{InstalledRuntime, InstalledTool, Storage};
use nodejs_semver::{Range as NodeRange, Version as NodeVersion};
use semver::Version;
use serde::Deserialize;

use crate::{
    checks::runtime::{ManifestRead, read_manifest, valid_manifest_artifact},
    error::DoctorError,
    probe::version_probe_check,
    types::Check,
};

pub fn tool_checks(
    storage: &Storage,
    resolution: &ProjectResolution,
    runtime: Option<&InstalledRuntime>,
    checks: &mut Vec<Check>,
) -> Result<(), DoctorError> {
    if resolution.tools.is_empty() {
        checks.push(Check::warning(
            "tools",
            "no tool requirement was found",
            "add tools to jolter.json when deterministic tooling is needed",
        ));
        return Ok(());
    }
    for resolved in &resolution.tools {
        let Some(runtime) = runtime.filter(|runtime| runtime.kind == RuntimeKind::Node) else {
            checks.push(Check::fail(
                "tool",
                format!(
                    "{}@{} requires an installed Node.js runtime",
                    resolved.request.kind, resolved.request.selector
                ),
                "configure a Node.js runtime and run `jolter sync`",
            ));
            continue;
        };
        let Some(tool) = storage.find_matching_tool(&resolved.request)? else {
            checks.push(Check::fail(
                "tool",
                format!(
                    "{} is configured but no matching managed installation exists",
                    resolved.request
                ),
                "run `jolter sync` to install the required tool",
            ));
            continue;
        };

        checks.push(Check::pass(
            "tool",
            format!(
                "{} is satisfied by {}@{}",
                resolved.request, tool.kind, tool.version
            ),
        ));
        checks.push(tool_manifest_check(&tool));
        checks.push(tool_engine_check(&tool, &runtime.version));
        checks.push(version_probe_check(
            "tool version",
            tool_probe_command(storage, runtime, &tool, resolved),
            &tool.version,
            "run `jolter repair` to replace the tool installation",
        ));
    }
    Ok(())
}

#[must_use]
pub fn tool_manifest_check(tool: &InstalledTool) -> Check {
    let path = tool.path.join(".jolter-tool.json");
    let manifest: ToolManifest = match read_manifest(&path) {
        Ok(manifest) => manifest,
        Err(ManifestRead::Missing) => {
            return Check::warning(
                "tool manifest",
                format!("installation manifest is missing at {}", path.display()),
                "run `jolter repair` to recreate a verified installation",
            );
        }
        Err(ManifestRead::Invalid(error)) => {
            return Check::fail(
                "tool manifest",
                format!(
                    "invalid installation manifest at {}: {error}",
                    path.display()
                ),
                "run `jolter repair` to replace the installation",
            );
        }
    };
    if manifest.tool != tool.kind.to_string()
        || manifest.version != tool.version.to_string()
        || !valid_manifest_artifact(&manifest.artifact_url, &manifest.integrity)
    {
        return Check::fail(
            "tool manifest",
            format!(
                "manifest identity or integrity metadata does not match {}@{}",
                tool.kind, tool.version
            ),
            "run `jolter repair` to replace the installation",
        );
    }
    Check::pass(
        "tool manifest",
        format!("verified metadata is present at {}", path.display()),
    )
}

#[must_use]
pub fn tool_engine_check(tool: &InstalledTool, node_version: &Version) -> Check {
    let path = tool.path.join("package.json");
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Check::warning(
                "Node.js compatibility",
                format!("package metadata is missing at {}", path.display()),
                "run `jolter repair` to recreate the managed tool",
            );
        }
        Err(error) => {
            return Check::fail(
                "Node.js compatibility",
                format!("could not read {}: {error}", path.display()),
                "fix file permissions or run `jolter repair`",
            );
        }
    };
    let metadata: PackageMetadata = match serde_json::from_str(&contents) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Check::fail(
                "Node.js compatibility",
                format!("invalid package metadata at {}: {error}", path.display()),
                "run `jolter repair` to replace the tool",
            );
        }
    };
    let Some(requirement) = metadata
        .engines
        .node
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Check::pass(
            "Node.js compatibility",
            format!(
                "{}@{} declares no Node.js restriction",
                tool.kind, tool.version
            ),
        );
    };
    let range = match NodeRange::parse(requirement) {
        Ok(range) => range,
        Err(error) => {
            return Check::fail(
                "Node.js compatibility",
                format!(
                    "{}@{} declares invalid Node.js range `{requirement}`: {error}",
                    tool.kind, tool.version
                ),
                "run `jolter repair`; if the metadata is unchanged, report it upstream",
            );
        }
    };
    let node = NodeVersion::from((node_version.major, node_version.minor, node_version.patch));
    if range.satisfies(&node) {
        Check::pass(
            "Node.js compatibility",
            format!(
                "{}@{} supports node@{} via `{requirement}`",
                tool.kind, tool.version, node_version
            ),
        )
    } else {
        Check::fail(
            "Node.js compatibility",
            format!(
                "{}@{} requires Node.js `{requirement}`, but node@{} is selected",
                tool.kind, tool.version, node_version
            ),
            "pin a compatible Node.js or tool version, then run `jolter sync`",
        )
    }
}

#[must_use]
pub fn tool_probe_command(
    storage: &Storage,
    runtime: &InstalledRuntime,
    tool: &InstalledTool,
    requirement: &ResolvedTool,
) -> Command {
    let mut command = Command::new(runtime.executable());
    if let Some(entrypoint) = storage.tool_entrypoint(
        tool.kind,
        &tool.version,
        &requirement.request.kind.to_string(),
    ) {
        command.arg(entrypoint);
    }
    command.arg("--version");
    command
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolManifest {
    #[serde(alias = "packageManager")]
    tool: String,
    version: String,
    artifact_url: String,
    integrity: String,
}

#[derive(Debug, Default, Deserialize)]
struct PackageMetadata {
    #[serde(default)]
    engines: PackageEngines,
}

#[derive(Debug, Default, Deserialize)]
struct PackageEngines {
    node: Option<String>,
}
