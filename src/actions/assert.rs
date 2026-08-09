use async_trait::async_trait;
use colored::*;
use regex::Regex;
use serde_json::{Value, json};
use serde_yaml::Value as YamlValue;

use crate::actions::request::yaml_to_json;
use crate::actions::save::LAST_RESPONSE_KEY;
use crate::actions::{Runnable, extract, extract_optional};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

/// The kind of assertion to run. When no `type` is present in the YAML, the
/// historical `Equals` behavior is kept (compare a context value with a
/// literal). The other kinds assert against the last HTTP response, stored in
/// the context as `_last_response` by `Request::execute`.
#[derive(Clone, Debug, PartialEq)]
enum AssertType {
  /// Compare `context[key]` against `value` (historical behavior).
  Equals,
  /// Assert the last response status code is one of the given codes.
  Status,
  /// Assert a response header value contains `value` (case-insensitive).
  Header,
  /// Assert the first JSONPath match on the response body equals `value`.
  JsonPath,
  /// Assert the last response duration (ms) satisfies `operator` against the
  /// maximum `value`.
  Duration,
}

#[derive(Clone)]
pub struct Assert {
  name: String,
  key: String,
  /// String-typed expected value (used by `Equals` and `Header`).
  value: String,
  /// Typed expected value for `jsonpath` assertions, parsed from any YAML
  /// scalar or collection into `serde_json::Value` (R3.1).
  json_value: Option<Value>,
  assert_type: AssertType,
  status_codes: Vec<u16>,
  duration_max: u64,
  /// Comparison operator. `duration` accepts lt/lte/gt/gte/eq; `jsonpath`
  /// accepts eq/neq/gt/gte/lt/lte/contains/in/exists/not_exists/is_null/regex.
  operator: String,
  /// When true, `jsonpath` assertions apply to every match (R3.3).
  every: bool,
  /// Expected number of JSONPath matches (R3.3).
  match_count: Option<u64>,
  match_count_operator: String,
  /// Precompiled regex for `operator: regex` — fails fast at parse time.
  regex: Option<Regex>,
  weight: u32,
}

impl Assert {
  pub fn is_that_you(item: &YamlValue) -> bool {
    item.get("assert").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &YamlValue, _with_item: Option<YamlValue>) -> Assert {
    let name = extract(item, "name");
    let assert_val = item.get("assert").expect("assert field is required");
    let weight = item.get("weight").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(1);

    let assert_type = match assert_val.get("type").and_then(|v| v.as_str()) {
      Some("status") => AssertType::Status,
      Some("header") => AssertType::Header,
      Some("jsonpath") => AssertType::JsonPath,
      Some("duration") => AssertType::Duration,
      Some(other) => panic!("Unknown assert type '{other}'. Supported types: status, header, jsonpath, duration"),
      None => AssertType::Equals,
    };

    let mut assert = Assert {
      name,
      key: String::new(),
      value: String::new(),
      json_value: None,
      assert_type,
      status_codes: Vec::new(),
      duration_max: 0,
      operator: String::new(),
      every: false,
      match_count: None,
      match_count_operator: String::new(),
      regex: None,
      weight,
    };

    match assert.assert_type {
      AssertType::Equals | AssertType::Header => {
        assert.key = extract(assert_val, "key");
        assert.value = extract(assert_val, "value");
      }
      AssertType::Status => {
        let value = assert_val.get("value").expect("assert value is required");
        assert.status_codes = parse_status_codes(value);
      }
      AssertType::Duration => {
        let value = assert_val.get("value").expect("assert value is required");
        assert.duration_max = value.as_u64().unwrap_or_else(|| panic!("Duration value must be a positive integer (ms), got {:?}", value));
        assert.operator = assert_val.get("operator").and_then(|v| v.as_str()).unwrap_or("lt").to_string();
      }
      AssertType::JsonPath => {
        assert.key = extract_optional(assert_val, "key").unwrap_or_default();
        assert.operator = assert_val.get("operator").and_then(|v| v.as_str()).unwrap_or("eq").to_string();
        assert.every = assert_val.get("every").and_then(|v| v.as_bool()).unwrap_or(false);
        let (match_count, match_count_operator) = parse_match_count(assert_val.get("match_count"));
        assert.match_count = match_count;
        assert.match_count_operator = match_count_operator;

        const SUPPORTED: &[&str] = &["eq", "neq", "gt", "gte", "lt", "lte", "contains", "in", "exists", "not_exists", "is_null", "regex"];
        if !SUPPORTED.contains(&assert.operator.as_str()) {
          panic!("Unknown JsonPath operator '{}'. Supported operators: {}", assert.operator, SUPPORTED.join(", "));
        }

        if assert.operator == "regex" {
          let pattern = assert_val.get("value").and_then(|v| v.as_str()).unwrap_or_else(|| panic!("Regex operator requires a string 'value'"));
          assert.regex = Some(Regex::new(pattern).unwrap_or_else(|e| panic!("Invalid regex '{}': {e}", pattern)));
        } else if let Some(value) = assert_val.get("value") {
          assert.json_value = Some(yaml_to_json(value.clone()));
        }
      }
    }

    assert
  }

  /// Human-readable description of the assertion, used in non-quiet output.
  fn describe(&self) -> String {
    match self.assert_type {
      AssertType::Equals => format!("{}={}?", self.key.cyan().bold(), self.value.magenta()),
      AssertType::Status => format!("status in {:?}", self.status_codes),
      AssertType::Header => format!("header[{}] contains '{}'", self.key.cyan().bold(), self.value.magenta()),
      AssertType::JsonPath => {
        let target = if self.key.is_empty() {
          "<body>".to_string()
        } else {
          self.key.clone()
        };
        let expected = self.json_value.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "∅".to_string());
        let scope = if self.every {
          " (every match)"
        } else {
          ""
        };
        let count = self.match_count.map(|c| format!(", match_count {} {c}", self.match_count_operator)).unwrap_or_default();
        format!("jsonpath[{}] {} {}?{}{}", target.cyan().bold(), self.operator, expected.magenta(), scope, count)
      }
      AssertType::Duration => format!("duration {} {}", self.operator, self.duration_max),
    }
  }

  fn last_response(&self, context: &Context) -> Value {
    context.get(LAST_RESPONSE_KEY).cloned().unwrap_or_else(|| panic!("Assert needs a previous request: no '{}' entry in the context.", LAST_RESPONSE_KEY))
  }

  fn execute_equals(&self, context: &Context) {
    let interpolator = interpolator::Interpolator::new(context);
    let eval = format!("{{{{ {} }}}}", self.key);
    let stored = interpolator.resolve(&eval, true);
    let assertion = json!(self.value.to_owned());

    if !stored.eq(&assertion) {
      panic!("Assertion mismatched: {stored} != {assertion}");
    }
  }

  fn execute_status(&self, context: &Context) {
    let last_response = self.last_response(context);
    let status = last_response.get("status").and_then(|v| v.as_u64()).map(|v| v as u16).unwrap_or_else(|| {
      panic!("Status assertion needs an integer 'status' in the last response, got {:?}", last_response.get("status"));
    });

    if !self.status_codes.contains(&status) {
      panic!("Status assertion mismatched: expected {:?}, got {status}", self.status_codes);
    }
  }

  fn execute_header(&self, context: &Context) {
    let last_response = self.last_response(context);
    let headers = last_response.get("headers").unwrap_or_else(|| panic!("Header assertion needs 'headers' in the last response"));

    let header_value = headers.get(self.key.to_lowercase()).or_else(|| headers.as_object()?.iter().find(|(name, _)| name.eq_ignore_ascii_case(&self.key)).map(|(_, value)| value)).and_then(|v| v.as_str());

    match header_value {
      Some(value) => {
        if !value.to_lowercase().contains(&self.value.to_lowercase()) {
          panic!("Header assertion mismatched: header '{}' value '{value}' does not contain '{}'", self.key, self.value);
        }
      }
      None => panic!("Header assertion mismatched: header '{}' not found in the last response", self.key),
    }
  }

  fn execute_jsonpath(&self, context: &Context) {
    let last_response = self.last_response(context);
    let body = last_response.get("body").and_then(|v| v.as_str()).unwrap_or_else(|| {
      panic!("JsonPath assertion needs a string 'body' in the last response, got {:?}", last_response.get("body"));
    });

    let body_json: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("JsonPath assertion failed: response body is not valid JSON: {e}"));

    let path = if self.key.is_empty() {
      "<body>".to_string()
    } else {
      self.key.clone()
    };
    let matches: Vec<&Value> = if self.key.is_empty() {
      vec![&body_json]
    } else {
      jsonpath_lib::select(&body_json, &self.key).unwrap_or_else(|e| panic!("JsonPath assertion failed: invalid path '{}': {e}", self.key))
    };

    match self.operator.as_str() {
      "exists" => {
        if matches.is_empty() {
          panic!("JsonPath assertion mismatched: no match for path '{path}'");
        }
      }
      "not_exists" => {
        if !matches.is_empty() {
          panic!("JsonPath assertion mismatched: expected no matches for path '{path}', found {}", matches.len());
        }
      }
      _ => {
        if matches.is_empty() {
          panic!("JsonPath assertion mismatched: no match for path '{path}'");
        }
      }
    }

    if let Some(expected) = self.match_count {
      let actual = matches.len() as u64;
      if !number_ok(actual as f64, &self.match_count_operator, expected as f64) {
        panic!("JsonPath assertion mismatched: match_count {} {} not satisfied, found {actual} matches", self.match_count_operator, expected);
      }
    }

    if matches!(self.operator.as_str(), "exists" | "not_exists") {
      return;
    }

    let needs_value = matches!(self.operator.as_str(), "eq" | "neq" | "gt" | "gte" | "lt" | "lte" | "contains" | "in");
    let expected = if needs_value {
      self.json_value.clone().unwrap_or_else(|| panic!("JsonPath operator '{}' requires a 'value'", self.operator))
    } else {
      Value::Null
    };

    let candidates: Vec<&Value> = if self.every {
      matches.clone()
    } else {
      matches.iter().take(1).copied().collect()
    };

    for found in candidates {
      if !self.value_ok(found, &expected) {
        let actual = as_plain_string(found);
        panic!("JsonPath assertion mismatched: value '{actual}' does not satisfy {} {} (path '{path}')", self.operator, expected);
      }
    }
  }

  fn value_ok(&self, found: &Value, expected: &Value) -> bool {
    match self.operator.as_str() {
      "eq" => {
        // A YAML string expected value keeps the historical loose compare
        // (number 123 matches literal "123"); any other typed value compares
        // structurally (R3.1: strict-by-default, backward compatible).
        if expected.is_string() {
          value_eq(found, expected)
        } else {
          found == expected
        }
      }
      "neq" => {
        if expected.is_string() {
          !value_eq(found, expected)
        } else {
          found != expected
        }
      }
      "gt" => number_ok_value(found, "gt", expected),
      "gte" => number_ok_value(found, "gte", expected),
      "lt" => number_ok_value(found, "lt", expected),
      "lte" => number_ok_value(found, "lte", expected),
      "contains" => contains_value(found, expected),
      "in" => expected.as_array().is_some_and(|arr| arr.iter().any(|e| e == found)),
      "is_null" => found.is_null(),
      "regex" => self.regex.as_ref().is_some_and(|re| re.is_match(&as_plain_string(found))),
      _ => false,
    }
  }

  fn execute_duration(&self, context: &Context) {
    let last_response = self.last_response(context);
    let duration = last_response.get("duration").and_then(|v| v.as_f64()).unwrap_or_else(|| {
      panic!("Duration assertion needs a 'duration' in the last response, got {:?}", last_response.get("duration"));
    });
    let max = self.duration_max as f64;

    let ok = match self.operator.as_str() {
      "lt" => duration < max,
      "lte" => duration <= max,
      "gt" => duration > max,
      "gte" => duration >= max,
      "eq" => duration == max,
      other => panic!("Unknown duration operator '{other}'. Supported operators: lt, lte, gt, gte, eq"),
    };

    if !ok {
      panic!("Duration assertion mismatched: {duration}ms is not {} {max}ms", self.operator);
    }
  }
}

/// Parses the `value` of a status assertion: a single integer or a sequence of
/// integers, e.g. `200` or `[200, 201]`.
fn parse_status_codes(value: &YamlValue) -> Vec<u16> {
  match value {
    YamlValue::Number(_) => vec![integer(value, "Status value must be a positive integer") as u16],
    YamlValue::Sequence(seq) => seq.iter().map(|v| integer(v, "Status codes must be positive integers") as u16).collect(),
    other => panic!("Status value must be an integer or a list of integers, got {:?}", other),
  }
}

fn integer(value: &YamlValue, message: &str) -> u64 {
  value.as_u64().unwrap_or_else(|| panic!("{message}, got {:?}", value))
}

/// Compares a JSONPath match with the expected value. Type differences are
/// tolerated by falling back to string representation comparison, so `123`
/// (number) matches the literal `"123"` (string).
fn value_eq(found: &Value, expected: &Value) -> bool {
  if found == expected {
    return true;
  }

  as_plain_string(found) == as_plain_string(expected)
}

fn as_plain_string(value: &Value) -> String {
  match value {
    Value::String(s) => s.clone(),
    other => other.to_string(),
  }
}

/// Parses `match_count`: a plain integer (`3`) or a mapping (`{gte: 2}`).
fn parse_match_count(value: Option<&YamlValue>) -> (Option<u64>, String) {
  let Some(value) = value else {
    return (None, String::new());
  };
  match value {
    YamlValue::Number(_) => (Some(integer(value, "match_count value must be a positive integer")), "eq".to_string()),
    YamlValue::Mapping(mapping) => {
      let Some((op, count)) = mapping.iter().next() else {
        panic!("match_count mapping must not be empty");
      };
      let Some(op) = op.as_str() else {
        panic!("match_count operator must be a string")
      };
      const SUPPORTED: &[&str] = &["eq", "neq", "gt", "gte", "lt", "lte"];
      if !SUPPORTED.contains(&op) {
        panic!("Unknown match_count operator '{op}'. Supported operators: {}", SUPPORTED.join(", "));
      }
      (Some(integer(count, "match_count value must be a positive integer")), op.to_string())
    }
    other => panic!("match_count must be an integer or a mapping like {{gte: 2}}, got {:?}", other),
  }
}

fn number_ok(actual: f64, operator: &str, expected: f64) -> bool {
  match operator {
    "gt" => actual > expected,
    "gte" => actual >= expected,
    "lt" => actual < expected,
    "lte" => actual <= expected,
    "eq" => actual == expected,
    "neq" => actual != expected,
    other => panic!("Unknown numeric operator '{other}'. Supported operators: gt, gte, lt, lte, eq, neq"),
  }
}

fn contained_number(value: &Value) -> Option<f64> {
  match value {
    Value::Number(n) => n.as_f64(),
    Value::String(s) => s.parse().ok(),
    _ => None,
  }
}

// Numeric comparison: both the match and the expected value must be numbers
// (a numeric string also counts).
fn number_ok_value(actual: &Value, operator: &str, expected: &Value) -> bool {
  let Some(actual) = contained_number(actual) else {
    return false;
  };
  let Some(expected) = contained_number(expected) else {
    return false;
  };
  number_ok(actual, operator, expected)
}

fn contains_value(found: &Value, expected: &Value) -> bool {
  match found {
    Value::String(s) => s.contains(&as_plain_string(expected)),
    Value::Array(arr) => arr.iter().any(|e| e == expected),
    _ => found == expected,
  }
}

#[async_trait]
impl Runnable for Assert {
  fn weight(&self) -> u32 {
    self.weight
  }

  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if !config.quiet {
      crate::emit(config.stats_json, format_args!("{:width$} {}", self.name.green(), self.describe(), width = 25));
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match self.assert_type {
      AssertType::Equals => self.execute_equals(context),
      AssertType::Status => self.execute_status(context),
      AssertType::Header => self.execute_header(context),
      AssertType::JsonPath => self.execute_jsonpath(context),
      AssertType::Duration => self.execute_duration(context),
    }));

    if let Err(e) = result {
      let msg = if let Some(s) = e.downcast_ref::<&str>() {
        s.to_string()
      } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
      } else {
        "Assertion failed".to_string()
      };
      if config.continue_on_assert_fail {
        config.assertion_failures.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if !config.quiet {
          eprintln!("  {} {} {}", "Assertion failed:".red().bold(), msg.yellow(), "(continuing)".purple());
        }
      } else {
        panic!("{}", msg);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::Map;
  use std::collections::HashMap;
  use std::sync::atomic::AtomicUsize;
  use std::sync::{Arc, Mutex};

  fn empty_config() -> Config {
    Config {
      base: String::new(),
      concurrency: 1,
      iterations: 1,
      relaxed_interpolations: false,
      no_check_certificate: false,
      rampup: 0,
      quiet: true,
      nanosec: false,
      timeout: 10,
      verbose: false,
      results: None,
      lifecycle: Default::default(),
      load_shape: None,
      arrival_rate: None,
      vars: HashMap::new(),
      threads: 1,
      conn_per_iter: false,
      persist_context: false,
      run_time: 0,
      continue_on_assert_fail: false,
      success_codes: Vec::new(),
      stats_json: false,
      assertion_failures: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn empty_pool() -> Pool {
    Arc::new(Mutex::new(HashMap::new()))
  }

  fn assert_yaml(text: &str) -> YamlValue {
    serde_yaml::from_str(text).unwrap()
  }

  /// Builds a context holding a `_last_response` snapshot, mirroring the shape
  /// written by `Request::execute` (including the `duration` field).
  fn last_response(status: u16, body: &str, headers: Map<String, Value>, duration_ms: f64) -> Context {
    let mut context = Context::new();
    context.insert(
      LAST_RESPONSE_KEY.to_string(),
      json!({
        "status": status,
        "body": body,
        "headers": headers,
        "url": "http://example.com/",
        "duration": duration_ms,
      }),
    );
    context
  }

  async fn run(assert: &Assert, context: &mut Context) {
    let mut reports = Vec::new();
    assert.execute(context, &mut reports, &empty_pool(), &empty_config()).await;
  }

  #[test]
  fn new_defaults_to_equals() {
    let assert = Assert::new(&assert_yaml("---\nname: Check bar\nassert:\n  key: bar\n  value: '2'"), None);

    assert_eq!(assert.assert_type, AssertType::Equals);
    assert_eq!(assert.key, "bar");
    assert_eq!(assert.value, "2");
  }

  #[test]
  fn new_parses_status_single_value() {
    let assert = Assert::new(&assert_yaml("---\nname: Check status\nassert:\n  type: status\n  value: 200"), None);

    assert_eq!(assert.assert_type, AssertType::Status);
    assert_eq!(assert.status_codes, vec![200]);
  }

  #[test]
  fn new_parses_status_array_of_values() {
    let assert = Assert::new(&assert_yaml("---\nname: Check status\nassert:\n  type: status\n  value: [200, 201]"), None);

    assert_eq!(assert.assert_type, AssertType::Status);
    assert_eq!(assert.status_codes, vec![200, 201]);
  }

  #[test]
  fn new_parses_header() {
    let assert = Assert::new(&assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: content-type\n  value: application/json"), None);

    assert_eq!(assert.assert_type, AssertType::Header);
    assert_eq!(assert.key, "content-type");
    assert_eq!(assert.value, "application/json");
  }

  #[test]
  fn new_parses_jsonpath() {
    let assert = Assert::new(&assert_yaml("---\nname: Check token\nassert:\n  type: jsonpath\n  key: '$.token'\n  value: abc123"), None);

    assert_eq!(assert.assert_type, AssertType::JsonPath);
    assert_eq!(assert.key, "$.token");
    assert_eq!(assert.json_value, Some(json!("abc123")));
    assert_eq!(assert.operator, "eq");
  }

  #[test]
  fn new_parses_duration_with_operator() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 500\n  operator: lt"), None);

    assert_eq!(assert.assert_type, AssertType::Duration);
    assert_eq!(assert.duration_max, 500);
    assert_eq!(assert.operator, "lt");
  }

  #[test]
  fn new_duration_defaults_operator_to_lt() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 500"), None);

    assert_eq!(assert.operator, "lt");
  }

  #[tokio::test]
  async fn equals_still_works() {
    let assert = Assert::new(&assert_yaml("---\nname: Check bar\nassert:\n  key: bar\n  value: '2'"), None);
    let mut context = Context::new();
    context.insert("bar".to_string(), json!("2"));

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Assertion mismatched")]
  async fn equals_fails_on_mismatch() {
    let assert = Assert::new(&assert_yaml("---\nname: Check bar\nassert:\n  key: bar\n  value: '3'"), None);
    let mut context = Context::new();
    context.insert("bar".to_string(), json!("2"));

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn status_single_value_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check status\nassert:\n  type: status\n  value: 200"), None);
    let mut context = last_response(200, "", Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn status_array_of_values_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check status\nassert:\n  type: status\n  value: [200, 201]"), None);
    let mut context = last_response(201, "", Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Status assertion mismatched")]
  async fn status_single_value_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check status\nassert:\n  type: status\n  value: 200"), None);
    let mut context = last_response(404, "", Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Status assertion mismatched")]
  async fn status_array_without_match_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check status\nassert:\n  type: status\n  value: [200, 201]"), None);
    let mut context = last_response(500, "", Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn header_substring_match_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: content-type\n  value: application/json"), None);
    let mut headers = Map::new();
    headers.insert("content-type".to_string(), json!("application/json; charset=utf-8"));
    let mut context = last_response(200, "", headers, 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn header_match_is_case_insensitive() {
    let assert = Assert::new(&assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: CONTENT-TYPE\n  value: Application/JSON"), None);
    let mut headers = Map::new();
    headers.insert("content-type".to_string(), json!("application/json"));
    let mut context = last_response(200, "", headers, 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Header assertion mismatched")]
  async fn header_missing_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: x-missing\n  value: anything"), None);
    let mut context = last_response(200, "", Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Header assertion mismatched")]
  async fn header_value_not_contained_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: content-type\n  value: text/html"), None);
    let mut headers = Map::new();
    headers.insert("content-type".to_string(), json!("application/json"));
    let mut context = last_response(200, "", headers, 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_match_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check id\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  value: '123'"), None);
    let mut context = last_response(200, r#"{"data": {"id": 123}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_array_result_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check role\nassert:\n  type: jsonpath\n  key: '$.roles[1]'\n  value: editor"), None);
    let mut context = last_response(200, r#"{"roles": ["admin", "editor"]}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_no_match_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check id\nassert:\n  type: jsonpath\n  key: '$.missing'\n  value: '123'"), None);
    let mut context = last_response(200, r#"{"data": {"id": 123}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_value_mismatch_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check id\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  value: '456'"), None);
    let mut context = last_response(200, r#"{"data": {"id": 123}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn duration_lt_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 500\n  operator: lt"), None);
    let mut context = last_response(200, "", Map::new(), 400.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn duration_lte_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 400\n  operator: lte"), None);
    let mut context = last_response(200, "", Map::new(), 400.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn duration_gt_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 500\n  operator: gt"), None);
    let mut context = last_response(200, "", Map::new(), 600.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn duration_gte_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 500\n  operator: gte"), None);
    let mut context = last_response(200, "", Map::new(), 500.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn duration_eq_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 500\n  operator: eq"), None);
    let mut context = last_response(200, "", Map::new(), 500.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Duration assertion mismatched")]
  async fn duration_lt_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check latency\nassert:\n  type: duration\n  value: 500\n  operator: lt"), None);
    let mut context = last_response(200, "", Map::new(), 600.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Assert needs a previous request")]
  async fn missing_last_response_panics() {
    let assert = Assert::new(&assert_yaml("---\nname: Check status\nassert:\n  type: status\n  value: 200"), None);
    let mut context = Context::new();

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_typed_number_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check id type\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  value: 123"), None);
    let mut context = last_response(200, r#"{"data": {"id": 123}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_typed_number_strict_vs_string_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check id type\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  value: 123"), None);
    let mut context = last_response(200, r#"{"data": {"id": "123"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_typed_bool_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check active\nassert:\n  type: jsonpath\n  key: '$.data.active'\n  value: true"), None);
    let mut context = last_response(200, r#"{"data": {"active": true}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_typed_bool_fails_on_truthy_string() {
    let assert = Assert::new(&assert_yaml("---\nname: Check active\nassert:\n  type: jsonpath\n  key: '$.data.active'\n  value: true"), None);
    let mut context = last_response(200, r#"{"data": {"active": "true"}}"#, Map::new(), 10.0);

    // Strict structural: string "true" is not boolean true.
    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_null_mismatch_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check null\nassert:\n  type: jsonpath\n  key: '$.data.missing'\n  value: null"), None);
    let mut context = last_response(200, r#"{"data": {"id": 1}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_nested_object_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check user\nassert:\n  type: jsonpath\n  key: '$.user'\n  value: {id: 1, role: admin}"), None);
    let mut context = last_response(200, r#"{"user": {"id": 1, "role": "admin"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_full_document_equality_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check whole body\nassert:\n  type: jsonpath\n  value: {ok: true}"), None);
    let mut context = last_response(200, r#"{"ok": true}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_full_document_equality_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check whole body\nassert:\n  type: jsonpath\n  value: {ok: true}"), None);
    let mut context = last_response(200, r#"{"ok": false}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_neq_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check role\nassert:\n  type: jsonpath\n  key: '$.data.role'\n  operator: neq\n  value: admin"), None);
    let mut context = last_response(200, r#"{"data": {"role": "editor"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_neq_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check role\nassert:\n  type: jsonpath\n  key: '$.data.role'\n  operator: neq\n  value: admin"), None);
    let mut context = last_response(200, r#"{"data": {"role": "admin"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_gt_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: gt\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 11}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_gt_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: gt\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 9}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_gte_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: gte\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 10}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_gte_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Compare count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: gte\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 9}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_lt_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Compare count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: lt\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 9}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_lt_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Compare count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: lt\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 10}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_lte_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Compare count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: lte\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 10}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_lte_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Compare count\nassert:\n  type: jsonpath\n  key: '$.data.count'\n  operator: lte\n  value: 10"), None);
    let mut context = last_response(200, r#"{"data": {"count": 11}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_contains_substring_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check name\nassert:\n  type: jsonpath\n  key: '$.data.name'\n  operator: contains\n  value: Jo"), None);
    let mut context = last_response(200, r#"{"data": {"name": "John"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_contains_substring_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check name\nassert:\n  type: jsonpath\n  key: '$.data.name'\n  operator: contains\n  value: Zo"), None);
    let mut context = last_response(200, r#"{"data": {"name": "John"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_contains_array_element_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check roles\nassert:\n  type: jsonpath\n  key: '$.data.roles'\n  operator: contains\n  value: admin"), None);
    let mut context = last_response(200, r#"{"data": {"roles": ["admin", "editor"]}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_contains_array_element_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check roles\nassert:\n  type: jsonpath\n  key: '$.data.roles'\n  operator: contains\n  value: owner"), None);
    let mut context = last_response(200, r#"{"data": {"roles": ["admin", "editor"]}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_in_fails_when_not_member() {
    let assert = Assert::new(&assert_yaml("---\nname: Check role\nassert:\n  type: jsonpath\n  key: '$.data.role'\n  operator: in\n  value: [admin, editor]"), None);
    let mut context = last_response(200, r#"{"data": {"role": "owner"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_in_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check role\nassert:\n  type: jsonpath\n  key: '$.data.role'\n  operator: in\n  value: [admin, editor]"), None);
    let mut context = last_response(200, r#"{"data": {"role": "editor"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_exists_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check exists\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  operator: exists"), None);
    let mut context = last_response(200, r#"{"data": {"id": 1}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_exists_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check exists\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  operator: exists"), None);
    let mut context = last_response(200, r#"{"data": {}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_not_exists_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check not exists\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  operator: not_exists"), None);
    let mut context = last_response(200, r#"{"data": {}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_not_exists_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check not exists\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  operator: not_exists"), None);
    let mut context = last_response(200, r#"{"data": {"id": 1}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_is_null_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check null\nassert:\n  type: jsonpath\n  key: '$.data.deleted'\n  operator: is_null"), None);
    let mut context = last_response(200, r#"{"data": {"deleted": null}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_is_null_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check null\nassert:\n  type: jsonpath\n  key: '$.data.deleted'\n  operator: is_null"), None);
    let mut context = last_response(200, r#"{"data": {"deleted": 0}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_operator_regex_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check email\nassert:\n  type: jsonpath\n  key: '$.data.email'\n  operator: regex\n  value: '^[a-z]+@example\\.com$'"), None);
    let mut context = last_response(200, r#"{"data": {"email": "john@example.com"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_operator_regex_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check email\nassert:\n  type: jsonpath\n  key: '$.data.email'\n  operator: regex\n  value: '^[a-z]+@example\\.com$'"), None);
    let mut context = last_response(200, r#"{"data": {"email": "john@bad.com"}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Unknown JsonPath operator")]
  async fn jsonpath_unknown_operator_panics() {
    Assert::new(&assert_yaml("---\nname: Check\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  operator: matches\n  value: 1"), None);
  }

  #[tokio::test]
  async fn jsonpath_every_all_match_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check all statuses\nassert:\n  type: jsonpath\n  key: '$.items[*].status'\n  every: true\n  value: ok"), None);
    let mut context = last_response(200, r#"{"items": [{"status": "ok"}, {"status": "ok"}]}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "JsonPath assertion mismatched")]
  async fn jsonpath_every_any_mismatch_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check all statuses\nassert:\n  type: jsonpath\n  key: '$.items[*].status'\n  every: true\n  operator: eq\n  value: ok"), None);
    let mut context = last_response(200, r#"{"items": [{"status": "ok"}, {"status": "failed"}]}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_every_false_checks_first_match_only() {
    let assert = Assert::new(&assert_yaml("---\nname: Check first status\nassert:\n  type: jsonpath\n  key: '$.items[*].status'\n  every: false\n  operator: eq\n  value: ok"), None);
    let mut context = last_response(200, r#"{"items": [{"status": "ok"}, {"status": "failed"}]}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_match_count_exact_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check items count\nassert:\n  type: jsonpath\n  key: '$.items[*]'\n  match_count: 3\n  operator: exists"), None);
    let mut context = last_response(200, r#"{"items": [1, 2, 3]}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "match_count")]
  async fn jsonpath_match_count_exact_fails() {
    let assert = Assert::new(&assert_yaml("---\nname: Check items count\nassert:\n  type: jsonpath\n  key: '$.items[*]'\n  match_count: 2\n  operator: exists"), None);
    let mut context = last_response(200, r#"{"items": [1, 2, 3]}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_match_count_mapping_gte_passes() {
    let assert = Assert::new(&assert_yaml("---\nname: Check items count\nassert:\n  type: jsonpath\n  key: '$.items[*]'\n  match_count: {gte: 2}\n  operator: exists"), None);
    let mut context = last_response(200, r#"{"items": [1, 2, 3]}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn jsonpath_loose_string_eq_still_passes() {
    // Historical behavior: a YAML string value compares loosely, so the
    // number 123 in the body matches the literal "123".
    let assert = Assert::new(&assert_yaml("---\nname: Check id\nassert:\n  type: jsonpath\n  key: '$.data.id'\n  value: '123'"), None);
    let mut context = last_response(200, r#"{"data": {"id": 123}}"#, Map::new(), 10.0);

    run(&assert, &mut context).await;
  }
}
