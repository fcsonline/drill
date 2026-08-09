use serde_yaml::Value;

use super::diag::Collector;

const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "head", "delete"];
const AUTH_TYPES: &[&str] = &["basic", "bearer", "oauth2"];

const WITH_ITEMS: &str = "with_items";
const WITH_RANGE: &str = "with_items_range";
const WITH_CSV: &str = "with_items_from_csv";
const WITH_FILE: &str = "with_items_from_file";

/// Validates a plan (sequence of items). `base` is a YAML-path-ish prefix for locations
/// (e.g. `plan` or `lifecycle.setup`).
pub fn validate_plan(plan: &[Value], base: &str, diags: &mut Collector) {
  for (i, item) in plan.iter().enumerate() {
    let loc = format!("{base}[{i}]");
    validate_plan_item(item, &loc, diags);
  }
}

fn validate_plan_item(item: &Value, loc: &str, diags: &mut Collector) {
  if item.get("name").is_none() && item.get("include").is_none() {
    diags.suggestion(loc, "plan item has no `name`; naming items improves report readability");
  }
  if let Some(w) = item.get("weight")
    && w.as_u64().is_none()
  {
    diags.error(loc, "`weight` must be a positive integer");
  }

  // An `include` item is a leaf here; recursion.rs resolves the target file.
  if item.get("include").is_some() {
    if item.get("include").and_then(|v| v.as_str()).is_none() {
      diags.error(loc, "`include` must be a file path string");
    }
    return;
  }

  if is_map(item, "for_each") {
    validate_for_each(item, loc, diags);
    return;
  }

  if let Some(req) = item.get("request") {
    validate_request(item, req, loc, diags);
    return;
  }

  if is_map(item, "delay") {
    validate_delay(item.get("delay").unwrap(), loc, diags);
    return;
  }
  if item.get("exec").is_some() {
    validate_exec(item.get("exec").unwrap(), loc, diags);
    return;
  }
  if is_map(item, "assign") {
    validate_assign(item.get("assign").unwrap(), loc, diags);
    return;
  }
  if is_map(item, "save") {
    validate_save(item.get("save").unwrap(), loc, diags);
    return;
  }
  if is_map(item, "assert") {
    validate_assert(item.get("assert").unwrap(), loc, diags);
    return;
  }

  if let Some(map) = item.as_mapping() {
    let keys: Vec<String> = map.keys().map(|k| k.as_str().unwrap_or("").to_string()).collect();
    if keys.is_empty() {
      diags.error(loc, "plan item is an empty mapping");
    } else {
      diags.error(loc, format!("unrecognized plan item (no known action key; found {keys:?})"));
    }
  } else {
    diags.error(loc, "plan item must be a mapping");
  }
}

fn validate_request(item: &Value, req: &Value, loc: &str, diags: &mut Collector) {
  let Some(req_map) = req.as_mapping() else {
    diags.error(loc, "`request` must be a mapping");
    return;
  };

  let matrix_count = [WITH_ITEMS, WITH_RANGE, WITH_CSV, WITH_FILE].iter().filter(|k| item.get(**k).is_some()).count();
  if matrix_count > 1 {
    diags.error(loc, format!("conflicting matrix discriminators ({matrix_count} of `{WITH_ITEMS}`/`{WITH_RANGE}`/`{WITH_CSV}`/`{WITH_FILE}`) on one request item"));
  }

  match req_map.get("url") {
    None => diags.error(loc, "`request` block requires a `url`"),
    Some(v) if v.as_str().is_none() => diags.error(loc, "`request.url` must be a string"),
    Some(_) => {}
  }

  if let Some(m) = req_map.get("method") {
    match m.as_str() {
      Some(s) => {
        let lower = s.to_lowercase();
        if !HTTP_METHODS.contains(&lower.as_str()) {
          diags.error(loc, format!("invalid HTTP method `{s}` (expected one of {HTTP_METHODS:?})"));
        } else if s != lower {
          diags.warning(loc, format!("method `{s}` should be lowercase (`{lower}`)"));
        }
      }
      None => diags.error(loc, "`request.method` must be a string"),
    }
  }

  if let Some(h) = req_map.get("headers")
    && h.as_mapping().is_none()
  {
    diags.error(loc, "`request.headers` must be a mapping of header -> value");
  }

  if let Some(body) = req_map.get("body") {
    validate_body(body, loc, diags);
    let method = req_map.get("method").and_then(|v| v.as_str()).unwrap_or("get").to_lowercase();
    if method == "get" {
      diags.suggestion(loc, "`request` has a body but no explicit method; if POST is intended, set `method: POST`");
    }
  }

  if let Some(auth) = req_map.get("auth") {
    validate_auth(auth, loc, diags);
  }

  for (k, expect) in [(WITH_ITEMS, "a sequence"), (WITH_RANGE, "a mapping with `start`/`stop`"), (WITH_CSV, "a path string or map"), (WITH_FILE, "a path string or map")] {
    if let Some(v) = item.get(k) {
      let ok = match k {
        WITH_ITEMS => v.as_sequence().is_some(),
        WITH_RANGE => v.as_mapping().is_some(),
        WITH_CSV | WITH_FILE => v.as_str().is_some() || v.as_mapping().is_some(),
        _ => false,
      };
      if !ok {
        diags.error(loc, format!("`{k}` expects {expect}"));
      }
    }
  }
}

fn validate_body(body: &Value, loc: &str, diags: &mut Collector) {
  match body {
    Value::String(_) | Value::Number(_) | Value::Bool(_) => {}
    Value::Mapping(m) => {
      const ALLOWED: &[&str] = &["file", "hex", "urlencoded", "formdata", "graphql"];
      for k in m.keys() {
        let k = k.as_str().unwrap_or("");
        if !ALLOWED.contains(&k) {
          diags.warning(loc, format!("unknown `body.{k}` sub-key (expected {ALLOWED:?})"));
        }
      }
    }
    _ => diags.error(loc, "`request.body` must be a string, number, or body-descriptor mapping"),
  }
}

fn validate_auth(auth: &Value, loc: &str, diags: &mut Collector) {
  let Some(m) = auth.as_mapping() else {
    diags.error(loc, "`request.auth` must be a mapping");
    return;
  };
  match m.get("type").and_then(|v| v.as_str()) {
    Some(t) => {
      let lower = t.to_lowercase();
      if !AUTH_TYPES.contains(&lower.as_str()) {
        diags.error(loc, format!("unsupported auth type `{t}` (expected {AUTH_TYPES:?})"));
      }
    }
    None => diags.error(loc, "`request.auth` requires a `type` field"),
  }
}

fn validate_delay(d: &Value, loc: &str, diags: &mut Collector) {
  let Some(m) = d.as_mapping() else {
    diags.error(loc, "`delay` must be a mapping");
    return;
  };
  match m.get("seconds") {
    Some(Value::Number(_)) => {}
    Some(_) => diags.error(loc, "`delay.seconds` must be a number"),
    None => diags.error(loc, "`delay` requires `seconds`"),
  }
}

fn validate_exec(e: &Value, loc: &str, diags: &mut Collector) {
  let Some(m) = e.as_mapping() else {
    diags.error(loc, "`exec` must be a mapping");
    return;
  };
  match m.get("command") {
    Some(Value::String(_)) => {}
    Some(_) => diags.error(loc, "`exec.command` must be a string"),
    None => diags.error(loc, "`exec` requires `command`"),
  }
}

fn validate_assign(a: &Value, loc: &str, diags: &mut Collector) {
  let Some(m) = a.as_mapping() else {
    diags.error(loc, "`assign` must be a mapping");
    return;
  };
  for key in ["key", "value"] {
    if m.get(key).is_none() {
      diags.error(loc, format!("`assign` requires `{key}`"));
    }
  }
}

fn validate_save(s: &Value, loc: &str, diags: &mut Collector) {
  let Some(m) = s.as_mapping() else {
    diags.error(loc, "`save` must be a mapping");
    return;
  };
  for key in ["source", "jsonpath", "key"] {
    if m.get(key).is_none() {
      diags.error(loc, format!("`save` requires `{key}`"));
    }
  }
}

fn validate_assert(a: &Value, loc: &str, diags: &mut Collector) {
  let Some(m) = a.as_mapping() else {
    diags.error(loc, "`assert` must be a mapping");
    return;
  };
  // Runtime default: `Equals` (compare `context[key]` with `value`) when `type` is absent.
  let Some(ty) = m.get("type").and_then(|v| v.as_str()) else {
    if m.get("key").is_none() || m.get("value").is_none() {
      diags.error(loc, "`assert` (default equals type) requires both `key` and `value`");
    }
    return;
  };
  match ty {
    "status" => {
      if m.get("value").is_none() {
        diags.error(loc, "`assert` type `status` requires a `value` (status code)");
      }
    }
    "header" => {
      if m.get("key").is_none() {
        diags.error(loc, "`assert` type `header` requires `key`");
      }
      if m.get("value").is_none() {
        diags.error(loc, "`assert` type `header` requires `value`");
      }
    }
    "jsonpath" => {
      let op = m.get("operator").and_then(|v| v.as_str()).unwrap_or("eq");
      const OPERATORS: &[&str] = &["eq", "neq", "gt", "gte", "lt", "lte", "contains", "in", "exists", "not_exists", "is_null", "regex"];
      if !OPERATORS.contains(&op) {
        diags.error(loc, format!("unknown jsonpath operator `{op}` (expected {})", OPERATORS.join("|")));
      }
      // `key` is optional (full-document compare when omitted). Presence
      // operators (`exists`, `not_exists`, `is_null`) need no `value`.
      if !matches!(op, "exists" | "not_exists" | "is_null") && m.get("value").is_none() {
        diags.error(loc, format!("`assert` type `jsonpath` with operator `{op}` requires `value`"));
      }
    }
    "duration" => {
      if m.get("value").is_none() {
        diags.error(loc, "`assert` type `duration` requires `value` (max milliseconds)");
      }
    }
    _ => diags.error(loc, format!("unknown assert type `{ty}` (expected status|header|jsonpath|duration)")),
  }
}

fn validate_for_each(item: &Value, loc: &str, diags: &mut Collector) {
  let Some(fe) = item.get("for_each").and_then(|v| v.as_mapping()) else {
    diags.error(loc, "`for_each` must be a mapping");
    return;
  };
  // `for_each.plan` should be a mapping of name -> items; recursion.rs walks it.
  if let Some(p) = fe.get("plan")
    && p.as_mapping().is_none()
    && p.as_sequence().is_none()
  {
    diags.error(loc, "`for_each.plan` must be a mapping of step name -> items");
  }
}

fn is_map(v: &Value, key: &str) -> bool {
  v.get(key).and_then(|x| x.as_mapping()).is_some()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn validate_items(yaml_items: &str) -> Collector {
    let v: Value = serde_yaml::from_str(yaml_items).unwrap();
    let seq = v.as_sequence().unwrap();
    let mut c = Collector::default();
    validate_plan(seq, "plan", &mut c);
    c
  }

  #[test]
  fn valid_request_passes() {
    let c = validate_items("- request:\n    url: /api\n");
    assert!(!c.has_errors());
  }

  #[test]
  fn request_without_url_errors() {
    let c = validate_items("- request:\n    method: POST\n");
    assert!(c.has_errors());
  }

  #[test]
  fn bad_method_errors() {
    let c = validate_items("- request:\n    url: /\n    method: FOO\n");
    assert!(c.has_errors());
  }

  #[test]
  fn uppercase_method_warns_not_errors() {
    let c = validate_items("- request:\n    url: /\n    method: POST\n");
    assert!(!c.has_errors());
    assert_eq!(c.count(crate::validate::diag::Severity::Warning), 1);
  }

  #[test]
  fn conflicting_matrix_errors() {
    let c = validate_items("- request:\n    url: /\n  with_items: [1]\n  with_items_range:\n    start: 1\n    stop: 2\n");
    assert!(c.has_errors());
  }

  #[test]
  fn auth_bad_type_errors() {
    let c = validate_items("- request:\n    url: /\n    auth:\n      type: nope\n");
    assert!(c.has_errors());
  }

  #[test]
  fn unknown_action_errors() {
    let c = validate_items("- bogus: 1\n");
    assert!(c.has_errors());
  }

  #[test]
  fn jsonpath_without_key_or_value_passes() {
    let c = validate_items("- assert:\n    type: jsonpath\n    operator: exists\n");
    assert!(!c.has_errors(), "unexpected errors: {c:?}");
  }

  #[test]
  fn jsonpath_value_operator_needs_value() {
    let c = validate_items("- assert:\n    type: jsonpath\n    operator: gte\n    key: '$.data.count'\n");
    assert!(c.has_errors());
  }

  #[test]
  fn jsonpath_unknown_operator_errors() {
    let c = validate_items("- assert:\n    type: jsonpath\n    key: '$.x'\n    operator: matches\n    value: 1\n");
    assert!(c.has_errors());
  }

  #[test]
  fn jsonpath_typed_value_passes() {
    let c = validate_items("- assert:\n    type: jsonpath\n    key: '$.data.count'\n    operator: gte\n    value: 10\n");
    assert!(!c.has_errors(), "unexpected errors: {c:?}");
  }
}
