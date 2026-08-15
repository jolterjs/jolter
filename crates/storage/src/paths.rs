use std::{
    env,
    path::{Path, PathBuf},
};

use jolter_runtime::RuntimeKind;
use semver::Version;

#[must_use]
pub fn runtime_executable_in(root: &Path, kind: RuntimeKind) -> PathBuf {
    match kind {
        RuntimeKind::Node => {
            #[cfg(windows)]
            {
                root.join("node.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("bin").join("node")
            }
        }
        RuntimeKind::Bun => {
            #[cfg(windows)]
            {
                root.join("bun.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("bun")
            }
        }
        RuntimeKind::Deno => {
            #[cfg(windows)]
            {
                root.join("deno.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("deno")
            }
        }
    }
}

pub(crate) fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}

#[must_use]
pub fn plugin_path_parts(canonical_name: &str) -> (String, String) {
    let normalized = canonical_name
        .trim()
        .trim_start_matches('@')
        .to_ascii_lowercase();
    let (scope, name) = normalized
        .split_once('/')
        .unwrap_or(("unknown", normalized.as_str()));
    (scope.to_owned(), name.to_owned())
}

pub fn selector_matches_version(selector: &str, version: &Version) -> bool {
    if selector.eq_ignore_ascii_case("latest")
        || selector == "*"
        || selector.eq_ignore_ascii_case("x")
    {
        return true;
    }
    let Ok(parts) = selector
        .split('.')
        .filter(|part| !part.eq_ignore_ascii_case("x") && *part != "*")
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    match parts.as_slice() {
        [major] => version.major == *major,
        [major, minor] => version.major == *major && version.minor == *minor,
        [major, minor, patch] => {
            version.major == *major && version.minor == *minor && version.patch == *patch
        }
        _ => false,
    }
}
