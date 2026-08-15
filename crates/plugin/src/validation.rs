use std::{fs, io::Read, path::Path};

use semver::Version;
use sha2::{Digest, Sha256};

use crate::{PLUGIN_RELEASE_SCHEMA_URL, error::PluginError, manifest::PluginReleaseManifest};

pub(crate) fn validate_release_manifest(
    manifest: &PluginReleaseManifest,
    canonical: &str,
    version: &Version,
) -> Result<(), PluginError> {
    if manifest.schema_version != 1 {
        return Err(PluginError::UnsupportedSchema(manifest.schema_version));
    }
    if let Some(schema_url) = &manifest.schema_url
        && schema_url != PLUGIN_RELEASE_SCHEMA_URL
    {
        return Err(PluginError::SchemaUrlMismatch {
            found: schema_url.clone(),
            expected: PLUGIN_RELEASE_SCHEMA_URL.to_owned(),
        });
    }
    if manifest.name.to_ascii_lowercase() != canonical {
        return Err(PluginError::ManifestIdentity {
            expected: canonical.to_owned(),
            actual: manifest.name.clone(),
        });
    }
    if manifest.version != version.to_string() {
        return Err(PluginError::ManifestVersion {
            expected: version.to_string(),
            actual: manifest.version.clone(),
        });
    }
    if manifest.entrypoint.kind != "wasm"
        || manifest.entrypoint.path != manifest.artifacts.wasm.file
    {
        return Err(PluginError::InvalidEntrypoint);
    }
    if manifest.artifacts.wasm.sha256.len() != 64
        || !manifest
            .artifacts
            .wasm
            .sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(PluginError::InvalidSha256(
            manifest.artifacts.wasm.sha256.clone(),
        ));
    }
    if manifest
        .permissions
        .commands
        .as_ref()
        .is_some_and(|commands| commands.execute)
    {
        return Err(PluginError::CommandExecutionUnsupported);
    }
    Ok(())
}

pub(crate) fn validate_plugin_name(value: &str) -> Result<(), PluginError> {
    let valid_scoped = value.starts_with('@')
        && value.split('/').count() == 2
        && value
            .trim_start_matches('@')
            .split('/')
            .all(valid_identifier_component);
    let valid_alias = !value.starts_with('@') && valid_identifier_component(value);
    if valid_scoped || valid_alias {
        Ok(())
    } else {
        Err(PluginError::InvalidName(value.to_owned()))
    }
}

pub(crate) fn valid_identifier_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        && value
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}

pub(crate) fn validate_selector(value: &str) -> Result<(), PluginError> {
    if value.eq_ignore_ascii_case("latest") || selector_components_valid(value) {
        Ok(())
    } else {
        Err(PluginError::InvalidSelector(value.to_owned()))
    }
}

pub(crate) fn selector_components_valid(value: &str) -> bool {
    let components = value.split('.').collect::<Vec<_>>();
    !components.is_empty()
        && components.len() <= 3
        && components.iter().all(|component| {
            component.eq_ignore_ascii_case("x")
                || *component == "*"
                || (!component.is_empty()
                    && component
                        .chars()
                        .all(|character| character.is_ascii_digit()))
        })
}

pub(crate) fn selector_matches(selector: &str, version: &Version) -> bool {
    if selector.eq_ignore_ascii_case("latest")
        || selector == "*"
        || selector.eq_ignore_ascii_case("x")
    {
        return true;
    }
    let parts = selector
        .split('.')
        .filter(|part| !part.eq_ignore_ascii_case("x") && *part != "*")
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>();
    match parts.as_deref() {
        Ok([major]) => version.major == *major,
        Ok([major, minor]) => version.major == *major && version.minor == *minor,
        Ok([major, minor, patch]) => {
            version.major == *major && version.minor == *minor && version.patch == *patch
        }
        _ => false,
    }
}

pub(crate) fn absolute_url(registry_url: &str, value: &str) -> Result<String, PluginError> {
    if value.starts_with("https://") {
        Ok(value.to_owned())
    } else if value.starts_with('/') {
        Ok(format!("{}{}", registry_url.trim_end_matches('/'), value))
    } else {
        Err(PluginError::InvalidUrl(value.to_owned()))
    }
}

pub(crate) fn encode_path(value: &str) -> String {
    value.replace('@', "%40").replace('/', "%2F")
}

pub(crate) fn ensure_https(url: &str) -> Result<(), PluginError> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err(PluginError::InsecureUrl(url.to_owned()))
    }
}

pub(crate) fn verify_sha256(path: &Path, expected: &str) -> Result<(), PluginError> {
    let mut file = fs::File::open(path).map_err(PluginError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(PluginError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual == expected.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(PluginError::ChecksumMismatch {
            expected: expected.to_owned(),
            actual,
        })
    }
}
