use std::collections::HashMap;
use std::io::{self};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use reqwest::{
    header::{self, HeaderMap, HeaderName, HeaderValue},
    ClientBuilder, Method, Response,
};
use std::fmt::Write;
use url::Url;
use yaml_rust2::Yaml;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::actions::{extract, extract_optional};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

use crate::actions::{Report, Runnable};

static USER_AGENT: &str = "drill";

#[derive(Clone)]
#[allow(dead_code)]
pub struct Request {
    name: String,
    url: String,
    time: f64,
    method: String,
    headers: HashMap<String, String>,
    pub body: Option<String>,
    pub with_item: Option<Yaml>,
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
    pub fn is_that_you(item: &Yaml) -> bool {
        item["request"].as_hash().is_some()
    }

    pub fn new(item: &Yaml, with_item: Option<Yaml>, index: Option<u32>) -> Result<Request, io::Error> {
        let name = extract(item, "name")?;
        let url = extract(&item["request"], "url")?;
        let assign = extract_optional(item, "assign")?;

        let method = match extract_optional(&item["request"], "method")? {
            Some(v) => v.to_uppercase(),
            None => "GET".to_string(),
        };

        let body_verbs = vec!["POST", "PATCH", "PUT"];
        let body = if body_verbs.contains(&method.as_str()) {
            Some(extract(&item["request"], "body")?)
        } else {
            None
        };

        let mut headers = HashMap::new();

        if let Some(hash) = item["request"]["headers"].as_hash() {
            for (key, val) in hash.iter() {
                if let Some(vs) = val.as_str() {
                    headers.insert(key.as_str().unwrap().to_string(), vs.to_string());
                } else {
                    return Err(io::Error::new(io::ErrorKind::Other, format!("{} Headers must be strings!!", "WARNING!".yellow().bold())));
                }
            }
        }

        Ok(Request {
            name,
            url,
            time: 0.0,
            method,
            headers,
            body,
            with_item,
            index,
            assign,
        })
    }

    fn format_time(tdiff: f64, nanosec: bool) -> String {
        if nanosec {
            (1_000_000.0 * tdiff).round().to_string() + "ns"
        } else {
            tdiff.round().to_string() + "ms"
        }
    }

    async fn send_request(&self, context: &mut Context, pool: &Pool, config: &Config) -> Result<(Option<Response>, f64), io::Error> {
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
                        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} Wrong type 'base' variable!", "WARNING!".yellow().bold())));
                    }
                }
                _ => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} Unknown 'base' variable!", "WARNING!".yellow().bold())));
                }
            }
        } else {
            interpolated_url
        };

        let url = match Url::parse(&interpolated_base_url) {
            Ok(url) => url,
            Err(err) => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} Invalid URL: {}", "WARNING!".yellow().bold(), err))),
        };
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
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("Unknown method '{}'", self.method))),
        };

        // Resolve the body
        let (client, request) = {
            let mut pool_lock = match pool.lock() {
                Ok(pool2) => pool2,
                Err(err) => {
                    return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to lock pool: {}", err)));
                }
            };
            let client = pool_lock.entry(domain).or_insert({
                match ClientBuilder::default().danger_accept_invalid_certs(config.no_check_certificate).build() {
                    Ok(client) => client,
                    Err(err) => {
                        return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to create client: {}", err)));
                    }
                }
            });

            let request = if let Some(body) = self.body.as_ref() {
                interpolated_body = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(body, !config.relaxed_interpolations);

                client.request(method, interpolated_base_url.as_str()).body(interpolated_body)
            } else {
                client.request(method, interpolated_base_url.as_str())
            };

            (client.clone(), request)
        };

        // Headers
        let mut headers = HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            match HeaderValue::from_str(USER_AGENT) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Failed to parse USER_AGENT header: {}", err);
                    HeaderValue::from_static("Unknown")
                }
            },
        );

        if let Some(cookies) = context.get("cookies") {
            let cookies: Map<String, Value> = serde_json::from_value(cookies.clone())?;
            let cookie = cookies.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(";");

            headers.insert(
                header::COOKIE,
                match HeaderValue::from_str(&cookie) {
                    Ok(value) => value,
                    Err(err) => {
                        eprintln!("Failed to parse COOKIE header: {}", err);
                        HeaderValue::from_static("Unknown")
                    }
                },
            );
        }

        // Resolve headers
        for (key, val) in self.headers.iter() {
            let interpolated_header = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(val, !config.relaxed_interpolations);
            headers.insert(
                HeaderName::from_bytes(key.as_bytes()).unwrap(),
                match HeaderValue::from_str(&interpolated_header) {
                    Ok(value) => value,
                    Err(err) => {
                        eprintln!("Failed to parse header '{}': {}", key, err);
                        HeaderValue::from_static("Unknown")
                    }
                },
            );
        }

        let request_builder = request.headers(headers).timeout(Duration::from_secs(config.timeout));
        let request = match request_builder.build() {
            Ok(request) => request,
            Err(err) => {
                return Err(io::Error::new(io::ErrorKind::Other, err));
            }
        };

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
                Ok((None, duration_ms))
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

                Ok((Some(response), duration_ms))
            }
        }
    }
}

fn yaml_to_json(data: Yaml) -> Result<Value, io::Error> {
    match data {
        Yaml::Boolean(b) => Ok(json!(b)),
        Yaml::Integer(i) => Ok(json!(i)),
        Yaml::String(s) => Ok(json!(s)),
        Yaml::Hash(h) => {
            let mut map = Map::new();

            for (key, value) in h.iter() {
                map.entry(key.as_str().unwrap()).or_insert(yaml_to_json(value.clone())?);
            }

            Ok(json!(map))
        }
        Yaml::Array(v) => {
            let mut array = Vec::new();

            for value in v.iter() {
                array.push(yaml_to_json(value.clone())?);
            }

            Ok(json!(array))
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "Unknown Yaml node")),
    }
}

#[async_trait]
impl Runnable for Request {
    async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &Pool, config: &Config) -> Result<(), io::Error> {
        match self.with_item.clone() {
            Some(item) => {
                context.insert("item".to_string(), yaml_to_json(item.clone())?);
            }
            None => {}
        }

        match self.index {
            Some(index) => {
                context.insert("index".to_string(), json!(index));
            }
            None => {}
        }

        let (res, duration_ms) = self.send_request(context, pool, config).await?;

        let log_message_response = if config.verbose {
            Some(log_message_response(&res, duration_ms))
        } else {
            None
        };

        match res {
            None => {
                reports.push(Report {
                    name: self.name.to_owned(),
                    duration: duration_ms,
                    status: 520u16,
                });
                Ok(())
            }
            Some(response) => {
                let status = response.status().as_u16();

                reports.push(Report {
                    name: self.name.to_owned(),
                    duration: duration_ms,
                    status,
                });

                for cookie in response.cookies() {
                    let cookies = match context.entry("cookies").or_insert_with(|| json!({})).as_object_mut() {
                        Some(cookies) => cookies,
                        None => {
                            return Err(io::Error::new(io::ErrorKind::Other, "Failed to get cookies"));
                        }
                    };
                    cookies.insert(cookie.name().to_string(), json!(cookie.value().to_string()));
                }

                let data = if let Some(ref key) = self.assign {
                    let mut headers = Map::new();

                    response.headers().iter().for_each(|(header, value)| {
                        headers.insert(header.to_string(), json!(value.to_str().unwrap()));
                    });

                    let data = match response.text().await {
                        Ok(data) => data,
                        Err(err) => {
                            return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to read response text: {}", err)));
                        }
                    };

                    let body: Value = serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);

                    let assigned = AssignedRequest {
                        status,
                        body,
                        headers,
                    };

                    let value = serde_json::to_value(assigned)?;

                    context.insert(key.to_owned(), value);

                    Some(data)
                } else {
                    None
                };

                if let Some(msg) = log_message_response {
                    log_response(msg, &data)
                }

                Ok(())
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
