use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use hex;
use reqwest::{
  header::{self, HeaderMap, HeaderName, HeaderValue},
  ClientBuilder, Method, Response,
};
use serde_yaml::Value as YamlValue;
use std::fmt::Write;
use std::fs::File;
use std::io::Read;
use url::Url;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

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

  async fn send_request(&self, context: &mut Context, pool: &Pool, config: &Config) -> (Option<Response>, f64) {
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
    let duration_ms = begin.elapsed().as_secs_f64() * 1000.0;

    match response_result {
      Err(e) => {
        if !config.quiet || config.verbose {
          println!("Error connecting '{}': {:?}", interpolated_base_url.as_str(), e);
        }
        (None, duration_ms)
      }
      Ok(response) => {
        if !config.quiet {
          let status = response.status();
          let status_text = if status.is_server_error() {
            status.to_string().red()
          } else if status.is_client_error() {
            status.to_string().purple()
          } else {
            status.to_string().yellow()
          };

          println!("{:width$} {} {} {}", interpolated_name.green(), interpolated_base_url.blue().bold(), status_text, Request::format_time(duration_ms, config.nanosec).cyan(), width = 25);
        }

        (Some(response), duration_ms)
      }
    }
  }
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
    if self.with_item.is_some() {
      context.insert("item".to_string(), yaml_to_json(self.with_item.clone().unwrap()));
    }

    if self.index.is_some() {
      context.insert("index".to_string(), json!(self.index.unwrap()));
    }

    let (res, duration_ms) = self.send_request(context, pool, config).await;

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
      }),
      Some(response) => {
        let status = response.status().as_u16();

        reports.push(Report {
          name: self.name.to_owned(),
          duration: duration_ms,
          status,
        });

        for cookie in response.cookies() {
          let cookies = context.entry("cookies").or_insert_with(|| json!({})).as_object_mut().unwrap();
          cookies.insert(cookie.name().to_string(), json!(cookie.value().to_string()));
        }

        let data = if let Some(ref key) = self.assign {
          let mut headers = Map::new();

          response.headers().iter().for_each(|(header, value)| {
            headers.insert(header.to_string(), json!(value.to_str().unwrap()));
          });

          let data = response.text().await.unwrap();

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

fn log_message_response(response: &Option<reqwest::Response>, duration_ms: f64) -> String {
  let mut message = String::new();
  match response {
    Some(response) => {
      write!(message, " {} {},", "URL:".bold(), response.url()).unwrap();
      write!(message, " {} {},", "STATUS:".bold(), response.status()).unwrap();
      write!(message, " {} {:?}", "HEADERS:".bold(), response.headers()).unwrap();
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
