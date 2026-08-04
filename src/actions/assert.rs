use async_trait::async_trait;
use colored::*;
use serde_json::{Value, json};
use serde_yaml::Value as YamlValue;

use crate::actions::save::LAST_RESPONSE_KEY;
use crate::actions::{Runnable, extract};
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
  value: String,
  assert_type: AssertType,
  status_codes: Vec<u16>,
  duration_max: u64,
  operator: String,
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

    let (key, value, status_codes, duration_max, operator) = match assert_type {
      AssertType::Equals | AssertType::Header | AssertType::JsonPath => {
        let key = extract(assert_val, "key");
        let value = extract(assert_val, "value");
        (key, value, Vec::new(), 0, String::new())
      }
      AssertType::Status => {
        let value = assert_val.get("value").expect("assert value is required");
        (String::new(), String::new(), parse_status_codes(value), 0, String::new())
      }
      AssertType::Duration => {
        let value = assert_val.get("value").expect("assert value is required");
        let duration_max = value.as_u64().unwrap_or_else(|| panic!("Duration value must be a positive integer (ms), got {:?}", value));
        let operator = assert_val.get("operator").and_then(|v| v.as_str()).unwrap_or("lt").to_string();
        (String::new(), String::new(), Vec::new(), duration_max, operator)
      }
    };

    Assert {
      name,
      key,
      value,
      assert_type,
      status_codes,
      duration_max,
      operator,
      weight,
    }
  }

  /// Human-readable description of the assertion, used in non-quiet output.
  fn describe(&self) -> String {
    match self.assert_type {
      AssertType::Equals => format!("{}={}?", self.key.cyan().bold(), self.value.magenta()),
      AssertType::Status => format!("status in {:?}", self.status_codes),
      AssertType::Header => format!("header[{}] contains '{}'", self.key.cyan().bold(), self.value.magenta()),
      AssertType::JsonPath => format!("jsonpath[{}] == '{}'", self.key.cyan().bold(), self.value.magenta()),
      AssertType::Duration => format!("duration {} {}", self.operator, self.duration_max),
    }
  }

  fn last_response(&self, context: &Context) -> Value {
    context.get(LAST_RESPONSE_KEY).cloned().unwrap_or_else(|| {
      panic!(
        "Assert needs a previous request: no '{}' entry in the context.",
        LAST_RESPONSE_KEY
      )
    })
  }

  fn execute_equals(&self, context: &Context) {
    let interpolator = interpolator::Interpolator::new(context);
    let eval = format!("{{{{ {} }}}}", &self.key);
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

    let header_value = headers
      .get(self.key.to_lowercase())
      .or_else(|| {
        headers
          .as_object()?
          .iter()
          .find(|(name, _)| name.eq_ignore_ascii_case(&self.key))
          .map(|(_, value)| value)
      })
      .and_then(|v| v.as_str());

    match header_value {
      Some(value) => {
        if !value.to_lowercase().contains(&self.value.to_lowercase()) {
          panic!(
            "Header assertion mismatched: header '{}' value '{value}' does not contain '{}'",
            self.key, self.value
          );
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

    let body_json: Value = serde_json::from_str(body)
      .unwrap_or_else(|e| panic!("JsonPath assertion failed: response body is not valid JSON: {e}"));

    let matches = jsonpath_lib::select(&body_json, &self.key)
      .unwrap_or_else(|e| panic!("JsonPath assertion failed: invalid path '{}': {e}", self.key));

    let Some(found) = matches.first() else {
      panic!("JsonPath assertion mismatched: no match for path '{}'", self.key);
    };

    let assertion = json!(self.value.to_owned());
    if !value_eq(found, &assertion) {
      panic!("JsonPath assertion mismatched: {found} != {assertion} (path '{}')", self.key);
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
    YamlValue::Sequence(seq) => seq
      .iter()
      .map(|v| integer(v, "Status codes must be positive integers") as u16)
      .collect(),
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

  fn as_plain_string(value: &Value) -> String {
    match value {
      Value::String(s) => s.clone(),
      other => other.to_string(),
    }
  }

  as_plain_string(found) == as_plain_string(expected)
}

#[async_trait]
impl Runnable for Assert {
  fn weight(&self) -> u32 {
    self.weight
  }

  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if !config.quiet {
      println!("{:width$} {}", self.name.green(), self.describe(), width = 25);
    }

    match self.assert_type {
      AssertType::Equals => self.execute_equals(context),
      AssertType::Status => self.execute_status(context),
      AssertType::Header => self.execute_header(context),
      AssertType::JsonPath => self.execute_jsonpath(context),
      AssertType::Duration => self.execute_duration(context),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::Map;
  use std::collections::HashMap;
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
    let assert = Assert::new(
      &assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: content-type\n  value: application/json"),
      None,
    );

    assert_eq!(assert.assert_type, AssertType::Header);
    assert_eq!(assert.key, "content-type");
    assert_eq!(assert.value, "application/json");
  }

  #[test]
  fn new_parses_jsonpath() {
    let assert = Assert::new(&assert_yaml("---\nname: Check token\nassert:\n  type: jsonpath\n  key: '$.token'\n  value: abc123"), None);

    assert_eq!(assert.assert_type, AssertType::JsonPath);
    assert_eq!(assert.key, "$.token");
    assert_eq!(assert.value, "abc123");
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
    let assert = Assert::new(
      &assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: content-type\n  value: application/json"),
      None,
    );
    let mut headers = Map::new();
    headers.insert("content-type".to_string(), json!("application/json; charset=utf-8"));
    let mut context = last_response(200, "", headers, 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  async fn header_match_is_case_insensitive() {
    let assert = Assert::new(
      &assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: CONTENT-TYPE\n  value: Application/JSON"),
      None,
    );
    let mut headers = Map::new();
    headers.insert("content-type".to_string(), json!("application/json"));
    let mut context = last_response(200, "", headers, 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Header assertion mismatched")]
  async fn header_missing_fails() {
    let assert = Assert::new(
      &assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: x-missing\n  value: anything"),
      None,
    );
    let mut context = last_response(200, "", Map::new(), 10.0);

    run(&assert, &mut context).await;
  }

  #[tokio::test]
  #[should_panic(expected = "Header assertion mismatched")]
  async fn header_value_not_contained_fails() {
    let assert = Assert::new(
      &assert_yaml("---\nname: Check header\nassert:\n  type: header\n  key: content-type\n  value: text/html"),
      None,
    );
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
}
