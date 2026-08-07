use std::fmt;

/// Severity levels for a validation diagnostic.
///
/// * `Error` — breaks the run (bad type, missing required field, cross-field invariant).
/// * `Warning` — runnable but risky (unknown top-level key, unnamed plan item, unresolved ref).
/// * `Suggestion` — non-blocking standards guidance from `SYNTAX.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
  Error,
  Warning,
  Suggestion,
}

impl Severity {
  pub fn as_str(self) -> &'static str {
    match self {
      Severity::Error => "error",
      Severity::Warning => "warning",
      Severity::Suggestion => "suggestion",
    }
  }
}

impl fmt::Display for Severity {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.as_str())
  }
}

/// A single validation finding, located by a YAML-path-style `location` (e.g. `plan[3].request.url`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
  pub severity: Severity,
  pub location: String,
  pub message: String,
}

/// Collects diagnostics. The validator never panics on malformed input — it records a diagnostic.
#[derive(Debug, Default)]
pub struct Collector {
  pub items: Vec<Diagnostic>,
}

impl Collector {
  pub fn push(&mut self, severity: Severity, location: &str, message: impl Into<String>) {
    self.items.push(Diagnostic {
      severity,
      location: location.to_string(),
      message: message.into(),
    });
  }

  pub fn error(&mut self, location: &str, message: impl Into<String>) {
    self.push(Severity::Error, location, message);
  }

  pub fn warning(&mut self, location: &str, message: impl Into<String>) {
    self.push(Severity::Warning, location, message);
  }

  pub fn suggestion(&mut self, location: &str, message: impl Into<String>) {
    self.push(Severity::Suggestion, location, message);
  }

  pub fn has_errors(&self) -> bool {
    self.items.iter().any(|d| d.severity == Severity::Error)
  }

  pub fn count(&self, severity: Severity) -> usize {
    self.items.iter().filter(|d| d.severity == severity).count()
  }

  #[cfg(test)]
  pub fn count_all(&self) -> usize {
    self.items.len()
  }

  /// Sort diagnostics by severity (errors first) then by location for stable output.
  pub fn sorted(&self) -> Vec<&Diagnostic> {
    let mut all: Vec<&Diagnostic> = self.items.iter().collect();
    all.sort_by(|a, b| {
      let sa = Severity::rank(a.severity);
      let sb = Severity::rank(b.severity);
      sa.cmp(&sb).then_with(|| a.location.cmp(&b.location))
    });
    all
  }
}

impl Severity {
  fn rank(s: Severity) -> u8 {
    match s {
      Severity::Error => 0,
      Severity::Warning => 1,
      Severity::Suggestion => 2,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sample() -> Collector {
    let mut c = Collector::default();
    c.warning("plan[0]", "no name");
    c.suggestion("plan[0].request", "consider a method");
    c.error("plan", "undefined");
    c
  }

  #[test]
  fn has_errors_reflects_severity() {
    assert!(sample().has_errors());
    let mut c = Collector::default();
    c.warning("x", "y");
    assert!(!c.has_errors());
  }

  #[test]
  fn counts_per_severity() {
    let c = sample();
    assert_eq!(c.count(Severity::Error), 1);
    assert_eq!(c.count(Severity::Warning), 1);
    assert_eq!(c.count(Severity::Suggestion), 1);
    assert_eq!(c.count_all(), 3);
  }

  #[test]
  fn sorted_orders_errors_first() {
    let c = sample();
    let ordered = c.sorted();
    assert_eq!(ordered[0].severity, Severity::Error);
    assert_eq!(ordered[2].severity, Severity::Suggestion);
  }

  #[test]
  fn display_matches_snake_case() {
    assert_eq!(Severity::Error.to_string(), "error");
    assert_eq!(Severity::Suggestion.as_str(), "suggestion");
  }
}
