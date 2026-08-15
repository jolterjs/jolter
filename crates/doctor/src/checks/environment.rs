use std::{
    env, fs,
    path::{Path, PathBuf},
};

use jolter_shim::SHIM_COMMANDS;
use jolter_storage::Storage;

use crate::{error::DoctorError, types::Check};

#[must_use]
pub fn storage_write_check(storage: &Storage) -> Check {
    match tempfile::NamedTempFile::new_in(storage.root()) {
        Ok(_) => Check::pass("storage permissions", "storage is writable"),
        Err(error) => Check::fail(
            "storage permissions",
            format!("{} is not writable: {error}", storage.root().display()),
            "fix directory ownership/permissions or set JOLTER_HOME to a writable location",
        ),
    }
}

#[must_use]
pub fn platform_check() -> Check {
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

#[must_use]
pub fn shim_check(storage: &Storage) -> Check {
    let desired = match jolter_shim::desired_shim_commands(storage) {
        Ok(commands) => commands,
        Err(error) => {
            return Check::fail(
                "shims",
                format!("failed to evaluate expected shims: {error}"),
                "run `jolter setup` or reinstall the affected runtimes/tools",
            );
        }
    };
    let missing = desired
        .iter()
        .filter(|command| !storage.shims_dir().join(shim_file_name(command)).is_file())
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        if desired.is_empty() {
            Check::pass(
                "shims",
                "no shims are required yet (install a runtime or tool to create shims)",
            )
        } else {
            Check::pass(
                "shims",
                format!(
                    "all required shims are installed in {}",
                    storage.shims_dir().display()
                ),
            )
        }
    } else {
        Check::warning(
            "shims",
            format!("missing shims: {}", missing.join(", ")),
            "run `jolter setup` or install the required toolchain to recreate shims",
        )
    }
}

#[must_use]
pub fn path_check(storage: &Storage) -> Check {
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

#[must_use]
pub fn path_conflict_check(storage: &Storage) -> Check {
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

pub fn cache_check(storage: &Storage) -> Result<Check, DoctorError> {
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

#[must_use]
pub fn network_environment_check() -> Check {
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
