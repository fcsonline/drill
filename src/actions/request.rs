use std::collections::HashMap;
use std::io::Read;

use colored::*;
use serde_json;
use time;
use yaml_rust::yaml;
use yaml_rust::Yaml;

use hyper::client::{Client, Response};
use hyper::header::{Cookie, Headers, SetCookie, UserAgent};
use hyper::method::Method;
use hyper::net::HttpsConnector;
use hyper_native_tls::native_tls::TlsConnector;
use hyper_native_tls::NativeTlsClient;

use crate::config;
use crate::interpolator;

use crate::actions::{Report, Runnable};

static USER_AGENT: &'static str = "drill";

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
      method: method,
      headers: headers,
      body: body.map(str::to_string),
      with_item: with_item,
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

  fn send_request(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, serde_json::Value>, config: &config::Config) -> (Option<Response>, f64) {
    let begin = time::precise_time_s();
    let mut uninterpolator = None;

    // Resolve the name
    let interpolated_name = if self.name.contains("{") {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(&self.name)
    } else {
      self.name.clone()
    };

    // Resolve the url
    let interpolated_url = if self.url.contains("{") {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(&self.url)
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

    let client = if interpolated_base_url.starts_with("https") {
      // Build a TSL connector
      let mut connector_builder = TlsConnector::builder();
      connector_builder.danger_accept_invalid_certs(config.no_check_certificate);

      let ssl = NativeTlsClient::from(connector_builder.build().unwrap());
      let connector = HttpsConnector::new(ssl);

      Client::with_connector(connector)
    } else {
      Client::new()
    };

    let interpolated_body;

    // Method
    let method = match self.method.to_uppercase().as_ref() {
      "GET" => Method::Get,
      "POST" => Method::Post,
      "PUT" => Method::Put,
      "PATCH" => Method::Patch,
      "DELETE" => Method::Delete,
      "HEAD" => Method::Head,
      _ => panic!("Unknown method '{}'", self.method),
    };

    // Resolve the body
    let request = if let Some(body) = self.body.as_ref() {
      interpolated_body = uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(body);

      client.request(method, interpolated_base_url.as_str()).body(&interpolated_body)
    } else {
      client.request(method, interpolated_base_url.as_str())
    };

    // Headers
    let mut headers = Headers::new();
    headers.set(UserAgent(USER_AGENT.to_string()));

    if let Some(Yaml::Hash(cookies)) = context.get("cookies") {
      headers.set(Cookie(cookies.iter().map(|(key, value)| format!("{}={}", key.as_str().unwrap(), value.as_str().unwrap())).collect()));
    }

    // Resolve headers
    for (key, val) in self.headers.iter() {
      let interpolated_header = uninterpolator.get_or_insert(interpolator::Interpolator::new(context, responses)).resolve(val);

      headers.set_raw(key.to_owned(), vec![interpolated_header.clone().into_bytes()]);
    }

    let response_result = request.headers(headers).send();
    let duration_ms = (time::precise_time_s() - begin) * 1000.0;

    match response_result {
      Err(e) => {
        if !config.quiet {
          println!("Error connecting '{}': {:?}", interpolated_base_url.as_str(), e);
        }
        (None, duration_ms)
      }
      Ok(response) => {
        if !config.quiet {
          let status_text = if response.status.is_server_error() {
            response.status.to_string().red()
          } else if response.status.is_client_error() {
            response.status.to_string().purple()
          } else {
            response.status.to_string().yellow()
          };

          println!("{:width$} {} {} {}", interpolated_name.green(), interpolated_base_url.blue().bold(), status_text, Request::format_time(duration_ms, config.nanosec).cyan(), width = 25);
        }

        (Some(response), duration_ms)
      }
    }
  }
}

impl Runnable for Request {
  fn execute(&self, context: &mut HashMap<String, Yaml>, responses: &mut HashMap<String, serde_json::Value>, reports: &mut Vec<Report>, config: &config::Config) {
    if self.with_item.is_some() {
      context.insert("item".to_string(), self.with_item.clone().unwrap());
    }

    let (res, duration_ms) = self.send_request(context, responses, config);

    match res {
      None => reports.push(Report {
        name: self.name.to_owned(),
        duration: duration_ms,
        status: 520u16,
      }),
      Some(mut response) => {
        reports.push(Report {
          name: self.name.to_owned(),
          duration: duration_ms,
          status: response.status.to_u16(),
        });

        if let Some(&SetCookie(ref cookies)) = response.headers.get::<SetCookie>() {
          let context_cookies = context.entry("cookies".to_string()).or_insert_with(|| Yaml::Hash(yaml::Hash::new()));
          if let Yaml::Hash(hash) = context_cookies {
            for cookie in cookies.iter() {
              let pair = cookie.split(';').next();
              if let Some(pair) = pair {
                let parts: Vec<&str> = pair.splitn(2, '=').collect();
                if parts.len() == 2 {
                  let (key, value) = (parts[0].trim(), parts[1].trim());
                  hash.insert(Yaml::String(key.to_string()), Yaml::String(value.to_string()));
                } else {
                  panic!("Invalid cookie pair {}", pair);
                }
              } else {
                panic!("Cookie pair not found");
              }
            }
          } else {
            panic!("The cookies context must be an array");
          }
        }

        if let Some(ref key) = self.assign {
          let mut data = String::new();

          response.read_to_string(&mut data).unwrap();

          let value: serde_json::Value = serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);

          responses.insert(key.to_owned(), value);
        }
      }
    }
  }
}
