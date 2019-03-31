use std::collections::HashMap;
use std::fmt;

use colored::*;
use futures::{Future, Stream};
use hyper::Client;
use hyper_tls::HttpsConnector;
use serde_json;
use std::sync::{Arc, Mutex};
use time;
use yaml_rust::Yaml;

use crate::actions::{Report, Runnable};
use crate::config;
use crate::interpolator::Interpolator;

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

  fn relative_path(&self) -> bool {
    &self.url[..1] == "/"
  }
}

impl fmt::Debug for Request {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "Request: {}\n", self.name)
  }
}

impl Runnable for Request {
  fn execute<'a>(
    &'a self,
    context: &'a Arc<Mutex<HashMap<String, Yaml>>>,
    responses: &'a Arc<Mutex<HashMap<String, serde_json::Value>>>,
    reports: &'a Arc<Mutex<Vec<Report>>>,
    config: &'a config::Config,
  ) -> (Box<Future<Item = (), Error = ()> + Send + 'a>) {
    let work = futures::future::ok::<_, ()>({}).and_then(move |_a| {
      let new_context = context.clone();
      let new_responses = responses.clone();

      let mut context = context.lock().unwrap();
      let responses = responses.lock().unwrap();

      if self.with_item.is_some() {
        context.insert("item".to_string(), self.with_item.clone().unwrap());
      }

      let begin = time::precise_time_s();
      let mut uninterpolator = None;

      // Resolve the name
      let interpolated_name = if Interpolator::has_interpolations(&self.name) {
        uninterpolator.get_or_insert(Interpolator::new(&context, &responses)).resolve(&self.name)
      } else {
        self.name.clone()
      };

      // Resolve the url
      let interpolated_url = if Interpolator::has_interpolations(&self.url) {
        uninterpolator.get_or_insert(Interpolator::new(&context, &responses)).resolve(&self.url)
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

      // TODO: I don't understand why I need this
      let interpolated_base_url_for_err = interpolated_base_url.clone();

      let client = if interpolated_base_url.starts_with("https") {
        // Build a TSL connector
        // TODO
        // let mut connector_builder = TlsConnector::builder();
        // connector_builder.danger_accept_invalid_certs(config.no_check_certificate);

        // let ssl = NativeTlsClient::from(connector_builder.build().unwrap());
        // let connector = HttpsConnector::new(ssl);

        // Client::with_connector(connector)

        let https = HttpsConnector::new(4).expect("TLS initialization failed");
        Client::builder().build::<_, hyper::Body>(https)
      } else {
        Client::new();

        // FIXME
        let https = HttpsConnector::new(4).expect("TLS initialization failed");
        Client::builder().build::<_, hyper::Body>(https)
      };

      // Resolve the body
      let interpolated_body = if let Some(body) = self.body.as_ref() {
        uninterpolator.get_or_insert(Interpolator::new(&context, &responses)).resolve(body)
      } else {
        "".to_string()
      };

      // Request building
      let mut request = hyper::Request::builder().method(self.method.to_uppercase().as_str()).uri(interpolated_base_url).body(hyper::Body::from(interpolated_body)).expect("request builder without body");

      // Headers
      let headers = request.headers_mut();
      headers.insert(hyper::header::USER_AGENT, USER_AGENT.parse().unwrap());

      if let Some(cookie) = context.get("cookie") {
        headers.insert(hyper::header::COOKIE, cookie.as_str().unwrap().parse().unwrap());
      }

      // Resolve headers
      for (key, val) in self.headers.iter() {
        let interpolated_header = uninterpolator.get_or_insert(Interpolator::new(&context, &responses)).resolve(val);

        let header_name = hyper::header::HeaderName::from_lowercase(key.to_lowercase().as_bytes()).unwrap();
        headers.insert(header_name, interpolated_header.parse().unwrap());
      }

      let req = client
        .request(request)
        .and_then(move |response| {
          let duration_ms = (time::precise_time_s() - begin) * 1000.0;

          if !config.quiet {
            let message = response.status().to_string();
            let status_text = if response.status().is_server_error() {
              message.red()
            } else if response.status().is_client_error() {
              message.purple()
            } else {
              message.yellow()
            };

            println!("{:width$} {} {}", interpolated_name.green(), status_text, Request::format_time(duration_ms, config.nanosec).cyan(), width = 25);
          }

          let mut reports = reports.lock().unwrap();

          reports.push(Report {
            name: self.name.clone(),
            duration: duration_ms,
            status: response.status().as_u16(),
          });

          if let Some(cookie) = response.headers().get(hyper::header::SET_COOKIE) {
            let value = String::from(cookie.to_str().unwrap().split(";").next().unwrap());
            let mut context = new_context.lock().unwrap();

            context.insert("cookie".to_string(), Yaml::String(value));
          }

          response.into_body().concat2()
        })
        .map(move |body| {
          let mut responses = new_responses.lock().unwrap();

          if let Some(ref key) = self.assign {
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            responses.insert(key.to_owned(), value);
          };

          ()
        })
        .map_err(move |err| {
          if !config.quiet {
            println!("Error connecting '{}': {:?}", interpolated_base_url_for_err.as_str(), err);
          }
        });

      req
    });

    Box::new(work)
  }

  fn has_interpolations(&self) -> bool {
    Interpolator::has_interpolations(&self.name)
      || Interpolator::has_interpolations(&self.url)
      || Interpolator::has_interpolations(&self.body.clone().unwrap_or("".to_string()))
      || self.relative_path()
      || self.with_item.is_some()
      || self.assign.is_some()
      || false // TODO: headers
  }
}
