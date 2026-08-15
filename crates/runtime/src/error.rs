use crate::hash::ToolHashAlgorithm;
use thiserror::Error;

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
