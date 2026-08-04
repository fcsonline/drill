use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use http::Extensions;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result};

/// Curl-style timing and size metrics for a single HTTP request.
///
/// Some curl metrics depend on transport-level phases (DNS, TCP connect, TLS
/// handshake) that reqwest does not expose through its public API. Those fields
/// are kept as curl-compatible placeholders and always `None`.
#[derive(Debug, Clone, Default)]
pub struct RequestMetrics {
    /// Time from start until the first byte of the response is received.
    /// Equivalent to curl's `time_starttransfer`.
    pub time_starttransfer_ms: f64,
    /// Time from start until the entire response body is received.
    /// Equivalent to curl's `time_total`.
    pub time_total_ms: f64,
    /// Approximate total request bytes sent (status line + headers + body).
    /// Equivalent to curl's `size_request`.
    pub size_request: u64,
    /// Request body bytes sent. Equivalent to curl's `size_upload`.
    pub size_upload: u64,
    /// Response body bytes received. Equivalent to curl's `size_download`.
    pub size_download: u64,
    /// Request header bytes (excluding body).
    pub size_header_request: u64,
    /// Response header bytes (excluding body).
    pub size_header_response: u64,
    /// Total bytes transferred (request + response).
    pub size_total: u64,
    /// Transport-level phase timings that reqwest does not expose.
    #[allow(dead_code)]
    pub time_namelookup_ms: Option<f64>,
    #[allow(dead_code)]
    pub time_connect_ms: Option<f64>,
    #[allow(dead_code)]
    pub time_appconnect_ms: Option<f64>,
    #[allow(dead_code)]
    pub time_pretransfer_ms: Option<f64>,
    #[allow(dead_code)]
    pub time_redirect_ms: Option<f64>,
}

/// Middleware that records curl-style timing and size metrics.
///
/// Metrics are written into a `RequestMetrics` value shared through the request
/// [`Extensions`]. The caller should create an `Arc<Mutex<RequestMetrics>>`,
/// insert it into extensions before the request, and read it afterwards.
#[derive(Clone, Default)]
pub struct MetricsMiddleware;

impl MetricsMiddleware {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Middleware for MetricsMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response> {
        let metrics = extensions
            .get::<Arc<Mutex<RequestMetrics>>>()
            .cloned()
            .unwrap_or_else(|| Arc::new(Mutex::new(RequestMetrics::default())));

        let start = Instant::now();

        let request_header_size = header_size(req.headers());
        let request_body_size = body_size(req.body()).unwrap_or(0);
        let size_request = request_header_size + request_body_size;

        {
            let mut m = metrics.lock().unwrap();
            m.size_header_request = request_header_size;
            m.size_upload = request_body_size;
            m.size_request = size_request;
        }

        let response = match next.run(req, extensions).await {
            Ok(response) => response,
            Err(e) => {
                let mut m = metrics.lock().unwrap();
                m.time_total_ms = start.elapsed().as_secs_f64() * 1000.0;
                return Err(e);
            }
        };

        let time_starttransfer_ms = start.elapsed().as_secs_f64() * 1000.0;
        {
            let mut m = metrics.lock().unwrap();
            m.time_starttransfer_ms = time_starttransfer_ms;
        }

        Ok(response)
    }
}

fn header_size(headers: &http::HeaderMap) -> u64 {
    headers
        .iter()
        .map(|(name, value)| {
            let name_len = name.as_str().len() as u64;
            let value_len = value.as_bytes().len() as u64;
            name_len + value_len + 4
        })
        .sum()
}

fn body_size(body: Option<&reqwest::Body>) -> Option<u64> {
    use http_body::Body;
    body.and_then(|b| Body::size_hint(b).exact())
}

pub fn new_metrics() -> Arc<Mutex<RequestMetrics>> {
    Arc::new(Mutex::new(RequestMetrics::default()))
}
