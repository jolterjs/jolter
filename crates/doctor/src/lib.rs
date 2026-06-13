use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use jolter_resolver::{ProjectResolution, ResolvedPackageManager, resolve};
use jolter_runtime::RuntimeKind;
use jolter_shim::SHIM_COMMANDS;
use jolter_storage::{InstalledRuntime, InstalledTool, Storage};
use nodejs_semver::{Range as NodeRange, Version as NodeVersion};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROBE_OUTPUT: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub status: CheckStatus,
    pub name: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl Check {
    fn pass(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Pass,
            name,
            message: message.into(),
            remediation: None,
        }
    }

    fn warning(
        name: &'static str,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            status: CheckStatus::Warning,
            name,
            message: message.into(),
            remediation: Some(remediation.into()),
        }
    }

    fn fail(
        name: &'static str,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            status: CheckStatus::Fail,
            name,
            message: message.into(),
            remediation: Some(remediation.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.checks
            .iter()
            .all(|check| check.status != CheckStatus::Fail)
    }
}

pub fn examine(project: &Path, storage: &Storage) -> Result<Report, DoctorError> {
    let mut checks = vec![
        Check::pass("storage", format!("using {}", storage.root().display())),
        storage_write_check(storage),
        platform_check(),
    ];
    let resolution = match resolve(project) {
        Ok(resolution) => {
            checks.push(Check::pass(
                "configuration",
                format!(
                    "project requirements resolved from {}",
                    resolution.root.display()
                ),
            ));
            Some(resolution)
        }
        Err(error) => {
            checks.push(Check::fail(
                "configuration",
                error.to_string(),
                "fix the project requirement file reported above, then rerun `jolter doctor`",
            ));
            None
        }
    };

    if let Some(resolution) = resolution.as_ref() {
        let matching_runtime = runtime_checks(storage, resolution, &mut checks)?;
        package_manager_checks(storage, resolution, matching_runtime.as_ref(), &mut checks)?;
    } else {
        checks.push(Check::warning(
            "runtime",
            "runtime health was not evaluated because project configuration is invalid",
            "fix the configuration check first",
        ));
        checks.push(Check::warning(
            "package manager",
            "package manager health was not evaluated because project configuration is invalid",
            "fix the configuration check first",
        ));
    }

    checks.push(shim_check(storage));
    checks.push(path_check(storage));
    checks.push(path_conflict_check(storage));
    checks.push(cache_check(storage)?);
    checks.push(network_environment_check());

    Ok(Report { checks })
}

fn runtime_checks(
    storage: &Storage,
    resolution: &ProjectResolution,
    checks: &mut Vec<Check>,
) -> Result<Option<InstalledRuntime>, DoctorError> {
    let Some(runtime) = &resolution.runtime else {
        checks.push(Check::warning(
            "runtime",
            "no project runtime requirement was found",
            "run `jolter pin node@<version>` or add a supported version file",
        ));
        return Ok(None);
    };
    let matching = storage.find_matching(&runtime.request)?;
    let Some(candidate) = matching else {
        checks.push(Check::fail(
            "runtime",
            format!(
                "{} is required but no complete installation was found",
                runtime.request
            ),
            "run `jolter sync` to install the required runtime",
        ));
        return Ok(None);
    };

    checks.push(Check::pass(
        "runtime",
        format!(
            "{} is satisfied by {}@{}",
            runtime.request, candidate.kind, candidate.version
        ),
    ));
    checks.push(runtime_manifest_check(&candidate));
    checks.push(runtime_permission_check(&candidate));
    checks.push(version_probe_check(
        "runtime version",
        runtime_probe_command(&candidate),
        &candidate.version,
        "run `jolter repair` to replace the runtime installation",
    ));
    Ok(Some(candidate))
}

fn package_manager_checks(
    storage: &Storage,
    resolution: &ProjectResolution,
    runtime: Option<&InstalledRuntime>,
    checks: &mut Vec<Check>,
) -> Result<(), DoctorError> {
    let Some(package_manager) = resolution.package_manager.as_ref() else {
        checks.push(Check::warning(
            "package manager",
            "no package manager requirement was found",
            "add packageManager to jolter.json or package.json when deterministic tooling is needed",
        ));
        return Ok(());
    };
    let Some(runtime) = runtime.filter(|runtime| runtime.kind == RuntimeKind::Node) else {
        checks.push(Check::fail(
            "package manager",
            format!(
                "{}@{} requires an installed Node.js runtime",
                package_manager.request.kind, package_manager.request.selector
            ),
            "configure a Node.js runtime and run `jolter sync`",
        ));
        return Ok(());
    };
    let Some(tool) = storage.find_matching_tool(&package_manager.request)? else {
        checks.push(Check::fail(
            "package manager",
            format!(
                "{} is configured but no matching managed installation exists",
                package_manager.request
            ),
            "run `jolter sync` to install the required package manager",
        ));
        return Ok(());
    };

    checks.push(Check::pass(
        "package manager",
        format!(
            "{} is satisfied by {}@{}",
            package_manager.request, tool.kind, tool.version
        ),
    ));
    checks.push(tool_manifest_check(&tool));
    checks.push(package_manager_engine_check(&tool, &runtime.version));
    checks.push(version_probe_check(
        "package manager version",
        package_manager_probe_command(storage, runtime, &tool, package_manager),
        &tool.version,
        "run `jolter repair` to replace the package manager installation",
    ));
    Ok(())
}

fn storage_write_check(storage: &Storage) -> Check {
    match tempfile::NamedTempFile::new_in(storage.root()) {
        Ok(_) => Check::pass("storage permissions", "storage is writable"),
        Err(error) => Check::fail(
            "storage permissions",
            format!("{} is not writable: {error}", storage.root().display()),
            "fix directory ownership/permissions or set JOLTER_HOME to a writable location",
        ),
    }
}

fn platform_check() -> Check {
    let os_supported = matches!(env::consts::OS, "windows" | "linux" | "macos");
    let arch_supported = matches!(env::consts::ARCH, "x86_64" | "aarch64");
    if !os_supported || !arch_supported {
        return Check::fail(
            "platform",
            format!(
                "{} {} is outside Jolter's supported platform matrix",
                env::consts::OS,
                env::consts::ARCH
            ),
            "use Windows, Linux, macOS, or WSL on x64 or ARM64",
        );
    }
    #[cfg(target_arch = "x86_64")]
    if !std::is_x86_feature_detected!("sse4.2") {
        return Check::warning(
            "platform",
            format!(
                "{} {} is supported, but this CPU cannot run Bun x64 builds",
                env::consts::OS,
                env::consts::ARCH
            ),
            "use Node.js or Deno, or run Bun on a CPU with SSE4.2",
        );
    }
    Check::pass(
        "platform",
        format!("{} {} is supported", env::consts::OS, env::consts::ARCH),
    )
}

fn runtime_manifest_check(runtime: &InstalledRuntime) -> Check {
    let path = runtime.path.join(".jolter-install.json");
    let manifest: RuntimeManifest = match read_manifest(&path) {
        Ok(manifest) => manifest,
        Err(ManifestRead::Missing) => {
            return Check::warning(
                "runtime manifest",
                format!("installation manifest is missing at {}", path.display()),
                "run `jolter repair` to recreate a verified installation",
            );
        }
        Err(ManifestRead::Invalid(error)) => {
            return Check::fail(
                "runtime manifest",
                format!(
                    "invalid installation manifest at {}: {error}",
                    path.display()
                ),
                "run `jolter repair` to replace the installation",
            );
        }
    };
    if manifest.runtime != runtime.kind.to_string()
        || manifest.version != runtime.version.to_string()
        || !valid_manifest_artifact(&manifest.artifact_url, &manifest.integrity)
    {
        return Check::fail(
            "runtime manifest",
            format!(
                "manifest identity or integrity metadata does not match {}@{}",
                runtime.kind, runtime.version
            ),
            "run `jolter repair` to replace the installation",
        );
    }
    Check::pass(
        "runtime manifest",
        format!("verified metadata is present at {}", path.display()),
    )
}

fn tool_manifest_check(tool: &InstalledTool) -> Check {
    let path = tool.path.join(".jolter-tool.json");
    let manifest: ToolManifest = match read_manifest(&path) {
        Ok(manifest) => manifest,
        Err(ManifestRead::Missing) => {
            return Check::warning(
                "package manager manifest",
                format!("installation manifest is missing at {}", path.display()),
                "run `jolter repair` to recreate a verified installation",
            );
        }
        Err(ManifestRead::Invalid(error)) => {
            return Check::fail(
                "package manager manifest",
                format!(
                    "invalid installation manifest at {}: {error}",
                    path.display()
                ),
                "run `jolter repair` to replace the installation",
            );
        }
    };
    if manifest.package_manager != tool.kind.to_string()
        || manifest.version != tool.version.to_string()
        || !valid_manifest_artifact(&manifest.artifact_url, &manifest.integrity)
    {
        return Check::fail(
            "package manager manifest",
            format!(
                "manifest identity or integrity metadata does not match {}@{}",
                tool.kind, tool.version
            ),
            "run `jolter repair` to replace the installation",
        );
    }
    Check::pass(
        "package manager manifest",
        format!("verified metadata is present at {}", path.display()),
    )
}

fn valid_manifest_artifact(url: &str, integrity: &str) -> bool {
    url.starts_with("https://")
        && (integrity.strip_prefix("sha256-").is_some_and(|value| {
            value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
        }) || integrity
            .strip_prefix("sha512-")
            .and_then(|value| BASE64.decode(value).ok())
            .is_some_and(|value| value.len() == 64))
}

#[cfg(unix)]
fn runtime_permission_check(runtime: &InstalledRuntime) -> Check {
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(runtime.executable()) {
        Ok(metadata) if metadata.permissions().mode() & 0o111 != 0 => Check::pass(
            "runtime permissions",
            "runtime executable permission is set",
        ),
        Ok(_) => Check::fail(
            "runtime permissions",
            format!("{} is not executable", runtime.executable().display()),
            "run `jolter repair` to restore executable permissions",
        ),
        Err(error) => Check::fail(
            "runtime permissions",
            format!(
                "could not inspect {}: {error}",
                runtime.executable().display()
            ),
            "run `jolter repair` to replace the installation",
        ),
    }
}

#[cfg(not(unix))]
fn runtime_permission_check(_runtime: &InstalledRuntime) -> Check {
    Check::pass(
        "runtime permissions",
        "runtime executable is present on Windows",
    )
}

fn package_manager_engine_check(tool: &InstalledTool, node_version: &Version) -> Check {
    let path = tool.path.join("package.json");
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Check::warning(
                "Node.js compatibility",
                format!("package metadata is missing at {}", path.display()),
                "run `jolter repair` to recreate the managed package manager",
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
                "run `jolter repair` to replace the package manager",
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
            "pin a compatible Node.js or package manager version, then run `jolter sync`",
        )
    }
}

fn runtime_probe_command(runtime: &InstalledRuntime) -> Command {
    let mut command = Command::new(runtime.executable());
    command.arg("--version");
    command
}

fn package_manager_probe_command(
    storage: &Storage,
    runtime: &InstalledRuntime,
    tool: &InstalledTool,
    requirement: &ResolvedPackageManager,
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

fn version_probe_check(
    name: &'static str,
    command: Command,
    expected: &Version,
    remediation: &'static str,
) -> Check {
    match run_probe(command) {
        Ok(output) if output.timed_out => Check::fail(
            name,
            format!("version probe exceeded {} seconds", PROBE_TIMEOUT.as_secs()),
            remediation,
        ),
        Ok(output) if !output.status.success() => Check::fail(
            name,
            format!(
                "version probe exited with {}: {}",
                output.status,
                output.combined_output()
            ),
            remediation,
        ),
        Ok(output) => match extract_version(&output.combined_output()) {
            Some(actual) if &actual == expected => {
                Check::pass(name, format!("reported version matches {expected}"))
            }
            Some(actual) => Check::fail(
                name,
                format!("reported version {actual} does not match installed version {expected}"),
                remediation,
            ),
            None => Check::fail(
                name,
                format!(
                    "could not parse a semantic version from `{}`",
                    output.combined_output()
                ),
                remediation,
            ),
        },
        Err(error) => Check::fail(name, format!("version probe failed: {error}"), remediation),
    }
}

struct ProbeOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

impl ProbeOutput {
    fn combined_output(&self) -> String {
        let stdout = self.stdout.trim();
        let stderr = self.stderr.trim();
        match (stdout.is_empty(), stderr.is_empty()) {
            (false, true) => stdout.to_owned(),
            (true, false) => stderr.to_owned(),
            (false, false) => format!("{stdout}; {stderr}"),
            (true, true) => "<no output>".to_owned(),
        }
    }
}

fn run_probe(mut command: Command) -> Result<ProbeOutput, std::io::Error> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .env("JOLTER_DOCTOR", "1");
    let mut child = command.spawn()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(25));
    };
    Ok(ProbeOutput {
        status,
        stdout: read_probe_output(&mut stdout)?,
        stderr: read_probe_output(&mut stderr)?,
        timed_out,
    })
}

fn read_probe_output(file: &mut fs::File) -> Result<String, std::io::Error> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(MAX_PROBE_OUTPUT).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn extract_version(output: &str) -> Option<Version> {
    output
        .split(|character: char| character.is_whitespace() || character == ',' || character == ';')
        .map(|token| {
            token.trim_matches(|character: char| {
                !(character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '-' | '+' | 'v' | 'V'))
            })
        })
        .find_map(|token| Version::parse(token.trim_start_matches(['v', 'V'])).ok())
}

fn shim_check(storage: &Storage) -> Check {
    let missing = SHIM_COMMANDS
        .iter()
        .filter(|command| !storage.shims_dir().join(shim_file_name(command)).is_file())
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Check::pass(
            "shims",
            format!(
                "all shims are installed in {}",
                storage.shims_dir().display()
            ),
        )
    } else {
        Check::warning(
            "shims",
            format!("missing shims: {}", missing.join(", ")),
            "run `jolter setup` to recreate all shims",
        )
    }
}

fn path_check(storage: &Storage) -> Check {
    if path_entries().any(|entry| same_path(&entry, &storage.shims_dir())) {
        Check::pass(
            "PATH",
            format!("{} is on PATH", storage.shims_dir().display()),
        )
    } else {
        Check::warning(
            "PATH",
            format!("{} is not on PATH", storage.shims_dir().display()),
            "run `jolter setup` and apply the printed command for your shell",
        )
    }
}

fn path_conflict_check(storage: &Storage) -> Check {
    let entries = path_entries().collect::<Vec<_>>();
    let Some(shim_index) = entries
        .iter()
        .position(|entry| same_path(entry, &storage.shims_dir()))
    else {
        return Check::warning(
            "PATH precedence",
            "Jolter shims are not on PATH, so executable precedence cannot be validated",
            "run `jolter setup` and place its shims directory near the start of PATH",
        );
    };
    let mut conflicts = Vec::new();
    for directory in &entries[..shim_index] {
        for command in SHIM_COMMANDS.into_iter().chain(["corepack"]) {
            if command_candidates(directory, command)
                .iter()
                .any(|candidate| candidate.is_file())
            {
                conflicts.push(format!("{command} ({})", directory.display()));
            }
        }
    }
    conflicts.sort();
    conflicts.dedup();
    if conflicts.is_empty() {
        Check::pass(
            "PATH precedence",
            "no conflicting JavaScript toolchain executables precede Jolter shims",
        )
    } else {
        Check::warning(
            "PATH precedence",
            format!("executables shadow Jolter shims: {}", conflicts.join(", ")),
            "move the Jolter shims directory earlier on PATH; remove stale nvm, fnm, Volta, or Corepack entries when appropriate",
        )
    }
}

fn command_candidates(directory: &Path, command: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let extensions = env::var_os("PATHEXT").map_or_else(
            || {
                vec![
                    ".com".to_owned(),
                    ".exe".to_owned(),
                    ".bat".to_owned(),
                    ".cmd".to_owned(),
                ]
            },
            |value| {
                value
                    .to_string_lossy()
                    .split(';')
                    .filter(|extension| !extension.is_empty())
                    .map(str::to_ascii_lowercase)
                    .collect::<Vec<_>>()
            },
        );
        extensions
            .into_iter()
            .map(|extension| directory.join(format!("{command}{extension}")))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![directory.join(command)]
    }
}

fn cache_check(storage: &Storage) -> Result<Check, DoctorError> {
    let mut invalid = Vec::new();
    inspect_cache_directory(
        &storage.cache_dir().join("metadata"),
        |name| {
            name.strip_suffix(".txt")
                .is_some_and(|stem| valid_hex(stem, 64))
        },
        &mut invalid,
    )?;
    inspect_cache_directory(
        &storage.cache_dir().join("downloads"),
        |name| {
            name.strip_suffix(".zip")
                .or_else(|| name.strip_suffix(".tar.gz"))
                .is_some_and(|stem| valid_hex(stem, 64))
        },
        &mut invalid,
    )?;
    if !invalid.is_empty() {
        return Ok(Check::warning(
            "cache",
            format!("found {} unrecognized cache entry(s)", invalid.len()),
            "run `jolter cache clean` to remove cached downloads and metadata",
        ));
    }
    let stats = storage.cache_stats()?;
    if offline_mode() && stats.files == 0 {
        return Ok(Check::warning(
            "cache",
            "offline mode is enabled but the cache is empty",
            "disable JOLTER_OFFLINE for the first sync or prewarm the cache online",
        ));
    }
    Ok(Check::pass(
        "cache",
        format!("cache contains {} file(s)", stats.files),
    ))
}

fn inspect_cache_directory(
    directory: &Path,
    valid_name: impl Fn(&str) -> bool,
    invalid: &mut Vec<PathBuf>,
) -> Result<(), DoctorError> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|source| DoctorError::ReadDirectory {
        path: directory.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| DoctorError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !entry
            .file_type()
            .map_err(|source| DoctorError::ReadDirectory {
                path: entry.path(),
                source,
            })?
            .is_file()
            || !valid_name(&name)
        {
            invalid.push(entry.path());
        }
    }
    Ok(())
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn network_environment_check() -> Check {
    let proxies = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .into_iter()
        .filter_map(|name| env::var_os(name).map(|value| (name, value)))
        .collect::<Vec<_>>();
    let invalid_proxy = proxies.iter().find(|(_, value)| {
        let value = value.to_string_lossy().to_ascii_lowercase();
        !(value.starts_with("http://") || value.starts_with("https://"))
    });
    if let Some((name, value)) = invalid_proxy {
        return Check::warning(
            "network environment",
            format!(
                "{name} has an unsupported value `{}`",
                value.to_string_lossy()
            ),
            "use an http:// or https:// proxy URL, or remove the proxy variable",
        );
    }
    for name in ["SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS", "REQUESTS_CA_BUNDLE"] {
        if let Some(value) = env::var_os(name) {
            let path = PathBuf::from(value);
            if !path.is_file() {
                return Check::warning(
                    "network environment",
                    format!(
                        "{name} points to missing certificate file {}",
                        path.display()
                    ),
                    "correct or remove the certificate environment variable",
                );
            }
        }
    }
    if proxies.is_empty() {
        Check::pass(
            "network environment",
            "no explicit proxy configuration was detected",
        )
    } else {
        Check::pass(
            "network environment",
            "proxy environment uses supported URL schemes",
        )
    }
}

fn path_entries() -> impl Iterator<Item = PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|value| env::split_paths(&value).collect::<Vec<_>>())
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

#[cfg(windows)]
fn shim_file_name(command: &str) -> String {
    format!("{command}.exe")
}

#[cfg(not(windows))]
fn shim_file_name(command: &str) -> String {
    command.to_owned()
}

fn offline_mode() -> bool {
    env::var_os("JOLTER_OFFLINE").is_some_and(|value| {
        matches!(
            value.to_string_lossy().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeManifest {
    runtime: String,
    version: String,
    artifact_url: String,
    integrity: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolManifest {
    package_manager: String,
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

enum ManifestRead {
    Missing,
    Invalid(String),
}

fn read_manifest<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ManifestRead> {
    let contents = fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ManifestRead::Missing
        } else {
            ManifestRead::Invalid(error.to_string())
        }
    })?;
    serde_json::from_str(&contents).map_err(|error| ManifestRead::Invalid(error.to_string()))
}

#[derive(Debug, Error)]
pub enum DoctorError {
    #[error("failed to read diagnostics directory {path}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Storage(#[from] jolter_storage::StorageError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolter_runtime::PackageManagerKind;

    #[cfg(not(windows))]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn reports_a_matching_managed_package_manager_as_healthy() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        fs::write(
            project.path().join("jolter.json"),
            r#"{"runtime":{"node":"24"},"packageManager":{"pnpm":"10"}}"#,
        )
        .unwrap();
        let node = storage.runtime_executable(RuntimeKind::Node, &Version::new(24, 1, 0));
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(node, b"node").unwrap();
        let pnpm = storage
            .tool_entrypoint(PackageManagerKind::Pnpm, &Version::new(10, 2, 0), "pnpm")
            .unwrap();
        fs::create_dir_all(pnpm.parent().unwrap()).unwrap();
        fs::write(pnpm, b"pnpm").unwrap();

        let report = examine(project.path(), &storage).unwrap();
        let check = report
            .checks
            .iter()
            .find(|check| check.name == "package manager")
            .unwrap();

        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn reports_invalid_configuration_as_a_check() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        fs::write(project.path().join("jolter.json"), "{not-json").unwrap();

        let report = examine(project.path(), &storage).unwrap();

        assert!(report.checks.iter().any(|check| {
            check.name == "configuration"
                && check.status == CheckStatus::Fail
                && check.remediation.is_some()
        }));
    }

    #[test]
    fn parses_common_runtime_version_output() {
        assert_eq!(extract_version("v24.2.1\n"), Some(Version::new(24, 2, 1)));
        assert_eq!(
            extract_version("deno 2.4.0 (stable, release)"),
            Some(Version::new(2, 4, 0))
        );
    }

    #[test]
    fn bounded_probe_reports_a_version() {
        #[cfg(windows)]
        {
            let mut command = Command::new(env::var_os("COMSPEC").unwrap());
            command.args(["/d", "/c", "echo", "3.2.1"]);
            let output = run_probe(command).unwrap();
            assert_eq!(
                extract_version(&output.combined_output()),
                Some(Version::new(3, 2, 1))
            );
        }
        #[cfg(not(windows))]
        {
            let directory = tempfile::tempdir().unwrap();
            let script = directory.path().join("probe");
            fs::write(&script, "#!/bin/sh\nprintf '3.2.1\\n'").unwrap();
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
            let output = run_probe(Command::new(&script)).unwrap();
            assert_eq!(
                extract_version(&output.combined_output()),
                Some(Version::new(3, 2, 1))
            );
        }
    }

    #[test]
    fn validates_runtime_and_tool_manifests() {
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        let runtime = InstalledRuntime {
            kind: RuntimeKind::Node,
            version: Version::new(24, 1, 0),
            path: storage.runtime_version_dir(RuntimeKind::Node, &Version::new(24, 1, 0)),
        };
        fs::create_dir_all(&runtime.path).unwrap();
        fs::write(
            runtime.path.join(".jolter-install.json"),
            r#"{
                "runtime":"node",
                "version":"24.1.0",
                "artifactUrl":"https://nodejs.org/node.zip",
                "integrity":"sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }"#,
        )
        .unwrap();
        assert_eq!(runtime_manifest_check(&runtime).status, CheckStatus::Pass);

        let tool = InstalledTool {
            kind: PackageManagerKind::Pnpm,
            version: Version::new(10, 2, 0),
            path: storage.tool_version_dir(PackageManagerKind::Pnpm, &Version::new(10, 2, 0)),
        };
        fs::create_dir_all(&tool.path).unwrap();
        let integrity = format!("sha512-{}", BASE64.encode([0_u8; 64]));
        fs::write(
            tool.path.join(".jolter-tool.json"),
            format!(
                r#"{{
                "packageManager":"pnpm",
                "version":"10.2.0",
                "artifactUrl":"https://registry.npmjs.org/pnpm.tgz",
                "integrity":"{integrity}"
            }}"#
            ),
        )
        .unwrap();
        assert_eq!(tool_manifest_check(&tool).status, CheckStatus::Pass);

        fs::write(
            tool.path.join(".jolter-tool.json"),
            r#"{"packageManager":"yarn","version":"10.2.0","artifactUrl":"http://example.test/tool","integrity":"bad"}"#,
        )
        .unwrap();
        assert_eq!(tool_manifest_check(&tool).status, CheckStatus::Fail);
    }

    #[test]
    fn diagnoses_package_manager_engine_metadata() {
        let home = tempfile::tempdir().unwrap();
        let tool = InstalledTool {
            kind: PackageManagerKind::Pnpm,
            version: Version::new(10, 2, 0),
            path: home.path().join("pnpm"),
        };
        fs::create_dir_all(&tool.path).unwrap();

        fs::write(
            tool.path.join("package.json"),
            r#"{"engines":{"node":"^20.0.0 || >=22"}}"#,
        )
        .unwrap();
        assert_eq!(
            package_manager_engine_check(&tool, &Version::new(22, 1, 0)).status,
            CheckStatus::Pass
        );
        assert_eq!(
            package_manager_engine_check(&tool, &Version::new(21, 0, 0)).status,
            CheckStatus::Fail
        );

        fs::write(
            tool.path.join("package.json"),
            r#"{"engines":{"node":"definitely not semver"}}"#,
        )
        .unwrap();
        assert_eq!(
            package_manager_engine_check(&tool, &Version::new(22, 1, 0)).status,
            CheckStatus::Fail
        );

        fs::write(tool.path.join("package.json"), "{}").unwrap();
        assert_eq!(
            package_manager_engine_check(&tool, &Version::new(22, 1, 0)).status,
            CheckStatus::Pass
        );
    }

    #[test]
    fn reports_invalid_cache_entries() {
        let home = tempfile::tempdir().unwrap();
        let storage = Storage::new(home.path());
        storage.ensure_layout().unwrap();
        let invalid = storage
            .cache_dir()
            .join("downloads")
            .join("not-a-cache-key");
        fs::create_dir_all(invalid.parent().unwrap()).unwrap();
        fs::write(invalid, b"bad").unwrap();

        let check = cache_check(&storage).unwrap();

        assert_eq!(check.status, CheckStatus::Warning);
        assert!(check.remediation.unwrap().contains("cache clean"));
    }

    #[test]
    fn version_probe_detects_mismatches_and_nonzero_exits() {
        #[cfg(windows)]
        let version_command = {
            let mut command = Command::new(env::var_os("COMSPEC").unwrap());
            command.args(["/d", "/c", "echo", "3.2.1"]);
            command
        };
        #[cfg(not(windows))]
        let version_command = {
            let mut command = Command::new("sh");
            command.args(["-c", "printf '3.2.1\\n'"]);
            command
        };
        assert_eq!(
            version_probe_check("version", version_command, &Version::new(3, 2, 0), "repair")
                .status,
            CheckStatus::Fail
        );

        #[cfg(windows)]
        let failing_command = {
            let mut command = Command::new(env::var_os("COMSPEC").unwrap());
            command.args(["/d", "/c", "exit", "7"]);
            command
        };
        #[cfg(not(windows))]
        let failing_command = {
            let mut command = Command::new("sh");
            command.args(["-c", "exit 7"]);
            command
        };
        assert_eq!(
            version_probe_check("version", failing_command, &Version::new(3, 2, 1), "repair")
                .status,
            CheckStatus::Fail
        );
    }
}
