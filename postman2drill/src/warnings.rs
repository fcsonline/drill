use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning {
    pub level: WarningLevel,
    pub location: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WarningLevel {
    Info,
    Warn,
    Error,
}

impl fmt::Display for WarningLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WarningLevel::Info => write!(f, "INFO"),
            WarningLevel::Warn => write!(f, "WARN"),
            WarningLevel::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Debug, Default)]
pub struct WarningCollector {
    warnings: Vec<Warning>,
}

impl WarningCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, level: WarningLevel, location: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(Warning {
            level,
            location: location.into(),
            message: message.into(),
            original: None,
        });
    }

    #[allow(dead_code)]
    pub fn add_with_original(&mut self, level: WarningLevel, location: impl Into<String>, message: impl Into<String>, original: impl Into<String>) {
        self.warnings.push(Warning {
            level,
            location: location.into(),
            message: message.into(),
            original: Some(original.into()),
        });
    }

    pub fn warn(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.add(WarningLevel::Warn, location, message);
    }

    #[allow(dead_code)]
    pub fn error(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.add(WarningLevel::Error, location, message);
    }

    #[allow(dead_code)]
    pub fn info(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.add(WarningLevel::Info, location, message);
    }

    #[allow(dead_code)]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    pub fn has_errors(&self) -> bool {
        self.warnings.iter().any(|w| w.level == WarningLevel::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.warnings.iter().any(|w| w.level == WarningLevel::Warn)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.warnings).unwrap_or_default()
    }

    pub fn to_text(&self) -> String {
        if self.warnings.is_empty() {
            return "No warnings.".to_string();
        }

        let mut out = String::new();
        for w in &self.warnings {
            out.push_str(&format!("[{}] {}: {}\n", w.level, w.location, w.message));
            if let Some(orig) = &w.original {
                out.push_str(&format!("  Original: {}\n", orig));
            }
        }
        out
    }
}