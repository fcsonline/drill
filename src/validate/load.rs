use serde_yaml::Value;
use std::fs;

use super::diag::Collector;

/// Loads and parses a benchmark YAML file into YAML documents without panicking.
///
/// Mirrors the multi-document split in `crate::reader` (`---\n` separators) but records
/// diagnostics instead of calling `panic!`, so the validator can report every problem
/// instead of aborting on the first one.
pub fn load_documents(path: &str, diags: &mut Collector) -> Vec<Value> {
  let content = match fs::read_to_string(path) {
    Ok(c) => c,
    Err(e) => {
      diags.error(path, format!("cannot read file: {e}"));
      return Vec::new();
    }
  };
  parse_yaml_content(&content, path, diags)
}

fn parse_yaml_content(content: &str, source: &str, diags: &mut Collector) -> Vec<Value> {
  let mut docs = Vec::new();
  let trimmed = content.trim();

  let multi = trimmed.contains("\n---\n") || (trimmed.starts_with("---\n") && trimmed.matches("---\n").count() > 1);

  if multi {
    for part in trimmed.split("---\n") {
      let t = part.trim();
      if t.is_empty() || t.chars().all(|c| c == '#' || c.is_whitespace() || c == '\n') {
        continue;
      }
      match serde_yaml::from_str::<Value>(t) {
        Ok(Value::Null) => {}
        Ok(doc) => docs.push(doc),
        Err(e) => diags.error(source, format!("malformed YAML document: {e}")),
      }
    }
  }

  if docs.is_empty() {
    let single = trimmed.strip_prefix("---\n").unwrap_or(trimmed).trim();
    if !single.is_empty() {
      match serde_yaml::from_str::<Value>(single) {
        Ok(Value::Null) => {}
        Ok(doc) => docs.push(doc),
        Err(e) => diags.error(source, format!("malformed YAML document: {e}")),
      }
    }
  }

  if docs.is_empty() && !diags.has_errors() {
    diags.warning(source, "file contains no YAML documents");
  }

  docs
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse(s: &str) -> (Vec<Value>, Collector) {
    let mut c = Collector::default();
    let docs = parse_yaml_content(s, "test.yml", &mut c);
    (docs, c)
  }

  #[test]
  fn parses_single_document_without_doc_marker() {
    let (docs, c) = parse("concurrency: 4\nplan:\n  - request:\n      url: /\n");
    assert!(docs.len() == 1);
    assert!(!c.has_errors());
  }

  #[test]
  fn parses_leading_doc_marker() {
    let (docs, c) = parse("---\nplan: []\n");
    assert!(docs.len() == 1);
    assert!(!c.has_errors());
  }

  #[test]
  fn parses_multi_document() {
    let (docs, c) = parse("---\na: 1\n---\nb: 2\n");
    assert_eq!(docs.len(), 2);
    assert!(!c.has_errors());
  }

  #[test]
  fn reports_malformed_document_as_error() {
    let (docs, c) = parse("plan: [\n"); // unclosed sequence
    assert!(docs.is_empty());
    assert!(c.has_errors());
  }

  #[test]
  fn empty_content_warns() {
    let (docs, c) = parse("   \n# just a comment\n");
    assert!(docs.is_empty());
    assert!(c.count_all() == 1);
  }
}
