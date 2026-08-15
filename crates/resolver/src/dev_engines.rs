use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DevEngines {
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub runtime: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub package_manager: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub cpu: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub os: Vec<DevEngineItem>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_one_or_many"
    )]
    pub libc: Vec<DevEngineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevEngineItem {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_fail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevEngineOnFail {
    Error,
    Warn,
    Ignore,
    Download,
}

impl DevEngineItem {
    #[must_use]
    pub fn parsed_name_and_selector(&self) -> (String, Option<String>) {
        if let Some((name, selector)) = self.name.rsplit_once('@') {
            if !name.is_empty() && !selector.is_empty() {
                return (name.to_owned(), Some(selector.to_owned()));
            }
        }
        (self.name.clone(), self.version.clone())
    }

    pub fn on_fail_mode(&self) -> DevEngineOnFail {
        match self
            .on_fail
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("warn") => DevEngineOnFail::Warn,
            Some("ignore") => DevEngineOnFail::Ignore,
            Some("download") => DevEngineOnFail::Download,
            _ => DevEngineOnFail::Error,
        }
    }
}

pub(crate) fn deserialize_one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        One(T),
        Many(Vec<T>),
    }

    Option::<OneOrMany<T>>::deserialize(deserializer).map(|opt| match opt {
        Some(OneOrMany::One(val)) => vec![val],
        Some(OneOrMany::Many(vec)) => vec,
        None => Vec::new(),
    })
}
