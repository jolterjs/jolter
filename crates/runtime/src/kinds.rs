use std::{fmt, str::FromStr};

use crate::error::{RuntimeRequestError, ToolRequestError};

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
