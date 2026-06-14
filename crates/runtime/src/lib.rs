use std::{fmt, str::FromStr};

use semver::Version;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeKind {
    Node,
    Bun,
    Deno,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolKind {
    Npm,
    Pnpm,
    Yarn,
}

impl ToolKind {
    pub const ALL: [Self; 3] = [Self::Npm, Self::Pnpm, Self::Yarn];

    #[must_use]
    pub const fn registry_package(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "@yarnpkg/cli-dist",
        }
    }

    #[must_use]
    pub fn entrypoint(self, command: &str) -> Option<&'static str> {
        match (self, command) {
            (Self::Npm, "npm") => Some("bin/npm-cli.js"),
            (Self::Npm, "npx") => Some("bin/npx-cli.js"),
            (Self::Pnpm, "pnpm") => Some("bin/pnpm.cjs"),
            (Self::Yarn, "yarn") => Some("bin/yarn.js"),
            _ => None,
        }
    }
}

impl fmt::Display for ToolKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
        })
    }
}

impl FromStr for ToolKind {
    type Err = ToolRequestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "npm" => Ok(Self::Npm),
            "pnpm" => Ok(Self::Pnpm),
            "yarn" | "yarnpkg" => Ok(Self::Yarn),
            _ => Err(ToolRequestError::UnsupportedTool(value.to_owned())),
        }
    }
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
        selector_matches_version(selector, version)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRequest {
    pub kind: ToolKind,
    pub selector: String,
    pub hash: Option<ToolHash>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolHashAlgorithm {
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

impl ToolHashAlgorithm {
    #[must_use]
    pub const fn hex_length(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha224 => 56,
            Self::Sha256 => 64,
            Self::Sha384 => 96,
            Self::Sha512 => 128,
        }
    }
}

impl fmt::Display for ToolHashAlgorithm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Sha1 => "sha1",
            Self::Sha224 => "sha224",
            Self::Sha256 => "sha256",
            Self::Sha384 => "sha384",
            Self::Sha512 => "sha512",
        })
    }
}

impl FromStr for ToolHashAlgorithm {
    type Err = ToolRequestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "sha1" => Ok(Self::Sha1),
            "sha224" => Ok(Self::Sha224),
            "sha256" => Ok(Self::Sha256),
            "sha384" => Ok(Self::Sha384),
            "sha512" => Ok(Self::Sha512),
            _ => Err(ToolRequestError::UnsupportedHashAlgorithm(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHash {
    pub algorithm: ToolHashAlgorithm,
    pub value: String,
}

impl fmt::Display for ToolHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.algorithm, self.value)
    }
}

impl ToolRequest {
    pub fn new(kind: ToolKind, selector: impl Into<String>) -> Result<Self, ToolRequestError> {
        let selector = selector.into();
        let selector = selector.trim().trim_start_matches('v');
        if selector.is_empty() {
            return Err(ToolRequestError::MissingSelector);
        }
        if selector.contains(char::is_whitespace) || selector.contains('@') {
            return Err(ToolRequestError::InvalidSelector(selector.to_owned()));
        }
        let (selector, hash) = parse_tool_selector(selector)?;

        Ok(Self {
            kind,
            selector: selector.to_owned(),
            hash,
        })
    }

    #[must_use]
    pub fn matches_version(&self, version: &Version) -> bool {
        selector_matches_version(&self.selector, version)
    }
}

impl fmt::Display for ToolRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.kind, self.selector)?;
        if let Some(hash) = &self.hash {
            write!(formatter, "+{hash}")?;
        }
        Ok(())
    }
}

impl FromStr for ToolRequest {
    type Err = ToolRequestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, selector) = value
            .rsplit_once('@')
            .ok_or(ToolRequestError::MissingSeparator)?;
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToolRequestError {
    #[error("tool request must use the form <tool>@<version>, for example pnpm@10")]
    MissingSeparator,
    #[error("tool selector cannot be empty")]
    MissingSelector,
    #[error("unsupported tool `{0}`; expected npm, pnpm, or yarn")]
    UnsupportedTool(String),
    #[error("invalid tool selector `{0}`")]
    InvalidSelector(String),
    #[error("tool hashes require an exact semantic version")]
    HashRequiresExactVersion,
    #[error("tool hash must use <algorithm>.<hex>")]
    InvalidHashFormat,
    #[error("unsupported tool hash algorithm `{0}`")]
    UnsupportedHashAlgorithm(String),
    #[error("invalid {algorithm} tool hash `{value}`")]
    InvalidHash {
        algorithm: ToolHashAlgorithm,
        value: String,
    },
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

    validate_numeric_selector(selector)
        .map_err(|()| RuntimeRequestError::InvalidSelector(selector.to_owned()))
}

fn validate_numeric_selector(selector: &str) -> Result<(), ()> {
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

fn parse_tool_selector(value: &str) -> Result<(&str, Option<ToolHash>), ToolRequestError> {
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

fn selector_matches_version(selector: &str, version: &Version) -> bool {
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

    #[test]
    fn parses_and_matches_tool_requests() {
        let request: ToolRequest = "pnpm@10.x".parse().unwrap();
        assert_eq!(request.kind, ToolKind::Pnpm);
        assert_eq!(request.hash, None);
        assert!(request.matches_version(&Version::new(10, 34, 3)));
        assert!(!request.matches_version(&Version::new(11, 0, 0)));
        assert!("@yarnpkg/cli-dist@4".parse::<ToolRequest>().is_err());
    }

    #[test]
    fn parses_corepack_tool_hashes() {
        let value = format!("pnpm@10.2.0+sha224.{}", "A".repeat(56));
        let request: ToolRequest = value.parse().unwrap();

        assert_eq!(request.selector, "10.2.0");
        assert_eq!(
            request.hash.as_ref().unwrap().algorithm,
            ToolHashAlgorithm::Sha224
        );
        assert_eq!(request.hash.as_ref().unwrap().value, "a".repeat(56));
        assert_eq!(
            request.to_string(),
            format!("pnpm@10.2.0+sha224.{}", "a".repeat(56))
        );
    }

    #[test]
    fn rejects_invalid_corepack_hashes() {
        assert!(matches!(
            format!("pnpm@10+sha224.{}", "a".repeat(56)).parse::<ToolRequest>(),
            Err(ToolRequestError::HashRequiresExactVersion)
        ));
        assert!(matches!(
            "pnpm@10.2.0+md5.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse::<ToolRequest>(),
            Err(ToolRequestError::UnsupportedHashAlgorithm(_))
        ));
        assert!(matches!(
            "pnpm@10.2.0+sha224.deadbeef".parse::<ToolRequest>(),
            Err(ToolRequestError::InvalidHash { .. })
        ));
    }
}
