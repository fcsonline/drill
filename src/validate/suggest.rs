use serde_yaml::Value;

use super::diag::Collector;

const WITH_ITEMS: &str = "with_items";
const WITH_RANGE: &str = "with_items_range";
const WITH_CSV: &str = "with_items_from_csv";
const WITH_FILE: &str = "with_items_from_file";

/// Standards suggestions that need document-wide context:
/// - relative `url` without a top-level `base`
/// - `{{ item }}` / `{{ index }}` interpolation outside a matrix/`for_each` scope
pub fn suggest_document(doc: &Value, source: &str, diags: &mut Collector) {
  let has_base = doc.get("base").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
  walk_for_suggestions(doc, source, has_base, diags);
}

fn walk_for_suggestions(value: &Value, source: &str, has_base: bool, diags: &mut Collector) {
  match value {
    Value::Mapping(map) => {
      if let Some(req) = map.get("request").and_then(|v| v.as_mapping())
        && let Some(url) = req.get("url").and_then(|v| v.as_str())
      {
        if url.starts_with('/') && !has_base {
          diags.warning(source, "relative `request.url` has no top-level `base`; URLs resolve against the current base or fail");
        }
        if url.contains("{{ item }}") || url.contains("{{ index }}") {
          let has_matrix = [WITH_ITEMS, WITH_RANGE, WITH_CSV, WITH_FILE].iter().any(|k| map.get(*k).is_some());
          let inside_for_each = contains_for_each_ancestor(map);
          if !has_matrix && !inside_for_each {
            diags.warning(source, "`{{ item }}`/`{{ index }}` interpolation outside a `with_items`/`for_each` scope may stay unresolved");
          }
        }
      }
      for v in map.values() {
        walk_for_suggestions(v, source, has_base, diags);
      }
    }
    Value::Sequence(seq) => {
      for v in seq {
        walk_for_suggestions(v, source, has_base, diags);
      }
    }
    _ => {}
  }
}

/// Cheap heuristic: the request mapping sits inside a `for_each` mapping if a `for_each`
/// sibling key exists at the same level (the plan item that owns the request).
fn contains_for_each_ancestor(map: &serde_yaml::Mapping) -> bool {
  map.contains_key(serde_yaml::Value::String("for_each".into()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn run(yaml: &str) -> Collector {
    let doc: Value = serde_yaml::from_str(yaml).unwrap();
    let mut c = Collector::default();
    suggest_document(&doc, "t.yml", &mut c);
    c
  }

  #[test]
  fn relative_url_without_base_warns() {
    let c = run("plan:\n  - request:\n      url: /api\n");
    assert!(!c.has_errors());
    assert_eq!(c.count(crate::validate::diag::Severity::Warning), 1);
  }

  #[test]
  fn relative_url_with_base_is_clean() {
    let c = run("base: http://localhost:9000\nplan:\n  - request:\n      url: /api\n");
    assert_eq!(c.count_all(), 0);
  }

  #[test]
  fn item_ref_outside_matrix_warns() {
    let c = run("base: http://x\nplan:\n  - request:\n      url: /api/{{ item }}\n");
    assert_eq!(c.count(crate::validate::diag::Severity::Warning), 1);
  }

  #[test]
  fn item_ref_inside_matrix_is_clean() {
    let c = run("base: http://x\nplan:\n  - request:\n      url: /api/{{ item }}\n    with_items: [1, 2]\n");
    assert_eq!(c.count_all(), 0);
  }
}
