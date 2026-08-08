use serde_yaml::Value;

use super::diag::Collector;

/// Top-level benchmark keys recognized by `config.rs`.
const TOP_LEVEL_KEYS: &[&str] = &["base", "iterations", "concurrency", "rampup", "threads", "new_conn_per_iter", "persist_context", "run_time", "success_codes", "results", "lifecycle", "load_shape", "arrival_rate", "vars", "plan"];

const LIFECYCLE_HOOKS: &[&str] = &["setup", "teardown", "iteration_start", "iteration_stop"];

/// Validates the top-level mapping: known keys, presence of `plan`, scalar types and signs,
/// and the `concurrency <= iterations` (absent `load_shape`) cross-field invariant.
pub fn validate_top(doc: &Value, source: &str, diags: &mut Collector) {
  let map = match doc.as_mapping() {
    Some(m) => m,
    None => {
      diags.error(source, "top-level document must be a YAML mapping");
      return;
    }
  };

  for key in map.keys() {
    let key = key.as_str().unwrap_or("");
    if !TOP_LEVEL_KEYS.contains(&key) {
      diags.warning(source, format!("unknown top-level key `{key}`"));
    }
  }

  let has_load_shape = map.contains_key(serde_yaml::Value::String("load_shape".into()));

  let iterations = as_i64(map, "iterations");
  let concurrency = as_i64(map, "concurrency");

  review_nonnegative(map, "rampup", diags, source);
  review_nonnegative(map, "run_time", diags, source);
  review_count(map, "iterations", diags, source, iterations);
  review_count(map, "concurrency", diags, source, concurrency);

  if let Some(threads) = map.get("threads") {
    match threads.as_i64() {
      Some(n) if n >= 1 => {}
      Some(_) => diags.error(source, "`threads` must be >= 1"),
      None => diags.error(source, "`threads` must be an integer"),
    }
  }

  if let Some(sc) = map.get("success_codes") {
    match sc.as_sequence() {
      Some(seq) => {
        for (i, v) in seq.iter().enumerate() {
          match v.as_i64() {
            Some(n) if n > 0 => {}
            _ => diags.error(source, format!("`success_codes[{i}]` must be a positive integer")),
          }
        }
      }
      None => diags.error(source, "`success_codes` must be a list of integers"),
    }
  }

  if let Some(plan) = map.get("plan") {
    match plan.as_sequence() {
      Some(seq) if !seq.is_empty() => {}
      Some(_) => diags.error(source, "`plan` must contain at least one item"),
      None => diags.error(source, "`plan` must be a list of benchmark items"),
    }
  } else {
    diags.error(source, "missing required top-level key `plan`");
  }

  // Cross-field invariant: concurrency > iterations is a runtime panic unless load_shape varies it.
  if !has_load_shape
    && let (Some(c), Some(i)) = (concurrency, iterations)
    && c > i
  {
    diags.error(source, format!("`concurrency` ({c}) exceeds `iterations` ({i}) without a `load_shape`; the runtime aborts"));
  }

  validate_arrival_rate(map, source, diags);
  validate_success_and_results(map, source, diags);
}

fn validate_success_and_results(map: &serde_yaml::Mapping, source: &str, diags: &mut Collector) {
  if let Some(res) = map.get("results") {
    match res {
      Value::String(_) => {}
      Value::Mapping(m) => {
        for key in m.keys() {
          let k = key.as_str().unwrap_or("");
          if k.is_empty() || !["output_dir", "csv", "html"].contains(&k) {
            diags.warning(source, format!("unknown `results` key `{k}`"));
          }
        }
      }
      _ => diags.error(source, "`results` must be a path string or a map with `output_dir`/`csv`/`html`"),
    }
  }
}

fn as_i64(map: &serde_yaml::Mapping, key: &str) -> Option<i64> {
  map.get(key).and_then(|v| v.as_i64())
}

fn review_count(map: &serde_yaml::Mapping, key: &str, diags: &mut Collector, source: &str, val: Option<i64>) {
  match val {
    Some(n) if n >= 1 => {}
    Some(_) => diags.error(source, format!("`{key}` must be >= 1")),
    None => {
      if map.contains_key(serde_yaml::Value::String(key.into())) {
        diags.error(source, format!("`{key}` must be an integer"));
      }
    }
  }
}

fn review_nonnegative(map: &serde_yaml::Mapping, key: &str, diags: &mut Collector, source: &str) {
  if let Some(v) = map.get(key) {
    match v.as_i64() {
      Some(n) if n >= 0 => {}
      Some(_) => diags.error(source, format!("`{key}` must be >= 0")),
      None => diags.error(source, format!("`{key}` must be an integer")),
    }
  }
}

pub fn validate_lifecycle(doc: &Value, source: &str, diags: &mut Collector) {
  let Some(lc) = doc.get("lifecycle") else {
    return;
  };
  let Some(map) = lc.as_mapping() else {
    diags.error(source, "`lifecycle` must be a mapping of phase names to plan items");
    return;
  };
  for key in map.keys() {
    let k = key.as_str().unwrap_or("");
    if !LIFECYCLE_HOOKS.contains(&k) {
      diags.warning(source, format!("unknown lifecycle phase `{k}` (expected one of {LIFECYCLE_HOOKS:?})"));
    }
  }
}

/// Arrival-rate keys that Drill does not implement yet; presenting them means
/// the author asked for behavior the runtime will not honor, so reject them.
const DEFERRED_ARRIVAL_KEYS: [&str; 4] = ["on_ceiling", "queue", "block", "preallocated"];

fn review_positive(map: &serde_yaml::Mapping, key: &str, diags: &mut Collector, source: &str) {
  if let Some(v) = map.get(key) {
    match v.as_i64() {
      Some(n) if n >= 1 => {}
      Some(_) => diags.error(source, format!("`{key}` must be >= 1")),
      None => diags.error(source, format!("`{key}` must be an integer")),
    }
  }
}

/// Validates the `arrival_rate` mapping: mutual exclusion with closed-model
/// knobs (AC10), a finite budget (AC4), a required `max_concurrency` (AC5),
/// and rejection of deferred drop-policy keys (AC11).
fn validate_arrival_rate(map: &serde_yaml::Mapping, source: &str, diags: &mut Collector) {
  let Some(v) = map.get("arrival_rate") else {
    return;
  };
  let Some(ar) = v.as_mapping() else {
    diags.error(source, "`arrival_rate` must be a mapping");
    return;
  };

  for conflicting in ["concurrency", "iterations", "rampup", "load_shape"] {
    if map.contains_key(serde_yaml::Value::String(conflicting.into())) {
      diags.error(source, format!("`arrival_rate` can not be combined with `{conflicting}`"));
    }
  }

  let has_duration = ar.contains_key(serde_yaml::Value::String("duration".into()));
  let has_max_iterations = ar.contains_key(serde_yaml::Value::String("max_iterations".into()));
  if !has_duration && !has_max_iterations {
    diags.error(source, "`arrival_rate` requires at least one of `duration` or `max_iterations`");
  }

  if ar.contains_key(serde_yaml::Value::String("max_concurrency".into())) {
    review_positive(ar, "max_concurrency", diags, source);
  } else {
    diags.error(source, "`arrival_rate.max_concurrency` is required");
  }

  for deferred in DEFERRED_ARRIVAL_KEYS {
    if ar.contains_key(serde_yaml::Value::String(deferred.into())) {
      diags.error(source, format!("`arrival_rate.{deferred}` is not yet supported"));
    }
  }

  if let Some(d) = ar.get("duration") {
    match d.as_i64() {
      Some(n) if n >= 1 => {}
      Some(_) => diags.error(source, "`arrival_rate.duration` must be >= 1"),
      None => diags.error(source, "`arrival_rate.duration` must be an integer"),
    }
  }
  if let Some(m) = ar.get("max_iterations") {
    match m.as_i64() {
      Some(n) if n >= 1 => {}
      Some(_) => diags.error(source, "`arrival_rate.max_iterations` must be >= 1"),
      None => diags.error(source, "`arrival_rate.max_iterations` must be an integer"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::validate::diag::Severity;
  use serde_yaml::Value;

  fn doc(yaml: &str) -> Value {
    serde_yaml::from_str(yaml).unwrap()
  }

  fn run(yaml: &str) -> Collector {
    let mut c = Collector::default();
    validate_top(&doc(yaml), "t.yml", &mut c);
    c
  }

  #[test]
  fn valid_benchmark_has_no_errors() {
    let c = run("concurrency: 4\niterations: 5\nplan:\n  - request:\n      url: /\n");
    assert!(!c.has_errors());
  }

  #[test]
  fn missing_plan_is_error() {
    let c = run("concurrency: 4\niterations: 5\n");
    assert!(c.has_errors());
  }

  #[test]
  fn empty_plan_is_error() {
    let c = run("concurrency: 4\niterations: 5\nplan: []\n");
    assert!(c.has_errors());
  }

  #[test]
  fn concurrency_over_iterations_without_load_shape_errors() {
    let c = run("concurrency: 10\niterations: 5\nplan:\n  - request:\n      url: /\n");
    assert!(c.has_errors());
  }

  #[test]
  fn load_shape_relaxes_concurrency_invariant() {
    let c = run("concurrency: 10\niterations: 5\nload_shape:\n  stages:\n    - duration: 10\n      users: 10\nplan:\n  - request:\n      url: /\n");
    assert!(!c.has_errors());
  }

  #[test]
  fn zero_iterations_errors() {
    let c = run("concurrency: 0\niterations: 0\nplan:\n  - request:\n      url: /\n");
    assert!(c.has_errors());
  }

  #[test]
  fn unknown_top_level_key_warns_not_errors() {
    let c = run("concurrency: 1\niterations: 1\nbogus: true\nplan:\n  - request:\n      url: /\n");
    assert!(!c.has_errors());
    assert!(c.count(Severity::Warning) == 1);
  }

  #[test]
  fn success_codes_validate() {
    let c = run("success_codes: [200, 0, -1]\nplan:\n  - request:\n      url: /\n");
    assert!(c.has_errors());
  }

  #[test]
  fn arrival_rate_valid_is_clean() {
    let c = run("arrival_rate:\n  rate: 10\n  duration: 5\n  max_concurrency: 100\nplan:\n  - request:\n      url: /\n");
    assert!(!c.has_errors(), "unexpected errors: {c:?}");
  }

  #[test]
  fn arrival_rate_mutually_exclusive_with_closed_knobs() {
    for conflicting in ["concurrency: 4\n", "iterations: 4\n", "rampup: 2\n"] {
      let c = run(&format!("arrival_rate:\n  rate: 10\n  duration: 5\n  max_concurrency: 100\n{conflicting}plan:\n  - request:\n      url: /\n"));
      assert!(c.has_errors(), "expected error for `{conflicting}`");
    }
  }

  #[test]
  fn arrival_rate_mutually_exclusive_with_load_shape() {
    let c = run("arrival_rate:\n  rate: 10\n  duration: 5\n  max_concurrency: 100\nload_shape:\n  stages:\n    - duration: 10\n      users: 5\nplan:\n  - request:\n      url: /\n");
    assert!(c.has_errors());
  }

  #[test]
  fn arrival_rate_requires_a_budget() {
    let c = run("arrival_rate:\n  rate: 10\n  max_concurrency: 100\nplan:\n  - request:\n      url: /\n");
    assert!(c.has_errors());
  }

  #[test]
  fn arrival_rate_max_concurrency_is_required() {
    let c = run("arrival_rate:\n  rate: 10\n  duration: 5\nplan:\n  - request:\n      url: /\n");
    assert!(c.has_errors());
  }

  #[test]
  fn arrival_rate_deferred_keys_are_rejected() {
    for deferred in ["on_ceiling", "queue", "block", "preallocated"] {
      let c = run(&format!("arrival_rate:\n  rate: 10\n  duration: 5\n  max_concurrency: 100\n  {deferred}: drop\nplan:\n  - request:\n      url: /\n"));
      assert!(c.has_errors(), "expected error for deferred key `{deferred}`");
    }
  }

  #[test]
  fn arrival_rate_invalid_types_error() {
    let c = run("arrival_rate:\n  rate: 10\n  duration: 0\n  max_concurrency: 0\nplan:\n  - request:\n      url: /\n");
    assert!(c.has_errors());
  }
}
