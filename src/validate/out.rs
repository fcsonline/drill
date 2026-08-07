use serde_json::{Value as Json, json};

use super::diag::{Collector, Severity};

/// Renders diagnostics to human-readable text (default) on stdout.
pub fn render_human(diags: &Collector) -> String {
  let ordered = diags.sorted();
  let mut out = String::new();
  if ordered.is_empty() {
    out.push_str("OK — no errors, warnings, or suggestions found.\n");
    return out;
  }
  out.push_str("Validation results:\n");
  for d in &ordered {
    let tag = match d.severity {
      Severity::Error => "error",
      Severity::Warning => "warning",
      Severity::Suggestion => "suggestion",
    };
    out.push_str(&format!("  [{tag:>9}] {loc}: {msg}\n", loc = d.location, msg = d.message));
  }
  out.push('\n');
  out.push_str(&format!("{} error(s), {} warning(s), {} suggestion(s)\n", diags.count(Severity::Error), diags.count(Severity::Warning), diags.count(Severity::Suggestion)));
  out
}

/// Renders diagnostics as a JSON array for machine consumption.
pub fn render_json(diags: &Collector) -> String {
  let arr: Vec<Json> = diags
    .sorted()
    .into_iter()
    .map(|d| {
      json!({
        "severity": d.severity.as_str(),
        "location": d.location,
        "message": d.message,
      })
    })
    .collect();
  serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sample() -> Collector {
    let mut c = Collector::default();
    c.error("plan[0].request", "missing url");
    c.warning("plan[0]", "no name");
    c.suggestion("plan[0].request.method", "consider POST");
    c
  }

  #[test]
  fn human_empty_ok() {
    assert!(render_human(&Collector::default()).starts_with("OK"));
  }

  #[test]
  fn human_summary_counts() {
    let s = render_human(&sample());
    assert!(s.contains("1 error(s), 1 warning(s), 1 suggestion(s)"));
  }

  #[test]
  fn json_is_strict_array() {
    let v: Json = serde_json::from_str(&render_json(&sample()).to_string()).unwrap();
    assert!(v.is_array());
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["severity"], json!("error"));
    assert!(arr[0]["location"].is_string());
    assert!(arr[0]["message"].is_string());
  }
}
