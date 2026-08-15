use std::{fmt, str::FromStr};

use semver::Version;

use crate::{
    error::{RuntimeRequestError, ToolRequestError},
    hash::ToolHash,
    kinds::{RuntimeKind, ToolKind},
    validation::{parse_tool_selector, selector_matches_version, validate_selector},
};

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
