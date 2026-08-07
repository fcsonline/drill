pub mod diag;
pub mod load;
pub mod out;
pub mod plan;
pub mod recursion;
pub mod suggest;
pub mod top;

use diag::Collector;

use load::load_documents;
use recursion::validate_document_tree;
use suggest::suggest_document;
use top::validate_top;

pub const FORMAT_HUMAN: &str = "human";
pub const FORMAT_JSON: &str = "json";

/// Runs the full validation pipeline for a benchmark file and returns the process exit code:
/// `0` clean (warnings/suggestions are non-fatal), `1` when any `error` is present.
///
/// `format` is one of [`FORMAT_HUMAN`] / [`FORMAT_JSON`].
pub fn run(path: &str, format: &str) -> i32 {
  let mut diags = Collector::default();

  let docs = load_documents(path, &mut diags);
  for doc in &docs {
    if doc.is_mapping() {
      validate_top(doc, path, &mut diags);
      validate_document_tree(doc, path, &mut diags);
      suggest_document(doc, path, &mut diags);
    }
  }

  let output = match format {
    FORMAT_JSON => out::render_json(&diags),
    _ => out::render_human(&diags),
  };
  println!("{output}");

  if diags.has_errors() {
    1
  } else {
    0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn valid_file_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("ok.yml");
    std::fs::write(&f, "concurrency: 1\niterations: 1\nplan:\n  - request:\n      url: http://x\n").unwrap();
    assert_eq!(run(f.to_str().unwrap(), FORMAT_HUMAN), 0);
  }

  #[test]
  fn invalid_file_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("bad.yml");
    // no plan -> error
    std::fs::write(&f, "concurrency: 1\niterations: 1\n").unwrap();
    let code = run(f.to_str().unwrap(), FORMAT_JSON);
    assert_eq!(code, 1);
  }

  #[test]
  fn malformed_yaml_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("syntax.yml");
    std::fs::write(&f, "plan: [\n").unwrap();
    assert_eq!(run(f.to_str().unwrap(), FORMAT_HUMAN), 1);
  }
}
