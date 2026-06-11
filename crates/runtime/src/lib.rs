use std::{fmt, str::FromStr};

use semver::Version;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeKind {
    Node,
    Bun,
    Deno,
}

impl RuntimeKind {
    pub const ALL: [Self; 3] = [Self::Node, Self::Bun, Self::Deno];

    #[must_use]
    pub const fn executable_name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Bun => "bun",
            Self::Deno => "deno",
        }
    }
}

impl fmt::Display for RuntimeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Node => "node",
            Self::Bun => "bun",
            Self::Deno => "deno",
        })
    }
}

impl FromStr for RuntimeKind {
    type Err = RuntimeRequestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "node" | "nodejs" => Ok(Self::Node),
            "bun" => Ok(Self::Bun),
            "deno" => Ok(Self::Deno),
            _ => Err(RuntimeRequestError::UnsupportedRuntime(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRequest {
    pub kind: RuntimeKind,
    pub selector: String,
}

impl RuntimeRequest {
    pub fn new(
        kind: RuntimeKind,
        selector: impl Into<String>,
    ) -> Result<Self, RuntimeRequestError> {
        let selector = selector.into();
        let selector = selector.trim().trim_start_matches('v');
        if selector.is_empty() {
            return Err(RuntimeRequestError::MissingSelector);
        }
        if selector.contains(char::is_whitespace) || selector.contains('@') {
            return Err(RuntimeRequestError::InvalidSelector(selector.to_owned()));
        }
        validate_selector(kind, selector)?;

        Ok(Self {
            kind,
            selector: selector.to_owned(),
        })
    }

    #[must_use]
    pub fn matches_version(&self, version: &Version) -> bool {
        let selector = self.selector.as_str();
        if selector.eq_ignore_ascii_case("latest") || selector.eq_ignore_ascii_case("lts") {
            return true;
        }

        let components: Vec<_> = selector
            .trim_end_matches(".x")
            .split('.')
            .filter(|component| !component.eq_ignore_ascii_case("x"))
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

    #[must_use]
    pub fn matches_release(&self, version: &Version, is_lts: bool) -> bool {
        if self.selector.eq_ignore_ascii_case("latest") {
            return true;
        }
        if self.selector.eq_ignore_ascii_case("lts") {
            return is_lts;
        }
        self.matches_version(version)
    }

    #[must_use]
    pub fn requires_release_metadata(&self) -> bool {
        self.selector.eq_ignore_ascii_case("latest") || self.selector.eq_ignore_ascii_case("lts")
    }
}

impl fmt::Display for RuntimeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.kind, self.selector)
    }
}

impl FromStr for RuntimeRequest {
    type Err = RuntimeRequestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, selector) = value
            .split_once('@')
            .ok_or(RuntimeRequestError::MissingSeparator)?;
        Self::new(kind.parse()?, selector)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuntimeRequestError {
    #[error("runtime request must use the form <runtime>@<version>, for example node@24")]
    MissingSeparator,
    #[error("runtime selector cannot be empty")]
    MissingSelector,
    #[error("unsupported runtime `{0}`; expected node, bun, or deno")]
    UnsupportedRuntime(String),
    #[error("invalid runtime selector `{0}`")]
    InvalidSelector(String),
    #[error("the `lts` selector is supported only for Node.js")]
    LtsUnsupported,
}

fn validate_selector(kind: RuntimeKind, selector: &str) -> Result<(), RuntimeRequestError> {
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

    let components: Vec<_> = selector.split('.').collect();
    if components.is_empty() || components.len() > 3 {
        return Err(RuntimeRequestError::InvalidSelector(selector.to_owned()));
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
            return Err(RuntimeRequestError::InvalidSelector(selector.to_owned()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_request() {
        let request: RuntimeRequest = "node@24".parse().unwrap();
        assert_eq!(request.kind, RuntimeKind::Node);
        assert_eq!(request.selector, "24");
    }

    #[test]
    fn rejects_unknown_runtime() {
        let error = "ruby@3".parse::<RuntimeRequest>().unwrap_err();
        assert!(matches!(
            error,
            RuntimeRequestError::UnsupportedRuntime(runtime) if runtime == "ruby"
        ));
    }

    #[test]
    fn matches_version_prefixes() {
        let version = Version::parse("24.2.1").unwrap();
        assert!(
            "node@24"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
        assert!(
            "node@24.x"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
        assert!(
            "node@24.2"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
        assert!(
            !"node@22"
                .parse::<RuntimeRequest>()
                .unwrap()
                .matches_version(&version)
        );
    }

    #[test]
    fn rejects_invalid_or_unsupported_selectors() {
        assert!("node@24.x.1".parse::<RuntimeRequest>().is_err());
        assert!("node@24-beta".parse::<RuntimeRequest>().is_err());
        assert!(matches!(
            "bun@lts".parse::<RuntimeRequest>(),
            Err(RuntimeRequestError::LtsUnsupported)
        ));
    }
}
