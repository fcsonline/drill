use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use colored::Colorize;
use encoding_rs::{Encoding, UTF_8};
use reqwest::{
  Method, StatusCode,
  header::{self, HeaderMap, HeaderName, HeaderValue},
  multipart::{Form, Part},
};
use serde_yaml::Value as YamlValue;
use std::fmt::Write;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use url::Url;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::actions::save::LAST_RESPONSE_KEY;
use crate::actions::{extract, extract_optional};
use crate::benchmark::{ClientEntry, Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;
use crate::metrics::{self, RequestMetrics};

use crate::actions::{Report, Runnable};

static USER_AGENT: &str = "drill";

#[derive(Clone)]
pub enum Body {
  Template(String),
  Binary(Vec<u8>),
  UrlEncoded(HashMap<String, String>),
  FormData(Vec<FormPart>),
  GraphQL {
    query: String,
    variables: Option<HashMap<String, String>>,
  },
}

/// One field of a `multipart/form-data` body: either a text field (`value`)
/// or a file field (`file`, with an optional `content_type`).
#[derive(Clone)]
pub struct FormPart {
  pub key: String,
  pub value: Option<String>,
  pub file: Option<String>,
  pub content_type: Option<String>,
}

#[derive(Clone)]
pub enum ApiKeyLocation {
  Header,
  Query,
}

/// Authentication scheme attached to a request via its `auth` block:
/// `apikey`, `oauth2` (`client_credentials` flow), `basic` or `bearer`.
#[derive(Clone)]
pub enum AuthConfig {
  Basic {
    username: String,
    password: String,
  },
  Bearer {
    token: String,
  },
  ApiKey {
    key: String,
    value: String,
    location: ApiKeyLocation,
  },
  OAuth2ClientCredentials {
    token_url: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
    /// Context key caching the obtained token and its expiry (`<key>_expires`).
    save_token_as: Option<String>,
  },
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct Request {
  name: String,
  url: String,
  time: f64,
  method: String,
  headers: HashMap<String, String>,
  auth: Option<AuthConfig>,
  pub body: Option<Body>,
  pub with_item: Option<YamlValue>,
  pub index: Option<u32>,
  pub assign: Option<String>,
  weight: u32,
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
      } else if let Some(body) = request_val.get("body") {
        Some(parse_structured_body(body))
      } else {
        panic!("{} Body must be string, file, hex, urlencoded, formdata or graphql!!", "WARNING!".yellow().bold());
      }
    } else {
      None
    };

    let weight = item.get("weight").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(1);

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

    let auth = request_val.get("auth").map(|auth_val| match extract(auth_val, "type").to_lowercase().as_str() {
      "apikey" => {
        let key = extract(auth_val, "key");
        let value = extract(auth_val, "value");
        let location = match extract(auth_val, "in").to_lowercase().as_str() {
          "query" => ApiKeyLocation::Query,
          _ => ApiKeyLocation::Header,
        };
        AuthConfig::ApiKey {
          key,
          value,
          location,
        }
      }
      "oauth2" => {
        let flow = extract(auth_val, "flow").to_lowercase();
        if flow != "client_credentials" {
          panic!("{} Only the 'client_credentials' OAuth2 flow is supported!", "WARNING!".yellow().bold());
        }
        let token_url = extract(auth_val, "token_url");
        let client_id = extract(auth_val, "client_id");
        let client_secret = extract(auth_val, "client_secret");
        let scope = extract_optional(auth_val, "scope");
        let save_token_as = extract_optional(auth_val, "save_token_as");
        AuthConfig::OAuth2ClientCredentials {
          token_url,
          client_id,
          client_secret,
          scope,
          save_token_as,
        }
      }
      "basic" => {
        let username = extract(auth_val, "username");
        let password = extract(auth_val, "password");
        AuthConfig::Basic {
          username,
          password,
        }
      }
      "bearer" => {
        let token = extract(auth_val, "token");
        AuthConfig::Bearer {
          token,
        }
      }
      other => panic!("{} Unknown auth type '{}'!", "WARNING!".yellow().bold(), other),
    });

    Request {
      name,
      url,
      time: 0.0,
      method,
      headers,
      auth,
      body,
      with_item,
      index,
      assign,
      weight,
    }
  }

  fn format_time(tdiff: f64, nanosec: bool) -> String {
    if nanosec {
      (1_000_000.0 * tdiff).round().to_string() + "ns"
    } else {
      tdiff.round().to_string() + "ms"
    }
  }

  async fn send_request(&self, context: &mut Context, pool: &Pool, config: &Config) -> (Option<ResponseData>, RequestMetrics) {
    // Resolve authentication first. OAuth2 client credentials may perform a
    // token exchange and mutate the context, so it runs before the lazy
    // interpolator below borrows the context.
    let resolved_auth = self.resolve_auth(context, pool, config).await;

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

    // API keys sent in the query string are appended here so the pooled client
    // builds the final URL below.
    let request_url = match &resolved_auth {
      Some(ResolvedAuth::Query(key, value)) => {
        let mut url = Url::parse(&interpolated_base_url).expect("Invalid url!");
        url.query_pairs_mut().append_pair(key, value);
        url.to_string()
      }
      _ => interpolated_base_url.clone(),
    };

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
    let (middleware, request) = {
      let mut pool2 = pool.lock().unwrap();
      let entry = pool2.entry(domain).or_insert_with(|| ClientEntry::new(config.no_check_certificate));

      let request = match self.body.as_ref() {
        Some(Body::Template(template_body)) => {
          interpolated_body = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(template_body, !config.relaxed_interpolations);
          entry.client.request(method, request_url.as_str()).body(interpolated_body)
        }
        Some(Body::Binary(binary_body)) => entry.client.request(method, request_url.as_str()).body(binary_body.clone()),
        Some(Body::UrlEncoded(params)) => {
          let interpolator = uninterpolator.get_or_insert(interpolator::Interpolator::new(context));
          let encoded: Vec<(String, String)> = params.iter().map(|(key, value)| (key.clone(), interpolator.resolve(value, !config.relaxed_interpolations))).collect();
          entry.client.request(method, request_url.as_str()).form(&encoded)
        }
        Some(Body::FormData(parts)) => {
          let interpolator = uninterpolator.get_or_insert(interpolator::Interpolator::new(context));
          let mut form = Form::new();
          for part in parts {
            let key = interpolator.resolve(&part.key, !config.relaxed_interpolations);
            if let Some(file_path) = &part.file {
              let file_path = interpolator.resolve(file_path, !config.relaxed_interpolations);
              let mut file = File::open(&file_path).expect("Unable to open file");
              let mut buffer = Vec::new();
              file.read_to_end(&mut buffer).expect("Unable to read file");
              let file_name = Path::new(&file_path).file_name().and_then(|name| name.to_str()).unwrap_or("file").to_string();
              let mut file_part = Part::bytes(buffer).file_name(file_name);
              if let Some(content_type) = &part.content_type {
                let content_type = interpolator.resolve(content_type, !config.relaxed_interpolations);
                file_part = file_part.mime_str(&content_type).expect("Invalid content type");
              }
              form = form.part(key, file_part);
            } else {
              let value = part.value.as_ref().map(|value| interpolator.resolve(value, !config.relaxed_interpolations)).unwrap_or_default();
              form = form.text(key, value);
            }
          }
          entry.client.request(method, request_url.as_str()).multipart(form)
        }
        Some(Body::GraphQL {
          query,
          variables,
        }) => {
          let interpolator = uninterpolator.get_or_insert(interpolator::Interpolator::new(context));
          let query = interpolator.resolve(query, !config.relaxed_interpolations);
          let variables = variables.as_ref().map(|variables| {
            let mut map = Map::new();
            for (key, value) in variables.iter() {
              map.insert(key.clone(), json!(interpolator.resolve(value, !config.relaxed_interpolations)));
            }
            json!(map)
          });
          let payload = json!({ "query": query, "variables": variables });
          entry.client.request(method, request_url.as_str()).json(&payload)
        }
        None => entry.client.request(method, request_url.as_str()),
      };

      (entry.middleware.clone(), request)
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

    // Apply the resolved authentication header (API-key header, basic, bearer
    // or OAuth2 token). API-key query params were already merged into the URL.
    if let Some(ResolvedAuth::Header(name, value)) = &resolved_auth {
      headers.insert(name.clone(), value.clone());
    }

    let request_builder = request.headers(headers).timeout(Duration::from_secs(config.timeout));
    let request = request_builder.build().expect("Cannot create request");

    if config.verbose {
      log_request(&request, config.stats_json);
    }

    let metrics = metrics::new_metrics();
    let mut extensions = http::Extensions::new();
    extensions.insert(metrics.clone());

    let begin = Instant::now();
    let response_result = middleware.execute_with_extensions(request, &mut extensions).await;

    let mut response = match response_result {
      Err(e) => {
        let metrics = metrics.lock().unwrap().clone();
        if !config.quiet || config.verbose {
          if config.stats_json {
            eprintln!("Error connecting '{}': {:?}", interpolated_base_url.as_str(), e);
          } else {
            println!("Error connecting '{}': {:?}", interpolated_base_url.as_str(), e);
          }
        }
        return (None, metrics);
      }
      Ok(response) => response,
    };

    // Snapshot everything that borrows the response before the body is consumed.
    let url = response.url().clone();
    let status = response.status();
    let headers = response.headers().clone();
    let cookies: Vec<(String, String)> = response.cookies().map(|cookie| (cookie.name().to_string(), cookie.value().to_string())).collect();

    let mut metrics = metrics.lock().unwrap().clone();

    // Read the full response body so the measured latency reflects
    // time-to-last-byte, matching wrk, k6, vegeta and other load-testing tools.
    // The body is drained one chunk at a time. It is only buffered when an
    // `assign` needs to decode it; otherwise each chunk is dropped immediately,
    // so peak memory stays O(chunk) rather than O(body) even for large responses.
    let mut body_buf = self.assign.is_some().then(Vec::new);
    let mut size_download = 0u64;
    let drain_result = loop {
      match response.chunk().await {
        Ok(Some(chunk)) => {
          size_download += chunk.len() as u64;
          if let Some(buf) = body_buf.as_mut() {
            buf.extend_from_slice(&chunk);
          }
        }
        Ok(None) => break Ok(()),
        Err(e) => break Err(e),
      }
    };

    let response_header_size = header_size_response(&headers);
    let body_bytes = body_buf.unwrap_or_default();

    metrics.time_total_ms = begin.elapsed().as_secs_f64() * 1000.0;
    metrics.size_header_response = response_header_size;
    metrics.size_download = size_download;
    metrics.size_total = metrics.size_request + response_header_size + size_download;

    if let Err(e) = drain_result {
      if !config.quiet || config.verbose {
        if config.stats_json {
          eprintln!("Error reading body '{}': {:?}", interpolated_base_url.as_str(), e);
        } else {
          println!("Error reading body '{}': {:?}", interpolated_base_url.as_str(), e);
        }
      }
      return (None, metrics);
    }

    if !config.quiet {
      let status_text = if status.is_server_error() {
        status.to_string().red()
      } else if status.is_client_error() {
        status.to_string().purple()
      } else {
        status.to_string().yellow()
      };

      if config.stats_json {
        eprintln!("{:width$} {} {} {}", interpolated_name.green(), interpolated_base_url.blue().bold(), status_text, Request::format_time(metrics.time_total_ms, config.nanosec).cyan(), width = 25);
      } else {
        println!("{:width$} {} {} {}", interpolated_name.green(), interpolated_base_url.blue().bold(), status_text, Request::format_time(metrics.time_total_ms, config.nanosec).cyan(), width = 25);
      }
    }

    // Decode the body (only present for `assign`) using the response charset,
    // mirroring reqwest's `Response::text`, so non-UTF-8 bodies are not corrupted.
    let body = self.assign.is_some().then(|| decode_body(&headers, &body_bytes));

    (
      Some(ResponseData {
        url,
        status,
        headers,
        cookies,
        body,
      }),
      metrics,
    )
  }

  /// Applies the configured auth scheme, returning the header or query
  /// parameter to attach. Interpolates values and, for `oauth2`, acquires and
  /// caches a client-credentials token when needed.
  async fn resolve_auth(&self, context: &mut Context, pool: &Pool, config: &Config) -> Option<ResolvedAuth> {
    match &self.auth {
      Some(AuthConfig::Basic {
        username,
        password,
      }) => {
        let interpolator = interpolator::Interpolator::new(context);
        let username = interpolator.resolve(username, !config.relaxed_interpolations);
        let password = interpolator.resolve(password, !config.relaxed_interpolations);
        let encoded = BASE64.encode(format!("{username}:{password}"));
        Some(ResolvedAuth::Header(header::AUTHORIZATION, HeaderValue::from_str(&format!("Basic {encoded}")).expect("invalid basic auth header")))
      }
      Some(AuthConfig::Bearer {
        token,
      }) => {
        let interpolator = interpolator::Interpolator::new(context);
        let token = interpolator.resolve(token, !config.relaxed_interpolations);
        Some(ResolvedAuth::Header(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}")).expect("invalid bearer token")))
      }
      Some(AuthConfig::ApiKey {
        key,
        value,
        location,
      }) => {
        let interpolator = interpolator::Interpolator::new(context);
        let key = interpolator.resolve(key, !config.relaxed_interpolations);
        let value = interpolator.resolve(value, !config.relaxed_interpolations);
        match location {
          ApiKeyLocation::Header => Some(ResolvedAuth::Header(HeaderName::from_bytes(key.as_bytes()).expect("invalid api key header name"), HeaderValue::from_str(&value).expect("invalid api key header value"))),
          ApiKeyLocation::Query => Some(ResolvedAuth::Query(key, value)),
        }
      }
      Some(AuthConfig::OAuth2ClientCredentials {
        ..
      }) => {
        let token = self.oauth2_token(context, pool, config).await;
        Some(ResolvedAuth::Header(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}")).expect("invalid oauth2 token")))
      }
      None => None,
    }
  }

  /// Obtains an OAuth2 access token for the `client_credentials` flow. A
  /// previously acquired token is reused while its cached expiry has not been
  /// reached; otherwise a new token is requested from `token_url` and stored in
  /// the context under `save_token_as` (expiry under `<save_token_as>_expires`).
  async fn oauth2_token(&self, context: &mut Context, pool: &Pool, config: &Config) -> String {
    let (token_url, client_id, client_secret, scope, save_token_as) = match &self.auth {
      Some(AuthConfig::OAuth2ClientCredentials {
        token_url,
        client_id,
        client_secret,
        scope,
        save_token_as,
      }) => (token_url, client_id, client_secret, scope, save_token_as),
      _ => unreachable!("oauth2_token called without an OAuth2 auth config"),
    };

    if let Some(save_token_as) = save_token_as {
      let expires_key = format!("{save_token_as}_expires");
      let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
      if let (Some(token), Some(expires)) = (context.get(save_token_as), context.get(&expires_key))
        && let (Some(token), Some(expires)) = (token.as_str(), expires.as_f64())
        && now < expires
      {
        return token.to_string();
      }
    }

    let interpolator = interpolator::Interpolator::new(context);
    let token_url = interpolator.resolve(token_url, !config.relaxed_interpolations);
    let client_id = interpolator.resolve(client_id, !config.relaxed_interpolations);
    let client_secret = interpolator.resolve(client_secret, !config.relaxed_interpolations);
    let scope = scope.as_ref().map(|s| interpolator.resolve(s, !config.relaxed_interpolations));

    let mut params: Vec<(String, String)> = vec![("grant_type".to_string(), "client_credentials".to_string()), ("client_id".to_string(), client_id), ("client_secret".to_string(), client_secret)];
    if let Some(scope) = scope {
      params.push(("scope".to_string(), scope));
    }

    let token_url_parsed = Url::parse(&token_url).expect("Invalid token url");
    let token_domain = format!("{}://{}:{}", token_url_parsed.scheme(), token_url_parsed.host_str().unwrap(), token_url_parsed.port().unwrap_or(0));
    let entry = {
      let mut pool2 = pool.lock().unwrap();
      pool2.entry(token_domain).or_insert_with(|| ClientEntry::new(config.no_check_certificate)).clone()
    };

    let response = entry.client.post(&token_url).form(&params).send().await;
    let response = match response {
      Ok(response) => response,
      Err(e) => panic!("OAuth2 token request to '{}' failed: {:?}", token_url, e),
    };

    let status = response.status();
    let payload: Value = response.json().await.unwrap_or_else(|_| panic!("OAuth2 token response from '{}' is not valid JSON (status '{}')", token_url, status));
    if !status.is_success() {
      panic!("OAuth2 token request to '{}' failed with status '{}': {}", token_url, status, payload);
    }

    let token = payload.get("access_token").and_then(|v| v.as_str()).unwrap_or_else(|| panic!("OAuth2 token response from '{}' is missing 'access_token': {}", token_url, payload)).to_string();
    let expires_in = payload.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(3600);

    if let Some(save_token_as) = save_token_as {
      let expires_key = format!("{save_token_as}_expires");
      let expires_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() + expires_in as f64;
      context.insert(save_token_as.clone(), json!(token));
      context.insert(expires_key, json!(expires_at));
    }

    token
  }
}

/// Result of applying the request's `auth` block: a header to attach or a
/// query parameter to merge into the URL.
enum ResolvedAuth {
  Header(HeaderName, HeaderValue),
  Query(String, String),
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

fn header_size_response(headers: &HeaderMap) -> u64 {
  // Approximate wire size: header name + value + separator overhead.
  headers
    .iter()
    .map(|(name, value)| {
      let name_len = name.as_str().len() as u64;
      let value_len = value.as_bytes().len() as u64;
      name_len + value_len + 4
    })
    .sum()
}

/// Parses a structured `body` mapping (urlencoded, formdata or graphql) into a
/// `Body` variant. The string/file/hex shorthand forms are handled in
/// `Request::new`.
fn parse_structured_body(body: &YamlValue) -> Body {
  if let Some(mapping) = body.get("urlencoded").and_then(|v| v.as_mapping()) {
    let mut params = HashMap::new();
    for (key, value) in mapping.iter() {
      let key = key.as_str().unwrap_or_else(|| panic!("{} Urlencoded keys must be strings!!", "WARNING!".yellow().bold()));
      let value = value.as_str().unwrap_or_else(|| panic!("{} Urlencoded values must be strings!!", "WARNING!".yellow().bold()));
      params.insert(key.to_string(), value.to_string());
    }
    Body::UrlEncoded(params)
  } else if let Some(parts) = body.get("formdata").and_then(|v| v.as_sequence()) {
    let mut form_parts = Vec::new();
    for entry in parts.iter() {
      let key = entry.get("key").and_then(|v| v.as_str()).unwrap_or_else(|| panic!("{} FormData parts must have a string 'key'!!", "WARNING!".yellow().bold())).to_string();
      let value = entry.get("value").and_then(|v| v.as_str()).map(str::to_string);
      let file = entry.get("file").and_then(|v| v.as_str()).map(str::to_string);
      let content_type = entry.get("content_type").and_then(|v| v.as_str()).map(str::to_string);
      form_parts.push(FormPart {
        key,
        value,
        file,
        content_type,
      });
    }
    Body::FormData(form_parts)
  } else if let Some(graphql) = body.get("graphql") {
    let query = graphql.get("query").and_then(|v| v.as_str()).unwrap_or_else(|| panic!("{} GraphQL body must have a string 'query'!!", "WARNING!".yellow().bold())).to_string();
    let variables = graphql.get("variables").and_then(|v| v.as_mapping()).map(|mapping| {
      let mut vars = HashMap::new();
      for (key, value) in mapping.iter() {
        let key = key.as_str().unwrap_or_else(|| panic!("{} GraphQL variable keys must be strings!!", "WARNING!".yellow().bold()));
        let value = value.as_str().unwrap_or_else(|| panic!("{} GraphQL variable values must be strings!!", "WARNING!".yellow().bold()));
        vars.insert(key.to_string(), value.to_string());
      }
      vars
    });
    Body::GraphQL {
      query,
      variables,
    }
  } else {
    panic!("{} Body must be string, file, hex, urlencoded, formdata or graphql!!", "WARNING!".yellow().bold());
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
  fn weight(&self) -> u32 {
    self.weight
  }

  async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &Pool, config: &Config) {
    if let Some(with_item) = &self.with_item {
      context.insert("item".to_string(), yaml_to_json(with_item.clone()));
    }

    if let Some(index) = self.index {
      context.insert("index".to_string(), json!(index));
    }

    let (res, metrics) = self.send_request(context, pool, config).await;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();

    let log_message_response = if config.verbose {
      Some(log_message_response(&res, &metrics))
    } else {
      None
    };

    match res {
      None => reports.push(Report {
        name: self.name.to_owned(),
        duration: metrics.time_total_ms,
        status: 520u16,
        timestamp,
        metrics: metrics.clone(),
      }),
      Some(response) => {
        let status = response.status.as_u16();

        reports.push(Report {
          name: self.name.to_owned(),
          duration: metrics.time_total_ms,
          status,
          timestamp,
          metrics: metrics.clone(),
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

        // Snapshot of the last response for later `save` plan steps.
        let mut response_headers = Map::new();
        response.headers.iter().for_each(|(header, value)| {
          let value = value.to_str().map(|s| s.to_string()).unwrap_or_else(|_| String::from_utf8_lossy(value.as_bytes()).into_owned());
          response_headers.insert(header.to_string(), json!(value));
        });

        context.insert(
          LAST_RESPONSE_KEY.to_string(),
          json!({
            "status": status,
            "body": data.clone().unwrap_or_default(),
            "headers": response_headers,
            "url": response.url.to_string(),
            "duration": metrics.time_total_ms,
            "time_starttransfer_ms": metrics.time_starttransfer_ms,
            "time_total_ms": metrics.time_total_ms,
            "size_upload": metrics.size_upload,
            "size_download": metrics.size_download,
            "size_request": metrics.size_request,
            "size_header_request": metrics.size_header_request,
            "size_header_response": metrics.size_header_response,
            "size_total": metrics.size_total,
          }),
        );

        if let Some(msg) = log_message_response {
          log_response(msg, &data, config.stats_json)
        }
      }
    }
  }
}

fn log_request(request: &reqwest::Request, stats_json: bool) {
  let mut message = String::new();
  write!(message, "{}", ">>>".bold().green()).unwrap();
  write!(message, " {} {},", "URL:".bold(), request.url()).unwrap();
  write!(message, " {} {},", "METHOD:".bold(), request.method()).unwrap();
  write!(message, " {} {:?}", "HEADERS:".bold(), request.headers()).unwrap();
  if stats_json {
    eprintln!("{message}");
  } else {
    println!("{message}");
  }
}

fn log_message_response(response: &Option<ResponseData>, metrics: &RequestMetrics) -> String {
  let mut message = String::new();
  match response {
    Some(response) => {
      write!(message, " {} {},", "URL:".bold(), response.url).unwrap();
      write!(message, " {} {},", "STATUS:".bold(), response.status).unwrap();
      write!(message, " {} {:?}", "HEADERS:".bold(), response.headers).unwrap();
      write!(message, " {} {:.4} ms,", "DURATION:".bold(), metrics.time_total_ms).unwrap();
      write!(message, " {} {:.4} ms,", "TTFB:".bold(), metrics.time_starttransfer_ms).unwrap();
      write!(message, " {} {} bytes,", "UPLOAD:".bold(), metrics.size_upload).unwrap();
      write!(message, " {} {} bytes", "DOWNLOAD:".bold(), metrics.size_download).unwrap();
    }
    None => {
      message = String::from("No response from server!");
    }
  }
  message
}

fn log_response(log_message_response: String, body: &Option<String>, stats_json: bool) {
  let mut message = String::new();
  write!(message, "{}{}", "<<<".bold().green(), log_message_response).unwrap();
  if let Some(body) = body.as_ref() {
    write!(message, " {} {:?}", "BODY:".bold(), body).unwrap()
  }
  if stats_json {
    eprintln!("{message}");
  } else {
    println!("{message}");
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_yaml::Value as YamlValue;
  use std::io::{Read, Write};
  use std::net::{SocketAddr, TcpListener};
  use std::sync::{Arc, Mutex};
  use std::thread;
  use tempfile::NamedTempFile;

  /// Spawns a bare HTTP/1.1 server that answers `connections` requests with a
  /// fixed JSON body, capturing the raw request head of each one so tests can
  /// assert on the headers/query/auth actually sent.
  fn spawn_mock_server(response_body: &str, connections: usize) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_thread = captured.clone();
    let body = response_body.to_string();

    thread::spawn(move || {
      for _ in 0..connections {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap();
        captured_thread.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).to_string());
        let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        stream.write_all(head.as_bytes()).unwrap();
        stream.flush().unwrap();
      }
    });

    (addr, captured)
  }

  fn send_with_context(request: &Request, context: &mut Context, config: &Config) {
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(request.send_request(context, &pool, config));
  }

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

  fn create_yaml_request_with_urlencoded_body() -> YamlValue {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    urlencoded:
      key1: value1
      key2: "{{ fake.email }}"
"#;
    serde_yaml::from_str(yaml_str).unwrap()
  }

  fn create_yaml_request_with_formdata_body() -> YamlValue {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    formdata:
      - key: field1
        value: text value
      - key: avatar
        file: path/to/image.png
        content_type: image/png
"#;
    serde_yaml::from_str(yaml_str).unwrap()
  }

  fn create_yaml_request_with_graphql_body() -> YamlValue {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    graphql:
      query: "query { user(id: 1) { name } }"
      variables:
        id: "{{ item.id }}"
"#;
    serde_yaml::from_str(yaml_str).unwrap()
  }

  #[test]
  fn parses_urlencoded_body() {
    let yaml = create_yaml_request_with_urlencoded_body();
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::UrlEncoded(params)) => {
        assert_eq!(params.get("key1").map(String::as_str), Some("value1"));
        assert_eq!(params.get("key2").map(String::as_str), Some("{{ fake.email }}"));
        assert_eq!(params.len(), 2);
      }
      _ => panic!("Expected Body::UrlEncoded"),
    }
  }

  #[test]
  fn parses_formdata_body() {
    let yaml = create_yaml_request_with_formdata_body();
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::FormData(parts)) => {
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].key, "field1");
        assert_eq!(parts[0].value.as_deref(), Some("text value"));
        assert!(parts[0].file.is_none());
        assert!(parts[0].content_type.is_none());
        assert_eq!(parts[1].key, "avatar");
        assert!(parts[1].value.is_none());
        assert_eq!(parts[1].file.as_deref(), Some("path/to/image.png"));
        assert_eq!(parts[1].content_type.as_deref(), Some("image/png"));
      }
      _ => panic!("Expected Body::FormData"),
    }
  }

  #[test]
  fn parses_graphql_body() {
    let yaml = create_yaml_request_with_graphql_body();
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::GraphQL {
        query,
        variables,
      }) => {
        assert_eq!(query, "query { user(id: 1) { name } }");
        let variables = variables.expect("expected variables");
        assert_eq!(variables.get("id").map(String::as_str), Some("{{ item.id }}"));
      }
      _ => panic!("Expected Body::GraphQL"),
    }
  }

  #[test]
  fn sends_urlencoded_body() {
    let (addr, captured) = spawn_mock_server("{}", 1);
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{addr}/api\n  method: POST\n  body:\n    urlencoded:\n      key1: value1\n      key2: \"{{{{ email }}}}\"\n")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    context.insert("email".to_string(), json!("user@example.com"));
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let head = captured.lock().unwrap()[0].clone();
    assert!(head.contains("content-type: application/x-www-form-urlencoded"), "missing urlencoded content type in: {head}");
    assert!(head.contains("key1=value1"), "missing key1=value1 in: {head}");
    assert!(head.contains("key2=user%40example.com") || head.contains("key2=user@example.com"), "missing interpolated key2 in: {head}");
  }

  #[test]
  fn sends_formdata_body_with_text_and_file() {
    let mut temp_file = NamedTempFile::new().unwrap();
    temp_file.write_all(b"file-content").unwrap();
    temp_file.flush().unwrap();
    let file_path = temp_file.path().to_str().unwrap();

    let (addr, captured) = spawn_mock_server("{}", 1);
    let yaml: YamlValue = serde_yaml::from_str(&format!(
      "name: test\nrequest:\n  url: http://{addr}/api\n  method: POST\n  body:\n    formdata:\n      - key: field1\n        value: text value\n      - key: avatar\n        file: {file_path}\n        content_type: text/plain\n"
    ))
    .unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let head = captured.lock().unwrap()[0].to_lowercase();
    assert!(head.contains("content-type: multipart/form-data; boundary="), "missing multipart content type in: {head}");
    assert!(head.contains("name=\"field1\""), "missing text field in: {head}");
    assert!(head.contains("text value"), "missing text field value in: {head}");
    assert!(head.contains("name=\"avatar\""), "missing file field in: {head}");
    assert!(head.contains("filename=\""), "missing file name in: {head}");
    assert!(head.contains("content-type: text/plain"), "missing file content type in: {head}");
    assert!(head.contains("file-content"), "missing file content in: {head}");
  }

  #[test]
  fn sends_graphql_body() {
    let (addr, captured) = spawn_mock_server("{}", 1);
    let yaml: YamlValue =
      serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{addr}/api\n  method: POST\n  body:\n    graphql:\n      query: \"query {{ user(id: 1) {{ name }} }}\"\n      variables:\n        id: \"{{{{ item_id }}}}\"\n")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    context.insert("item_id".to_string(), json!("42"));
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let head = captured.lock().unwrap()[0].clone();
    assert!(head.contains("content-type: application/json"), "missing json content type in: {head}");
    assert!(head.contains("{\"query\":\"query { user(id: 1) { name } }\",\"variables\":{\"id\":\"42\"}}"), "missing graphql payload in: {head}");
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
      load_shape: None,
      vars: HashMap::new(),
      threads: 1,
      conn_per_iter: false,
      persist_context: false,
      run_time: 0,
      continue_on_assert_fail: false,
      success_codes: Vec::new(),
      assertion_failures: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
      stats_json: false,
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
    let (response, metrics) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    server.join().unwrap();

    assert!(response.is_some(), "expected a successful response");
    assert!(metrics.time_total_ms >= 250.0, "measured {}ms; expected >= 250ms to include the 300ms body-transfer delay", metrics.time_total_ms);
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
    let (response, _metrics) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    server.join().unwrap();

    let response = response.expect("expected a successful response after draining the body");
    assert_eq!(response.status.as_u16(), 200);
    assert!(response.body.is_none(), "a non-assign body must be drained and dropped, not retained");
  }

  /// Metrics are captured end-to-end: TTFB, total duration, request body size,
  /// response body size and total transfer size are all populated.
  #[test]
  fn captures_request_metrics() {
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let body = r#"{"ok": true}"#;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let mut buf = [0u8; 8192];
      let n = stream.read(&mut buf).unwrap();
      let request = String::from_utf8_lossy(&buf[..n]);
      assert!(request.contains("hello=world"), "request body should contain the urlencoded payload");

      let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
      stream.write_all(head.as_bytes()).unwrap();
      stream.flush().unwrap();
    });

    let url = format!("http://{addr}/echo");
    let yaml: YamlValue = serde_yaml::from_str(&format!(
      r#"name: metrics-test
request:
  url: {url}
  method: POST
  body: hello=world
"#
    ))
    .unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context: Context = Context::new();
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let config = test_config();

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (response, metrics) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    assert!(response.is_some(), "expected a successful response");
    assert!(metrics.time_total_ms > 0.0, "time_total_ms should be recorded");
    assert!(metrics.time_starttransfer_ms > 0.0, "time_starttransfer_ms should be recorded");
    assert!(metrics.time_starttransfer_ms <= metrics.time_total_ms, "TTFB should not exceed total time");
    assert!(metrics.size_upload > 0, "upload size should be recorded for POST body");
    assert!(metrics.size_download > 0, "download size should be recorded for response body");
    assert!(metrics.size_total > metrics.size_upload + metrics.size_download, "size_total should include headers");
  }

  #[test]
  fn parses_apikey_header_auth() {
    let yaml: YamlValue = serde_yaml::from_str(
      r#"
name: test
request:
  url: http://example.com
  auth:
    type: apikey
    key: X-API-Key
    value: "{{ api_key }}"
    in: header
"#,
    )
    .unwrap();
    let request = Request::new(&yaml, None, None);

    match request.auth {
      Some(AuthConfig::ApiKey {
        key,
        value,
        location,
      }) => {
        assert_eq!(key, "X-API-Key");
        assert_eq!(value, "{{ api_key }}");
        assert!(matches!(location, ApiKeyLocation::Header));
      }
      _ => panic!("expected ApiKey auth"),
    }
  }

  #[test]
  fn parses_apikey_query_auth() {
    let yaml: YamlValue = serde_yaml::from_str(
      r#"
name: test
request:
  url: http://example.com
  auth:
    type: apikey
    key: api_key
    value: "{{ api_key }}"
    in: query
"#,
    )
    .unwrap();
    let request = Request::new(&yaml, None, None);

    match request.auth {
      Some(AuthConfig::ApiKey {
        key,
        value,
        location,
      }) => {
        assert_eq!(key, "api_key");
        assert_eq!(value, "{{ api_key }}");
        assert!(matches!(location, ApiKeyLocation::Query));
      }
      _ => panic!("expected ApiKey auth"),
    }
  }

  #[test]
  fn parses_basic_auth() {
    let yaml: YamlValue = serde_yaml::from_str(
      r#"
name: test
request:
  url: http://example.com
  auth:
    type: basic
    username: admin
    password: secret
"#,
    )
    .unwrap();
    let request = Request::new(&yaml, None, None);

    match request.auth {
      Some(AuthConfig::Basic {
        username,
        password,
      }) => {
        assert_eq!(username, "admin");
        assert_eq!(password, "secret");
      }
      _ => panic!("expected Basic auth"),
    }
  }

  #[test]
  fn parses_bearer_auth() {
    let yaml: YamlValue = serde_yaml::from_str(
      r#"
name: test
request:
  url: http://example.com
  auth:
    type: bearer
    token: my-token
"#,
    )
    .unwrap();
    let request = Request::new(&yaml, None, None);

    match request.auth {
      Some(AuthConfig::Bearer {
        token,
      }) => assert_eq!(token, "my-token"),
      _ => panic!("expected Bearer auth"),
    }
  }

  #[test]
  fn parses_oauth2_client_credentials_auth() {
    let yaml: YamlValue = serde_yaml::from_str(
      r#"
name: test
request:
  url: http://example.com
  auth:
    type: oauth2
    flow: client_credentials
    token_url: http://auth.example.com/token
    client_id: my-client
    client_secret: secret
    scope: read write
    save_token_as: access_token
"#,
    )
    .unwrap();
    let request = Request::new(&yaml, None, None);

    match request.auth {
      Some(AuthConfig::OAuth2ClientCredentials {
        token_url,
        client_id,
        client_secret,
        scope,
        save_token_as,
      }) => {
        assert_eq!(token_url, "http://auth.example.com/token");
        assert_eq!(client_id, "my-client");
        assert_eq!(client_secret, "secret");
        assert_eq!(scope.as_deref(), Some("read write"));
        assert_eq!(save_token_as.as_deref(), Some("access_token"));
      }
      _ => panic!("expected OAuth2ClientCredentials auth"),
    }
  }

  #[test]
  fn apikey_header_is_sent() {
    let (addr, captured) = spawn_mock_server("{}", 1);
    let auth_yaml = "    type: apikey\n    key: X-API-Key\n    value: \"{{ api_key }}\"\n    in: header\n";
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{addr}/api/data\n  auth:\n{auth_yaml}")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    context.insert("api_key".to_string(), json!("my-key"));
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let head = captured.lock().unwrap()[0].clone();
    assert!(head.contains("x-api-key: my-key"), "missing api key header in: {head}");
  }

  #[test]
  fn apikey_query_is_appended_to_url() {
    let (addr, captured) = spawn_mock_server("{}", 1);
    let auth_yaml = "    type: apikey\n    key: api_key\n    value: \"{{ api_key }}\"\n    in: query\n";
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{addr}/api/data\n  auth:\n{auth_yaml}")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    context.insert("api_key".to_string(), json!("my-key"));
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let head = captured.lock().unwrap()[0].clone();
    assert!(head.contains("api_key=my-key"), "missing api key query param in: {head}");
  }

  #[test]
  fn basic_auth_header_is_sent() {
    let (addr, captured) = spawn_mock_server("{}", 1);
    let auth_yaml = "    type: basic\n    username: admin\n    password: secret\n";
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{addr}/api/data\n  auth:\n{auth_yaml}")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let head = captured.lock().unwrap()[0].clone();
    assert!(head.contains("authorization: Basic YWRtaW46c2VjcmV0"), "missing basic auth header in: {head}");
  }

  #[test]
  fn bearer_auth_header_is_sent() {
    let (addr, captured) = spawn_mock_server("{}", 1);
    let auth_yaml = "    type: bearer\n    token: my-token\n";
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{addr}/api/data\n  auth:\n{auth_yaml}")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let head = captured.lock().unwrap()[0].clone();
    assert!(head.contains("authorization: Bearer my-token"), "missing bearer auth header in: {head}");
  }

  #[test]
  fn oauth2_client_credentials_acquires_and_sends_token() {
    let (token_addr, token_captured) = spawn_mock_server(r#"{"access_token": "tok-123", "expires_in": 3600}"#, 1);
    let (api_addr, api_captured) = spawn_mock_server("{}", 1);
    let auth_yaml = format!("    type: oauth2\n    flow: client_credentials\n    token_url: http://{token_addr}/oauth/token\n    client_id: my-client\n    client_secret: secret\n    scope: read write\n    save_token_as: access_token\n");
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{api_addr}/api/data\n  auth:\n{auth_yaml}")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    let config = test_config();

    send_with_context(&request, &mut context, &config);

    let token_head = token_captured.lock().unwrap()[0].clone();
    assert!(token_head.contains("grant_type=client_credentials"), "missing grant_type in token request: {token_head}");
    assert!(token_head.contains("client_id=my-client"), "missing client_id in token request: {token_head}");
    assert!(token_head.contains("client_secret=secret"), "missing client_secret in token request: {token_head}");
    assert!(token_head.contains("scope=read+write"), "missing scope in token request: {token_head}");

    let api_head = api_captured.lock().unwrap()[0].clone();
    assert!(api_head.contains("authorization: Bearer tok-123"), "missing bearer token on api request: {api_head}");

    assert_eq!(context.get("access_token"), Some(&json!("tok-123")));
    assert!(context.get("access_token_expires").is_some(), "token expiry must be cached");
  }

  #[test]
  fn oauth2_client_credentials_reuses_cached_token() {
    let (token_addr, token_captured) = spawn_mock_server(r#"{"access_token": "tok-123", "expires_in": 3600}"#, 1);
    let (api_addr, api_captured) = spawn_mock_server("{}", 2);
    let auth_yaml = format!("    type: oauth2\n    flow: client_credentials\n    token_url: http://{token_addr}/oauth/token\n    client_id: my-client\n    client_secret: secret\n    save_token_as: access_token\n");
    let yaml: YamlValue = serde_yaml::from_str(&format!("name: test\nrequest:\n  url: http://{api_addr}/api/data\n  auth:\n{auth_yaml}")).unwrap();
    let request = Request::new(&yaml, None, None);
    let mut context = Context::new();
    let config = test_config();

    send_with_context(&request, &mut context, &config);
    send_with_context(&request, &mut context, &config);

    assert_eq!(token_captured.lock().unwrap().len(), 1, "token endpoint must be hit only once when the token is cached");
    let api_heads = api_captured.lock().unwrap();
    assert_eq!(api_heads.len(), 2);
    assert!(api_heads[0].contains("authorization: Bearer tok-123"), "first request missing bearer token: {}", api_heads[0]);
    assert!(api_heads[1].contains("authorization: Bearer tok-123"), "second request missing bearer token: {}", api_heads[1]);
  }
}
