use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use colored::Colorize;
use encoding_rs::{Encoding, UTF_8};
use reqwest::{
  ClientBuilder, Method, StatusCode,
  header::{self, HeaderMap, HeaderName, HeaderValue},
};
use serde_yaml::Value as YamlValue;
use std::fmt::Write;
use std::fs::File;
use std::io::Read;
use url::Url;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::actions::{extract, extract_optional};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

use crate::actions::{Report, Runnable};

static USER_AGENT: &str = "drill";

#[derive(Clone)]
pub enum Body {
  Template(String),
  Binary(Vec<u8>),
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct Request {
  name: String,
  url: String,
  time: f64,
  method: String,
  headers: HashMap<String, String>,
  pub body: Option<Body>,
  pub with_item: Option<YamlValue>,
  pub index: Option<u32>,
  pub assign: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct AssignedRequest {
  status: u16,
  body: Value,
  headers: Map<String, Value>,
}

/// An owned snapshot of an HTTP response, captured before its body is consumed.
///
/// The latency timer now spans the full body download (time-to-last-byte), which
/// means the streaming `reqwest::Response` is consumed while the clock is still
/// running. Anything that borrows the response -- status, headers, cookies, and
/// the final URL -- is therefore cloned out into this struct *before* the body is
/// drained, so callers retain access to it afterwards.
struct ResponseData {
  /// Final request URL, used by verbose response logging.
  url: Url,
  /// HTTP status of the response.
  status: StatusCode,
  /// Response headers, surfaced to later plan steps through `assign`.
  headers: HeaderMap,
  /// `Set-Cookie` name/value pairs, materialized for the cookie jar.
  cookies: Vec<(String, String)>,
  /// Decoded response body, retained only when the request has an `assign`.
  body: Option<String>,
}

impl Request {
  pub fn is_that_you(item: &YamlValue) -> bool {
    item.get("request").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &YamlValue, with_item: Option<YamlValue>, index: Option<u32>) -> Request {
    let name = extract(item, "name");
    let request_val = item.get("request").expect("request field is required");
    let url = extract(request_val, "url");
    let assign = extract_optional(item, "assign");

    let method = if let Some(v) = extract_optional(request_val, "method") {
      v.to_uppercase()
    } else {
      "GET".to_string()
    };

    let body_verbs = ["POST", "PATCH", "PUT"];
    let body = if body_verbs.contains(&method.as_str()) {
      if let Some(body) = request_val.get("body").and_then(|v| v.as_str()) {
        Some(Body::Template(body.to_string()))
      } else if let Some(file_path) = request_val.get("body").and_then(|v| v.get("file")).and_then(|v| v.as_str()) {
        let mut file = File::open(file_path).expect("Unable to open file");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).expect("Unable to read file");
        Some(Body::Binary(buffer))
      } else if let Some(hex_str) = request_val.get("body").and_then(|v| v.get("hex")).and_then(|v| v.as_str()) {
        Some(Body::Binary(hex::decode(hex_str).expect("Invalid hex string")))
      } else {
        panic!("{} Body must be string, file or hex!!", "WARNING!".yellow().bold());
      }
    } else {
      None
    };

    let mut headers = HashMap::new();

    if let Some(mapping) = request_val.get("headers").and_then(|v| v.as_mapping()) {
      for (key, val) in mapping.iter() {
        if let Some(vs) = val.as_str() {
          if let Some(key_str) = key.as_str() {
            headers.insert(key_str.to_string(), vs.to_string());
          } else {
            panic!("{} Header keys must be strings!!", "WARNING!".yellow().bold());
          }
        } else {
          panic!("{} Headers must be strings!!", "WARNING!".yellow().bold());
        }
      }
    }

    Request {
      name,
      url,
      time: 0.0,
      method,
      headers,
      body,
      with_item,
      index,
      assign,
    }
  }

  fn format_time(tdiff: f64, nanosec: bool) -> String {
    if nanosec {
      (1_000_000.0 * tdiff).round().to_string() + "ns"
    } else {
      tdiff.round().to_string() + "ms"
    }
  }

  async fn send_request(&self, context: &mut Context, pool: &Pool, config: &Config) -> (Option<ResponseData>, f64) {
    let mut uninterpolator = None;

    // Resolve the name
    let interpolated_name = if self.name.contains('{') {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(&self.name, !config.relaxed_interpolations)
    } else {
      self.name.clone()
    };

    // Resolve the url
    let interpolated_url = if self.url.contains('{') {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(&self.url, !config.relaxed_interpolations)
    } else {
      self.url.clone()
    };

    // Resolve relative urls
    let interpolated_base_url = if &interpolated_url[..1] == "/" {
      match context.get("base") {
        Some(value) => {
          if let Some(vs) = value.as_str() {
            format!("{vs}{interpolated_url}")
          } else {
            panic!("{} Wrong type 'base' variable!", "WARNING!".yellow().bold());
          }
        }
        _ => {
          panic!("{} Unknown 'base' variable!", "WARNING!".yellow().bold());
        }
      }
    } else {
      interpolated_url
    };

    let url = Url::parse(&interpolated_base_url).expect("Invalid url!");
    let domain = format!("{}://{}:{}", url.scheme(), url.host_str().unwrap(), url.port().unwrap_or(0)); // Unique domain key for keep-alive

    let interpolated_body;

    // Method
    let method = match self.method.to_uppercase().as_ref() {
      "GET" => Method::GET,
      "POST" => Method::POST,
      "PUT" => Method::PUT,
      "PATCH" => Method::PATCH,
      "DELETE" => Method::DELETE,
      "HEAD" => Method::HEAD,
      _ => panic!("Unknown method '{}'", self.method),
    };

    // Resolve the body
    let (client, request) = {
      let mut pool2 = pool.lock().unwrap();
      let client = pool2.entry(domain).or_insert_with(|| ClientBuilder::default().danger_accept_invalid_certs(config.no_check_certificate).build().unwrap());

      let request = match self.body.as_ref() {
        Some(Body::Template(template_body)) => {
          interpolated_body = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(template_body, !config.relaxed_interpolations);
          client.request(method, interpolated_base_url.as_str()).body(interpolated_body)
        }
        Some(Body::Binary(binary_body)) => client.request(method, interpolated_base_url.as_str()).body(binary_body.clone()),
        None => client.request(method, interpolated_base_url.as_str()),
      };

      (client.clone(), request)
    };

    // Headers
    let mut headers = HeaderMap::new();
    headers.insert(header::USER_AGENT, HeaderValue::from_str(USER_AGENT).unwrap());

    if let Some(cookies) = context.get("cookies") {
      let cookies: Map<String, Value> = serde_json::from_value(cookies.clone()).unwrap();
      let cookie = cookies.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(";");

      headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
    }

    // Resolve headers
    for (key, val) in self.headers.iter() {
      let interpolated_header = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(val, !config.relaxed_interpolations);
      headers.insert(HeaderName::from_bytes(key.as_bytes()).unwrap(), HeaderValue::from_str(&interpolated_header).unwrap());
    }

    let request_builder = request.headers(headers).timeout(Duration::from_secs(config.timeout));
    let request = request_builder.build().expect("Cannot create request");

    if config.verbose {
      log_request(&request);
    }

    let begin = Instant::now();
    let response_result = client.execute(request).await;

    let mut response = match response_result {
      Err(e) => {
        let duration_ms = begin.elapsed().as_secs_f64() * 1000.0;
        if !config.quiet || config.verbose {
          println!("Error connecting '{}': {:?}", interpolated_base_url.as_str(), e);
        }
        return (None, duration_ms);
      }
      Ok(response) => response,
    };

    // Snapshot everything that borrows the response before the body stream is
    // consumed below.
    let url = response.url().clone();
    let status = response.status();
    let headers = response.headers().clone();
    let cookies: Vec<(String, String)> = response.cookies().map(|cookie| (cookie.name().to_string(), cookie.value().to_string())).collect();

    // Read the full response body so the measured latency reflects
    // time-to-last-byte, matching wrk, k6, vegeta and other load-testing tools.
    // The timer previously stopped at the response headers, which under-reported
    // any endpoint serving a non-trivial body (files, large JSON).
    //
    // The body is drained one chunk at a time. It is only buffered when an
    // `assign` needs to decode it; otherwise each chunk is dropped immediately,
    // so peak memory stays O(chunk) rather than O(body) even for large responses.
    let mut body_buf = self.assign.is_some().then(Vec::new);
    let drain_result = loop {
      match response.chunk().await {
        Ok(Some(chunk)) => {
          if let Some(buf) = body_buf.as_mut() {
            buf.extend_from_slice(&chunk);
          }
        }
        Ok(None) => break Ok(()),
        Err(e) => break Err(e),
      }
    };
    let duration_ms = begin.elapsed().as_secs_f64() * 1000.0;

    if let Err(e) = drain_result {
      if !config.quiet || config.verbose {
        println!("Error reading body '{}': {:?}", interpolated_base_url.as_str(), e);
      }
      return (None, duration_ms);
    }

    if !config.quiet {
      let status_text = if status.is_server_error() {
        status.to_string().red()
      } else if status.is_client_error() {
        status.to_string().purple()
      } else {
        status.to_string().yellow()
      };

      println!("{:width$} {} {} {}", interpolated_name.green(), interpolated_base_url.blue().bold(), status_text, Request::format_time(duration_ms, config.nanosec).cyan(), width = 25);
    }

    // Decode the buffered body (only present for `assign`) using the response
    // charset, mirroring reqwest's `Response::text`, so non-UTF-8 bodies are not
    // corrupted.
    let body = body_buf.map(|buf| decode_body(&headers, &buf));

    (
      Some(ResponseData {
        url,
        status,
        headers,
        cookies,
        body,
      }),
      duration_ms,
    )
  }
}

/// Decodes a response body using the charset declared in the `Content-Type`
/// header, defaulting to UTF-8 when none is present or the label is unknown.
///
/// This mirrors reqwest's `Response::text`, which drill can no longer call
/// directly: the body is drained as raw bytes inside the latency timer (so the
/// measured duration covers the full transfer), and only then decoded for the
/// `assign` path. Decoding the drained bytes here keeps charset-aware behaviour
/// for non-UTF-8 responses (e.g. `charset=iso-8859-1`).
fn decode_body(headers: &HeaderMap, bytes: &[u8]) -> String {
  let encoding = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(charset_from_content_type).and_then(|label| Encoding::for_label(label.as_bytes())).unwrap_or(UTF_8);

  encoding.decode(bytes).0.into_owned()
}

/// Extracts the `charset` parameter value from a `Content-Type` header value,
/// e.g. `text/html; charset=iso-8859-1` -> `iso-8859-1`. Surrounding quotes are
/// stripped. Returns `None` when no `charset` parameter is present.
fn charset_from_content_type(content_type: &str) -> Option<&str> {
  content_type.split(';').skip(1).find_map(|param| {
    let (key, value) = param.split_once('=')?;
    if key.trim().eq_ignore_ascii_case("charset") {
      Some(value.trim().trim_matches('"'))
    } else {
      None
    }
  })
}

fn yaml_to_json(data: YamlValue) -> Value {
  match data {
    YamlValue::Bool(b) => json!(b),
    YamlValue::Number(n) => {
      if let Some(i) = n.as_i64() {
        json!(i)
      } else if let Some(f) = n.as_f64() {
        json!(f)
      } else {
        // Fallback: convert to string representation
        json!(n.to_string())
      }
    }
    YamlValue::String(s) => json!(s),
    YamlValue::Mapping(m) => {
      let mut map = Map::new();
      for (key, value) in m.iter() {
        if let Some(key_str) = key.as_str() {
          map.insert(key_str.to_string(), yaml_to_json(value.clone()));
        }
      }
      json!(map)
    }
    YamlValue::Sequence(v) => {
      let mut array = Vec::new();
      for value in v.iter() {
        array.push(yaml_to_json(value.clone()));
      }
      json!(array)
    }
    YamlValue::Null => json!(null),
    _ => panic!("Unknown Yaml node"),
  }
}

#[async_trait]
impl Runnable for Request {
  async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &Pool, config: &Config) {
    if let Some(with_item) = &self.with_item {
      context.insert("item".to_string(), yaml_to_json(with_item.clone()));
    }

    if let Some(index) = self.index {
      context.insert("index".to_string(), json!(index));
    }

    let (res, duration_ms) = self.send_request(context, pool, config).await;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();

    let log_message_response = if config.verbose {
      Some(log_message_response(&res, duration_ms))
    } else {
      None
    };

    match res {
      None => reports.push(Report {
        name: self.name.to_owned(),
        duration: duration_ms,
        status: 520u16,
        timestamp,
      }),
      Some(response) => {
        let status = response.status.as_u16();

        reports.push(Report {
          name: self.name.to_owned(),
          duration: duration_ms,
          status,
          timestamp,
        });

        for (name, value) in &response.cookies {
          let cookies = context.entry("cookies").or_insert_with(|| json!({})).as_object_mut().unwrap();
          cookies.insert(name.clone(), json!(value));
        }

        let data = if let Some(ref key) = self.assign {
          let mut headers = Map::new();

          response.headers.iter().for_each(|(header, value)| {
            headers.insert(header.to_string(), json!(value.to_str().unwrap()));
          });

          let data = response.body.clone().unwrap_or_default();

          let body: Value = serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);

          let assigned = AssignedRequest {
            status,
            body,
            headers,
          };

          let value = serde_json::to_value(assigned).unwrap();

          context.insert(key.to_owned(), value);

          Some(data)
        } else {
          None
        };

        if let Some(msg) = log_message_response {
          log_response(msg, &data)
        }
      }
    }
  }
}

fn log_request(request: &reqwest::Request) {
  let mut message = String::new();
  write!(message, "{}", ">>>".bold().green()).unwrap();
  write!(message, " {} {},", "URL:".bold(), request.url()).unwrap();
  write!(message, " {} {},", "METHOD:".bold(), request.method()).unwrap();
  write!(message, " {} {:?}", "HEADERS:".bold(), request.headers()).unwrap();
  println!("{message}");
}

fn log_message_response(response: &Option<ResponseData>, duration_ms: f64) -> String {
  let mut message = String::new();
  match response {
    Some(response) => {
      write!(message, " {} {},", "URL:".bold(), response.url).unwrap();
      write!(message, " {} {},", "STATUS:".bold(), response.status).unwrap();
      write!(message, " {} {:?}", "HEADERS:".bold(), response.headers).unwrap();
      write!(message, " {} {:.4} ms,", "DURATION:".bold(), duration_ms).unwrap();
    }
    None => {
      message = String::from("No response from server!");
    }
  }
  message
}

fn log_response(log_message_response: String, body: &Option<String>) {
  let mut message = String::new();
  write!(message, "{}{}", "<<<".bold().green(), log_message_response).unwrap();
  if let Some(body) = body.as_ref() {
    write!(message, " {} {:?}", "BODY:".bold(), body).unwrap()
  }
  println!("{message}");
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_yaml::Value as YamlValue;
  use std::io::Write;
  use tempfile::NamedTempFile;

  fn create_yaml_request_with_string_body(body_content: &str) -> YamlValue {
    let yaml_str = format!(
      r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body: "{}"
"#,
      body_content
    );
    serde_yaml::from_str(&yaml_str).unwrap()
  }

  fn create_yaml_request_with_hex_body(hex_content: &str) -> YamlValue {
    let yaml_str = format!(
      r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    hex: "{}"
"#,
      hex_content
    );
    serde_yaml::from_str(&yaml_str).unwrap()
  }

  fn create_yaml_request_with_file_body(file_path: &str) -> YamlValue {
    let yaml_str = format!(
      r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    file: "{}"
"#,
      file_path
    );
    serde_yaml::from_str(&yaml_str).unwrap()
  }

  #[test]
  fn test_body_template_string() {
    let yaml = create_yaml_request_with_string_body("Hello, World!");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Template(content)) => {
        assert_eq!(content, "Hello, World!");
      }
      _ => panic!("Expected Body::Template"),
    }
  }

  #[test]
  fn test_body_hex() {
    // "Hello" in hex is "48656c6c6f"
    let yaml = create_yaml_request_with_hex_body("48656c6c6f");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_hex_empty() {
    let yaml = create_yaml_request_with_hex_body("");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"");
      }
      _ => panic!("Expected Body::Binary with empty data"),
    }
  }

  #[test]
  fn test_body_hex_complex() {
    // "Hello, World!" in hex
    let yaml = create_yaml_request_with_hex_body("48656c6c6f2c20576f726c6421");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello, World!");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_file() {
    // Create a temporary file with test content
    let mut temp_file = NamedTempFile::new().unwrap();
    let test_content = b"Test file content";
    temp_file.write_all(test_content).unwrap();
    temp_file.flush().unwrap();

    let file_path = temp_file.path().to_str().unwrap();
    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, test_content);
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_file_empty() {
    // Create an empty temporary file
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path().to_str().unwrap();

    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"");
      }
      _ => panic!("Expected Body::Binary with empty data"),
    }
  }

  #[test]
  fn test_body_file_binary_data() {
    // Create a file with binary data (not UTF-8)
    let mut temp_file = NamedTempFile::new().unwrap();
    let binary_content = vec![0x00, 0x01, 0x02, 0xFF, 0xFE, 0xFD];
    temp_file.write_all(&binary_content).unwrap();
    temp_file.flush().unwrap();

    let file_path = temp_file.path().to_str().unwrap();
    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, binary_content);
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_file_large_content() {
    // Create a file with larger content
    let mut temp_file = NamedTempFile::new().unwrap();
    let large_content: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    temp_file.write_all(&large_content).unwrap();
    temp_file.flush().unwrap();

    let file_path = temp_file.path().to_str().unwrap();
    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data.len(), 10000);
        assert_eq!(data, large_content);
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_none_for_get() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: GET
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    assert!(request.body.is_none());
  }

  #[test]
  fn test_body_none_for_delete() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: DELETE
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    assert!(request.body.is_none());
  }

  #[test]
  fn test_body_hex_uppercase() {
    // Test that hex decoding works with uppercase letters
    let yaml = create_yaml_request_with_hex_body("48656C6C6F");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_hex_mixed_case() {
    // Test that hex decoding works with mixed case
    let yaml = create_yaml_request_with_hex_body("48656c6C6F");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  #[should_panic(expected = "Invalid hex string")]
  fn test_body_hex_invalid() {
    let yaml = create_yaml_request_with_hex_body("InvalidHexString!");
    Request::new(&yaml, None, None);
  }

  #[test]
  #[should_panic(expected = "Unable to open file")]
  fn test_body_file_not_found() {
    let yaml = create_yaml_request_with_file_body("/nonexistent/path/to/file.txt");
    Request::new(&yaml, None, None);
  }

  #[test]
  fn test_body_priority_string_over_hex() {
    // When body is a string, it should be treated as Template, not hex
    let yaml = create_yaml_request_with_string_body("48656c6c6f");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Template(content)) => {
        assert_eq!(content, "48656c6c6f");
      }
      _ => panic!("Expected Body::Template when body is a string"),
    }
  }

  #[test]
  fn test_body_put_method() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: PUT
  body: "PUT body content"
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Template(content)) => {
        assert_eq!(content, "PUT body content");
      }
      _ => panic!("Expected Body::Template"),
    }
  }

  #[test]
  fn test_body_patch_method() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: PATCH
  body:
    hex: "5061746368"
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Patch");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  fn test_config() -> Config {
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
    }
  }

  #[test]
  fn charset_parsed_from_content_type() {
    assert_eq!(charset_from_content_type("text/html; charset=iso-8859-1"), Some("iso-8859-1"));
    assert_eq!(charset_from_content_type("text/plain;charset=\"UTF-16\""), Some("UTF-16"));
    assert_eq!(charset_from_content_type("application/json"), None);
    assert_eq!(charset_from_content_type("text/html; boundary=x"), None);
  }

  #[test]
  fn decode_body_honors_declared_charset() {
    // 0xE9 is 'e-acute' in ISO-8859-1 but invalid as standalone UTF-8.
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=iso-8859-1"));
    assert_eq!(decode_body(&headers, &[0xE9]), "\u{e9}");
  }

  #[test]
  fn decode_body_defaults_to_utf8() {
    let headers = HeaderMap::new();
    assert_eq!(decode_body(&headers, "hello \u{e9}".as_bytes()), "hello \u{e9}");
  }

  /// The latency timer must span the full body download (time-to-last-byte):
  /// a server that streams its body 300ms after the headers should measure at
  /// roughly 300ms, not ~0ms. Regression guard for the bug where the timer
  /// stopped at the response headers and the body was never read (#74).
  #[test]
  fn measures_full_body_transfer_time() {
    use std::collections::HashMap;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body_delay = Duration::from_millis(300);

    // A bare HTTP/1.1 server that sends the response head immediately but delays
    // the body, so time-to-headers and time-to-last-byte differ measurably.
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let mut buf = [0u8; 1024];
      let _ = stream.read(&mut buf).unwrap();

      let body = "x".repeat(4096);
      let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
      stream.write_all(head.as_bytes()).unwrap();
      stream.flush().unwrap();
      thread::sleep(body_delay);
      stream.write_all(body.as_bytes()).unwrap();
      stream.flush().unwrap();
    });

    let url = format!("http://{addr}/");
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: delayed-body\nrequest:\n  url: {url}\n")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context: Context = Context::new();
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let config = test_config();

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (response, duration_ms) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    server.join().unwrap();

    assert!(response.is_some(), "expected a successful response");
    assert!(duration_ms >= 250.0, "measured {duration_ms}ms; expected >= 250ms to include the 300ms body-transfer delay");
  }

  /// A request without `assign` must still fully drain a large body (so the
  /// timer covers the transfer and the connection can be reused), but must not
  /// retain it -- the chunks are dropped as they arrive rather than buffered.
  #[test]
  fn large_body_without_assign_is_drained_not_retained() {
    use std::collections::HashMap;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body_len = 1024 * 1024; // 1 MiB

    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let mut buf = [0u8; 1024];
      let _ = stream.read(&mut buf).unwrap();

      let body = "x".repeat(body_len);
      let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
      stream.write_all(head.as_bytes()).unwrap();
      stream.write_all(body.as_bytes()).unwrap();
      stream.flush().unwrap();
    });

    let url = format!("http://{addr}/");
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: large-body\nrequest:\n  url: {url}\n")).unwrap();
    let request = Request::new(&yaml, None, None); // no `assign`
    let mut context: Context = Context::new();
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let config = test_config();

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (response, _duration_ms) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    server.join().unwrap();

    let response = response.expect("expected a successful response after draining the body");
    assert_eq!(response.status.as_u16(), 200);
    assert!(response.body.is_none(), "a non-assign body must be drained and dropped, not retained");
  }
}
