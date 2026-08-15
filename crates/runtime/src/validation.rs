use semver::Version;

use crate::{
    error::{RuntimeRequestError, ToolRequestError},
    hash::{ToolHash, ToolHashAlgorithm},
    kinds::RuntimeKind,
};

pub(crate) fn validate_selector(
    kind: RuntimeKind,
    selector: &str,
) -> Result<(), RuntimeRequestError> {
    if selector.eq_ignore_ascii_case("latest") {
        return Ok(());
    }
    if selector.eq_ignore_ascii_case("lts") {
        return if kind == RuntimeKind::Node {
            Ok(())
        } else {
            Err(RuntimeRequestError::LtsUnsupported)
        };
    }

    validate_numeric_selector(selector)
        .map_err(|()| RuntimeRequestError::InvalidSelector(selector.to_owned()))
}

pub(crate) fn validate_numeric_selector(selector: &str) -> Result<(), ()> {
    if selector.eq_ignore_ascii_case("latest") {
        return Ok(());
    }

    let components: Vec<_> = selector.split('.').collect();
    if components.is_empty() || components.len() > 3 {
        return Err(());
    }
    let mut wildcard_seen = false;
    for component in components {
        if component.eq_ignore_ascii_case("x") || component == "*" {
            wildcard_seen = true;
        } else if wildcard_seen
            || component.is_empty()
            || !component
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return Err(());
        }
    }
    Ok(())
}

pub(crate) fn parse_tool_selector(
    value: &str,
) -> Result<(&str, Option<ToolHash>), ToolRequestError> {
    let mut parts = value.split('+');
    let selector = parts.next().ok_or(ToolRequestError::MissingSelector)?;
    let hash = parts.next();
    if parts.next().is_some() {
        return Err(ToolRequestError::InvalidHashFormat);
    }
    validate_numeric_selector(selector)
        .map_err(|()| ToolRequestError::InvalidSelector(selector.to_owned()))?;
    let Some(hash) = hash else {
        return Ok((selector, None));
    };
    let exact = Version::parse(selector)
        .is_ok_and(|version| version.pre.is_empty() && version.build.is_empty());
    if !exact {
        return Err(ToolRequestError::HashRequiresExactVersion);
    }
    let (algorithm, value) = hash
        .split_once('.')
        .ok_or(ToolRequestError::InvalidHashFormat)?;
    let algorithm: ToolHashAlgorithm = algorithm.parse()?;
    if value.len() != algorithm.hex_length()
        || !value.chars().all(|character| character.is_ascii_hexdigit())
    {
        return Err(ToolRequestError::InvalidHash {
            algorithm,
            value: value.to_owned(),
        });
    }
    Ok((
        selector,
        Some(ToolHash {
            algorithm,
            value: value.to_ascii_lowercase(),
        }),
    ))
}

pub(crate) fn selector_matches_version(selector: &str, version: &Version) -> bool {
    if selector.eq_ignore_ascii_case("latest") {
        return true;
    }
    if selector == "*" || selector.eq_ignore_ascii_case("x") {
        return true;
    }

    let components: Vec<_> = selector
        .trim_end_matches(".x")
        .split('.')
        .filter(|component| {
            !component.eq_ignore_ascii_case("x") && !component.eq_ignore_ascii_case("*")
        })
        .collect();
    let Ok(numbers) = components
        .iter()
        .map(|component| component.parse::<u64>())
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };

    match numbers.as_slice() {
        [major] => version.major == *major,
        [major, minor] => version.major == *major && version.minor == *minor,
        [major, minor, patch] => {
            version.major == *major && version.minor == *minor && version.patch == *patch
        }
        _ => false,
    }
}
