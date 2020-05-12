use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::*;
use reqwest::{
  header::{self, HeaderMap, HeaderName, HeaderValue},
  Client, ClientBuilder, Method, Response,
};
use yaml_rust::yaml;
use yaml_rust::Yaml;

use url::Url;

use crate::config;
use crate::interpolator;

use crate::actions::{Report, Runnable};

static USER_AGENT: &str = "drill";

#[derive(Clone)]
pub struct Request {
  name: String,
  url: String,
  time: f64,
  method: String,
  headers: HashMap<String, String>,
  pub body: Option<String>,
  pub with_item: Option<Yaml>,
  pub assign: Option<String>,
}

impl Request {
  pub fn is_that_you(item: &Yaml) -> bool {
    item["request"].as_hash().is_some()
  }

  pub fn new(item: &Yaml, with_item: Option<Yaml>) -> Request {
    let reference: Option<&str> = item["assign"].as_str();
    let body: Option<&str> = item["request"]["body"].as_str();
    let method = if let Some(v) = item["request"]["method"].as_str() {
      v.to_string().to_uppercase()
    } else {
      "GET".to_string()
    };

    if method == "POST" && body.is_none() {
      if item["request"]["body"].as_hash().is_some() {
        panic!("Body needs to be a string. Try adding quotes");
      } else {
        panic!("POST requests require a body attribute");
      }
    }

    let mut headers = HashMap::new();

    if let Some(hash) = item["request"]["headers"].as_hash() {
      for (key, val) in hash.iter() {
        if let Some(vs) = val.as_str() {
          headers.insert(key.as_str().unwrap().to_string(), vs.to_string());
        } else {
          panic!("{} Headers must be strings!!", "WARNING!".yellow().bold());
        }
      }
    }

    Request {
      name: item["name"].as_str().unwrap().to_string(),
      url: item["request"]["url"].as_str().unwrap().to_string(),
      time: 0.0,
      method,
      headers,
      body: body.map(str::to_string),
      with_item,
      assign: reference.map(str::to_string),
    }
  }

  fn format_time(tdiff: f64, nanosec: bool) -> String {
    if nanosec {
      (1_000_000.0 * tdiff).round().to_string() + "ns"
    } else {
      tdiff.round().to_string() + "ms"
    }
  }

  async fn send_request(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, serde_json::Value>, pool: &mut HashMap<String, Client>, config: &config::Config) -> (Option<Response>, f64) {
    let mut uninterpolator = None;

    // Resolve the name
    let interpolated_name = if self.name.contains('{') {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(&self.name, !config.relaxed_interpolations)
    } else {
      self.name.clone()
    };

    // Resolve the url
    let interpolated_url = if self.url.contains('{') {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(&self.url, !config.relaxed_interpolations)
    } else {
      self.url.clone()
    };

    // Resolve relative urls
    let interpolated_base_url = if &interpolated_url[..1] == "/" {
      match context.get("base") {
        Some(value) => {
          if let Some(vs) = value.as_str() {
            format!("{}{}", vs.to_string(), interpolated_url)
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

    let url = Url::parse(&interpolated_base_url).unwrap();
    let domain = format!("{}://{}:{}", url.scheme(), url.host_str().unwrap(), url.port().unwrap_or(0)); // Unique domain key for keep-alive
    let client = pool.entry(domain).or_insert_with(|| ClientBuilder::default().danger_accept_invalid_certs(config.no_check_certificate).build().unwrap());

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
    let request = if let Some(body) = self.body.as_ref() {
      interpolated_body = uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(body, !config.relaxed_interpolations);

      client.request(method, interpolated_base_url.as_str()).body(interpolated_body)
    } else {
      client.request(method, interpolated_base_url.as_str())
    };

    // Headers
    let mut headers = HeaderMap::new();
    headers.insert(header::USER_AGENT, HeaderValue::from_str(USER_AGENT).unwrap());

    if let Some(Yaml::Hash(cookies)) = context.get("cookies") {
      let cookie = cookies.iter().map(|(key, value)| format!("{}={}", key.as_str().unwrap(), value.as_str().unwrap())).collect::<Vec<_>>().join(";");
      headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
    }

    // Resolve headers
    for (key, val) in self.headers.iter() {
      let interpolated_header = uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(val, !config.relaxed_interpolations);
      headers.insert(HeaderName::from_bytes(key.as_bytes()).unwrap(), HeaderValue::from_str(&interpolated_header).unwrap());
    }

    let begin = Instant::now();
    let response_result = request.headers(headers).timeout(Duration::from_secs(10)).send().await;
    let duration_ms = begin.elapsed().as_secs_f64() * 1000.0;

    match response_result {
      Err(e) => {
        if !config.quiet {
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

#[async_trait]
impl Runnable for Request {
  async fn execute(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, serde_json::Value>, reports: &mut Vec<Report>, pool: &mut HashMap<String, Client>, config: &config::Config) {
    if self.with_item.is_some() {
      context.insert("item".to_string(), self.with_item.clone().unwrap());
    }

    let (res, duration_ms) = self.send_request(context, responses, pool, config).await;

    match res {
      None => reports.push(Report {
        name: self.name.to_owned(),
        duration: duration_ms,
        status: 520u16,
      }),
      Some(response) => {
        reports.push(Report {
          name: self.name.to_owned(),
          duration: duration_ms,
          status: response.status().as_u16(),
        });

        let context_cookies = context.entry("cookies".to_string()).or_insert_with(|| Yaml::Hash(yaml::Hash::new()));
        if let Yaml::Hash(hash) = context_cookies {
          for cookie in response.cookies() {
            hash.insert(Yaml::String(cookie.name().to_string()), Yaml::String(cookie.value().to_string()));
          }
        } else {
          panic!("The cookies context must be an array");
        }

        if let Some(ref key) = self.assign {
          let data = response.text().await.unwrap();
          let value: serde_json::Value = serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
          responses.insert(key.to_owned(), value);
        }
      }
    }
  }
}
