use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bun: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deno: Option<String>,
}

impl RuntimeConfig {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.node.is_none() && self.bun.is_none() && self.deno.is_none()
    }

    #[must_use]
    pub fn configured_count(&self) -> usize {
        self.entries().count()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            ("node", self.node.as_deref()),
            ("bun", self.bun.as_deref()),
            ("deno", self.deno.as_deref()),
        ]
        .into_iter()
        .filter_map(|(name, selector)| selector.map(|selector| (name, selector)))
    }
}
