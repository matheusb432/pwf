use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    pub action: Option<String>,
}

impl Check {
    pub fn pass(name: &str, detail: impl Into<String>) -> Self {
        Self::new(name, CheckStatus::Pass, detail, None)
    }

    pub fn warning(name: &str, detail: impl Into<String>, action: &str) -> Self {
        Self::new(name, CheckStatus::Warning, detail, Some(action))
    }

    pub fn fail(name: &str, detail: impl Into<String>, action: &str) -> Self {
        Self::new(name, CheckStatus::Fail, detail, Some(action))
    }

    pub fn new(
        name: &str,
        status: CheckStatus,
        detail: impl Into<String>,
        action: Option<&str>,
    ) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
            action: action.map(str::to_owned),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DoctorReport {
    pub version: String,
    pub checks: Vec<Check>,
}

impl DoctorReport {
    #[must_use]
    pub fn failed(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
    }
}
