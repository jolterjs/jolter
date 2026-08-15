use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub status: CheckStatus,
    pub name: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl Check {
    pub(crate) fn pass(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Pass,
            name,
            message: message.into(),
            remediation: None,
        }
    }

    pub(crate) fn warning(
        name: &'static str,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            status: CheckStatus::Warning,
            name,
            message: message.into(),
            remediation: Some(remediation.into()),
        }
    }

    pub(crate) fn fail(
        name: &'static str,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            status: CheckStatus::Fail,
            name,
            message: message.into(),
            remediation: Some(remediation.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.checks
            .iter()
            .all(|check| check.status != CheckStatus::Fail)
    }
}
