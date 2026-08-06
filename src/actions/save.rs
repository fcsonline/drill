use async_trait::async_trait;
use colored::*;
use serde_json::Value;
use serde_yaml::Value as YamlValue;

use crate::actions::{Runnable, extract, extract_optional};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;

/// Reserved context key under which `Request::execute` stores a snapshot of the
/// last HTTP response, so later plan steps (e.g. `save`) can extract values
/// from it.
pub const LAST_RESPONSE_KEY: &str = "_last_response";

const SOURCE_BODY: &str = "response_body";
const SOURCE_HEADERS: &str = "response_headers";
const SOURCE_STATUS: &str = "response_status";
const SOURCE_URL: &str = "response_url";

/// Extracts a value from the last HTTP response and stores it in the context.
///
/// ```yaml
/// - name: Save auth token
///   save:
///     source: response_body     # response_body | response_headers | response_status | response_url
///     jsonpath: "$.token"       # optional; when omitted the whole source is stored
///     key: auth_token           # context key to store the result under
/// ```
///
/// For `response_headers`, `key` is the header name to look up and the header
/// value is stored under the same name (matching is case-insensitive).
#[derive(Clone)]
pub struct Save {
  name: String,
  source: String,
  jsonpath: Option<String>,
  key: String,
  weight: u32,
}

impl Save {
  pub fn is_that_you(item: &YamlValue) -> bool {
    item.get("save").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &YamlValue, _with_item: Option<YamlValue>) -> Save {
    let name = extract_optional(item, "name").unwrap_or_else(|| "Save".to_string());
    let save = item.get("save").expect("save field is required");
    let source = extract_optional(save, "source").unwrap_or_else(|| SOURCE_BODY.to_string());
    let jsonpath = extract_optional(save, "jsonpath");
    let key = extract(save, "key");
    let weight = item.get("weight").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(1);

    Save {
      name,
      source,
      jsonpath,
      key,
      weight,
    }
  }

  /// Parses the stored `body` field (a string) into a JSON value. When the
  /// body is not valid JSON, the raw text is kept as a string value.
  fn body_as_json(last_response: &Value) -> Option<Value> {
    match last_response.get("body")? {
      Value::String(body) => Some(serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.clone()))),
      value => Some(value.clone()),
    }
  }

  fn save_body(&self, last_response: &Value) -> Option<Value> {
    let body = Self::body_as_json(last_response)?;

    match &self.jsonpath {
      Some(path) => jsonpath_lib::select(&body, path).ok().and_then(|mut matches| matches.drain(..).next()).cloned(),
      None => Some(body),
    }
  }

  /// Looks up a response header by name. The stored header map uses lowercase
  /// keys, so the lookup falls back to a case-insensitive scan to be forgiving
  /// of the casing the user writes in the plan.
  fn save_header(&self, last_response: &Value) -> Option<Value> {
    let headers = last_response.get("headers")?;

    headers.get(&self.key).or_else(|| headers.as_object()?.iter().find(|(name, _)| name.eq_ignore_ascii_case(&self.key)).map(|(_, value)| value)).cloned()
  }

  fn save_status(&self, last_response: &Value) -> Option<Value> {
    last_response.get("status").cloned()
  }

  fn save_url(&self, last_response: &Value) -> Option<Value> {
    last_response.get("url").cloned()
  }
}

#[async_trait]
impl Runnable for Save {
  fn weight(&self) -> u32 {
    self.weight
  }

  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if !config.quiet {
      println!("{:width$} {}<-{}", self.name.green(), self.key.cyan().bold(), self.source.magenta(), width = 25);
    }

    let Some(last_response) = context.get(LAST_RESPONSE_KEY).cloned() else {
      eprintln!("{} 'save' needs a previous request: no '{}' entry in the context. Add an 'assign' request before it.", "WARNING!".yellow().bold(), LAST_RESPONSE_KEY);
      return;
    };

    let value = match self.source.as_str() {
      SOURCE_BODY => self.save_body(&last_response),
      SOURCE_HEADERS => self.save_header(&last_response),
      SOURCE_STATUS => self.save_status(&last_response),
      SOURCE_URL => self.save_url(&last_response),
      other => {
        eprintln!("{} Unknown save source '{}'.", "WARNING!".yellow().bold(), other);
        return;
      }
    };

    match value {
      Some(value) => {
        context.insert(self.key.clone(), value);
      }
      None => {
        eprintln!("{} 'save' could not extract '{}' from the last response.", "WARNING!".yellow().bold(), self.key);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::{Map, json};
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
      vars: HashMap::new(),
      threads: 1,
      conn_per_iter: false,
    }
  }

  fn empty_pool() -> Pool {
    Arc::new(Mutex::new(HashMap::new()))
  }

  fn save_yaml(text: &str) -> YamlValue {
    let docs = crate::reader::read_file_as_yml_from_str(text);
    docs[0].clone()
  }

  fn last_response_with(body: &str, headers: Map<String, Value>, status: u16, url: &str) -> Context {
    let mut context = Context::new();
    context.insert(
      LAST_RESPONSE_KEY.to_string(),
      json!({
        "status": status,
        "body": body,
        "headers": headers,
        "url": url,
      }),
    );
    context
  }

  #[test]
  fn is_that_you_detects_save() {
    let text = "---\nname: Save token\nsave:\n  source: response_body\n  jsonpath: '$.token'\n  key: auth_token";
    let doc = &save_yaml(text);

    assert!(Save::is_that_you(doc));
  }

  #[test]
  fn new_parses_fields_and_defaults() {
    let text = "---\nname: Save token\nsave:\n  jsonpath: '$.token'\n  key: auth_token";
    let save = Save::new(&save_yaml(text), None);

    assert_eq!(save.name, "Save token");
    assert_eq!(save.source, SOURCE_BODY);
    assert_eq!(save.jsonpath.as_deref(), Some("$.token"));
    assert_eq!(save.key, "auth_token");
    assert_eq!(save.weight, 1);
  }

  #[test]
  fn new_parses_weight() {
    let text = "---\nname: Save token\nsave:\n  source: response_status\n  key: code\nweight: 4";
    let save = Save::new(&save_yaml(text), None);

    assert_eq!(save.weight, 4);
  }

  #[tokio::test]
  async fn extracts_value_from_body_with_jsonpath() {
    let text = "---\nname: Save token\nsave:\n  source: response_body\n  jsonpath: '$.token'\n  key: auth_token";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with(r#"{"token": "abc123"}"#, Map::new(), 200, "http://example.com/login");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("auth_token"), Some(&json!("abc123")));
  }

  #[tokio::test]
  async fn missing_jsonpath_stores_entire_body() {
    let text = "---\nname: Save body\nsave:\n  source: response_body\n  key: payload";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with(r#"{"user": "x", "roles": ["admin"]}"#, Map::new(), 200, "http://example.com/me");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("payload"), Some(&json!({"user": "x", "roles": ["admin"]})));
  }

  #[tokio::test]
  async fn missing_jsonpath_keeps_non_json_body_as_string() {
    let text = "---\nname: Save body\nsave:\n  source: response_body\n  key: raw";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with("plain text body", Map::new(), 200, "http://example.com/raw");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("raw"), Some(&json!("plain text body")));
  }

  #[tokio::test]
  async fn extracts_value_from_body_with_array_index() {
    let text = "---\nname: Save second role\nsave:\n  source: response_body\n  jsonpath: '$.roles[1]'\n  key: second_role";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with(r#"{"roles": ["admin", "editor"]}"#, Map::new(), 200, "http://example.com/me");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("second_role"), Some(&json!("editor")));
  }

  #[tokio::test]
  async fn extracts_header_value() {
    let text = "---\nname: Save rate limit\nsave:\n  source: response_headers\n  key: X-Rate-Limit";
    let save = Save::new(&save_yaml(text), None);
    let mut headers = Map::new();
    headers.insert("x-rate-limit".to_string(), json!("100"));
    let mut context = last_response_with("", headers, 200, "http://example.com/");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("X-Rate-Limit"), Some(&json!("100")));
  }

  #[tokio::test]
  async fn header_lookup_is_case_insensitive() {
    let text = "---\nname: Save auth header\nsave:\n  source: response_headers\n  key: authorization";
    let save = Save::new(&save_yaml(text), None);
    let mut headers = Map::new();
    headers.insert("Authorization".to_string(), json!("Bearer xyz"));
    let mut context = last_response_with("", headers, 200, "http://example.com/");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("authorization"), Some(&json!("Bearer xyz")));
  }

  #[tokio::test]
  async fn extracts_status_as_integer() {
    let text = "---\nname: Save status\nsave:\n  source: response_status\n  key: login_code";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with("", Map::new(), 201, "http://example.com/login");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("login_code"), Some(&json!(201)));
  }

  #[tokio::test]
  async fn extracts_url() {
    let text = "---\nname: Save url\nsave:\n  source: response_url\n  key: final_url";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with("", Map::new(), 302, "http://example.com/redirected");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert_eq!(context.get("final_url"), Some(&json!("http://example.com/redirected")));
  }

  #[tokio::test]
  async fn missing_last_response_is_a_no_op() {
    let text = "---\nname: Save token\nsave:\n  source: response_body\n  jsonpath: '$.token'\n  key: auth_token";
    let save = Save::new(&save_yaml(text), None);
    let mut context = Context::new();
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert!(context.get("auth_token").is_none());
  }

  #[tokio::test]
  async fn jsonpath_without_match_is_a_no_op() {
    let text = "---\nname: Save token\nsave:\n  source: response_body\n  jsonpath: '$.missing'\n  key: auth_token";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with(r#"{"token": "abc123"}"#, Map::new(), 200, "http://example.com/login");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert!(context.get("auth_token").is_none());
  }

  #[tokio::test]
  async fn unknown_source_is_a_no_op() {
    let text = "---\nname: Save token\nsave:\n  source: response_cookies\n  key: cookie";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with("", Map::new(), 200, "http://example.com/");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert!(context.get("cookie").is_none());
  }

  #[tokio::test]
  async fn missing_header_is_a_no_op() {
    let text = "---\nname: Save token\nsave:\n  source: response_headers\n  key: X-Missing";
    let save = Save::new(&save_yaml(text), None);
    let mut context = last_response_with("", Map::new(), 200, "http://example.com/");
    let mut reports = Vec::new();

    save.execute(&mut context, &mut reports, &empty_pool(), &empty_config()).await;

    assert!(context.get("X-Missing").is_none());
  }
}
