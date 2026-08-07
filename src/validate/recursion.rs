use serde_yaml::Value;
use std::path::{Path, PathBuf};

use super::diag::Collector;
use super::load::load_documents;
use super::plan::validate_plan;
use super::top::validate_lifecycle;

const MAX_DEPTH: usize = 16;

/// Validates a benchmark document and everything it recursively includes.
pub fn validate_document_tree(doc: &Value, source_path: &str, diags: &mut Collector) {
  let mut visited: Vec<PathBuf> = Vec::new();
  visit_doc(doc, source_path, diags, 0, &mut visited);
}

fn visit_doc(doc: &Value, source_path: &str, diags: &mut Collector, depth: usize, visited: &mut Vec<PathBuf>) {
  if depth > MAX_DEPTH {
    diags.error(source_path, format!("maximum include depth ({MAX_DEPTH}) exceeded; possible include cycle"));
    return;
  }

  let canonical = std::fs::canonicalize(source_path).unwrap_or_else(|_| source_path.into());
  if visited.contains(&canonical) {
    diags.error(source_path, "cyclic `include` detected (file already in the include stack)");
    return;
  }
  visited.push(canonical);

  if let Some(map) = doc.as_mapping() {
    if let Some(plan) = map.get("plan").and_then(|v| v.as_sequence()) {
      validate_plan(plan, "plan", diags);
    }
    validate_lifecycle(doc, source_path, diags);
    if let Some(lifecycle) = map.get("lifecycle").and_then(|v| v.as_mapping()) {
      for (hook_name, hook_items) in lifecycle {
        if let Some(seq) = hook_items.as_sequence() {
          let base = format!("lifecycle.{}", hook_name.as_str().unwrap_or(""));
          validate_plan(seq, &base, diags);
        }
      }
    }
  }

  // Resolve every `include: <file>` anywhere in the tree, relative to the source file's directory.
  let dir = source_dir(source_path);
  resolve_includes_in_value(doc, &dir, diags, depth, visited);

  visited.pop();
}

/// Walk the YAML tree locating every `include` key and validating the target file.
fn resolve_includes_in_value(value: &Value, source_dir: &Path, diags: &mut Collector, depth: usize, visited: &mut Vec<PathBuf>) {
  match value {
    Value::Mapping(map) => {
      if let Some(inc) = map.get("include").and_then(|v| v.as_str()) {
        let target = source_dir.join(inc);
        validate_include_file(&target, diags, depth, visited);
      }
      for v in map.values() {
        resolve_includes_in_value(v, source_dir, diags, depth, visited);
      }
    }
    Value::Sequence(seq) => {
      for v in seq {
        resolve_includes_in_value(v, source_dir, diags, depth, visited);
      }
    }
    _ => {}
  }
}

fn validate_include_file(target: &Path, diags: &mut Collector, depth: usize, visited: &mut Vec<PathBuf>) {
  let target_str = target.to_string_lossy().into_owned();
  if !target.exists() {
    diags.error(&target_str, "included file does not exist");
    return;
  }
  let docs = load_documents(&target_str, diags);
  for doc in docs {
    visit_doc(&doc, &target_str, diags, depth + 1, visited);
  }
}

fn source_dir(source_path: &str) -> PathBuf {
  Path::new(source_path).parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::tempdir;

  #[test]
  fn include_cycle_detected() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("seed.yml");
    std::fs::write(&path, "plan:\n  - include: seed.yml\n").unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let docs: Value = serde_yaml::from_str(&content).unwrap();
    let mut c = Collector::default();
    validate_document_tree(&docs, path.to_str().unwrap(), &mut c);
    assert!(c.has_errors());
  }

  #[test]
  fn missing_include_file_errors() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("seed.yml");
    std::fs::write(&path, "plan:\n  - include: nope.yml\n").unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let docs: Value = serde_yaml::from_str(&content).unwrap();
    let mut c = Collector::default();
    validate_document_tree(&docs, path.to_str().unwrap(), &mut c);
    assert!(c.has_errors());
  }
}
