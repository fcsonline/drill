use crate::model::{Request, Url, Header, Body, Auth};
use crate::convert::variables::VariableContext;
use crate::warnings::WarningCollector;
use crate::model::drill::{DrillRequest, DrillBody, DrillAuth, FormDataPart as DrillFormDataPart};
use std::collections::HashMap;
use anyhow::Result;

pub fn convert_request(
    req: Request,
    var_ctx: &VariableContext,
    inherited_auth: Option<&Auth>,
    warnings: &mut WarningCollector,
    location: &str,
) -> Result<DrillRequest> {
    let url = build_url(&req.url, var_ctx)?;
    let method = req.method.map(|m| m.to_uppercase());
    let headers = build_headers(&req.header, var_ctx);
    let body = req.body.map(|b| convert_body(b, var_ctx, warnings, location)).transpose()?;
    let auth = convert_auth(req.auth.as_ref().or(inherited_auth), var_ctx, warnings, location)?;

    Ok(DrillRequest {
        url,
        method,
        headers: if headers.is_empty() { None } else { Some(headers) },
        body,
        auth,
        with_items: None,
        with_items_range: None,
        with_items_from_csv: None,
        shuffle: None,
        pick: None,
    })
}

fn build_url(url: &Url, var_ctx: &VariableContext) -> Result<String> {
    // Prefer raw URL if present
    if let Some(raw) = &url.raw {
        let interpolated = var_ctx.interpolate(raw);
        // Normalize {{var}} to {{ var }}
        return Ok(normalize_interpolations(&interpolated));
    }

    // Build from components
    let protocol = url.protocol.as_deref().unwrap_or("http");
    let host = url.host.join(".");
    let path = if url.path.is_empty() {
        String::new()
    } else {
        format!("/{}", url.path.join("/"))
    };

    let mut query = String::new();
    if !url.query.is_empty() {
        let params: Vec<String> = url.query
            .iter()
            .filter(|q| q.disabled != Some(true))
            .map(|q| {
                let key = var_ctx.interpolate(&q.key);
                let value = q.value.as_ref().map(|v| var_ctx.interpolate(v)).unwrap_or_default();
                if value.is_empty() {
                    key
                } else {
                    format!("{}={}", key, value)
                }
            })
            .collect();
        if !params.is_empty() {
            query = format!("?{}", params.join("&"));
        }
    }

    let full = format!("{}://{}{}{}", protocol, host, path, query);
    Ok(normalize_interpolations(&full))
}

fn build_headers(headers: &[Header], var_ctx: &VariableContext) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for h in headers {
        if h.disabled == Some(true) { continue; }
        let key = normalize_interpolations(&var_ctx.interpolate(&h.key));
        let value = normalize_interpolations(&var_ctx.interpolate(&h.value));
        map.insert(key, value);
    }
    map
}

pub fn normalize_interpolations(s: &str) -> String {
    // Convert {{var}} to {{ var }} for Drill compatibility
    let re = regex::Regex::new(r"\{\{([^}\s]+)\}\}").unwrap();
    re.replace_all(s, |caps: &regex::Captures| {
        format!("{{{{ {} }}}}", &caps[1])
    }).to_string()
}

pub fn convert_body(
    body: Body,
    var_ctx: &VariableContext,
    warnings: &mut WarningCollector,
    location: &str,
) -> Result<DrillBody> {
    match body {
        Body::Raw { raw, options } => {
            let interpolated = var_ctx.interpolate(&raw);
            let normalized = normalize_interpolations(&interpolated);
            
            // Check if it's GraphQL
            let is_graphql = options.as_ref().and_then(|o| o.content_type.as_ref()).map(|ct| ct.contains("graphql")).unwrap_or(false);
            if !is_graphql {
                return Ok(DrillBody::Template(normalized));
            }

            // Try to parse as GraphQL
            let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&normalized) else {
                return Ok(DrillBody::Template(normalized));
            };
            let Some(query) = json_val.get("query").and_then(|v| v.as_str()) else {
                return Ok(DrillBody::Template(normalized));
            };
            let variables = json_val.get("variables").and_then(|v| v.as_object()).map(|obj| {
                let mut map = HashMap::new();
                for (k, v) in obj {
                    map.insert(k.clone(), v.as_str().unwrap_or("").to_string());
                }
                map
            });
            Ok(DrillBody::GraphQL {
                query: query.to_string(),
                variables,
            })
        }
        Body::UrlEncoded { urlencoded } => {
            let mut map = HashMap::new();
            for kv in urlencoded {
                let key = normalize_interpolations(&var_ctx.interpolate(&kv.key));
                let value = var_ctx.interpolate(&serde_json::to_string(&kv.value).unwrap_or_default());
                // Strip quotes if it's a JSON string
                let value = normalize_interpolations(value.trim_matches('"'));
                map.insert(key, value);
            }
            Ok(DrillBody::UrlEncoded(map))
        }
        Body::FormData { formdata } => {
            let mut parts = Vec::new();
            for part in formdata {
                if part.disabled == Some(true) { continue; }
                let key = normalize_interpolations(&var_ctx.interpolate(&part.key));
                let value = part.value.as_ref().map(|v| normalize_interpolations(&var_ctx.interpolate(v)));
                let file = part.src.as_ref().map(|v| normalize_interpolations(&var_ctx.interpolate(v)));
                let content_type = part.content_type.as_ref().map(|v| normalize_interpolations(&var_ctx.interpolate(v)));
                parts.push(DrillFormDataPart { key, value, file, content_type });
            }
            Ok(DrillBody::FormData(parts))
        }
        Body::File { file } => {
            let src = normalize_interpolations(&var_ctx.interpolate(&file.src));
            warnings.warn(location, format!("File body path '{}' may not exist in target environment", src));
            Ok(DrillBody::BinaryFile { file: src })
        }
        Body::GraphQL { graphql } => {
            let query = normalize_interpolations(&var_ctx.interpolate(&graphql.query));
            let variables = graphql.variables.as_ref().and_then(|v| {
                if let Some(obj) = v.as_object() {
                    let mut map = HashMap::new();
                    for (k, val) in obj {
                        map.insert(k.clone(), normalize_interpolations(&var_ctx.interpolate(val.as_str().unwrap_or(""))));
                    }
                    Some(map)
                } else {
                    None
                }
            });
            Ok(DrillBody::GraphQL { query, variables })
        }
    }
}

pub fn convert_auth(
    auth: Option<&Auth>,
    var_ctx: &VariableContext,
    warnings: &mut WarningCollector,
    location: &str,
) -> Result<Option<DrillAuth>> {
    let Some(auth) = auth else { return Ok(None) };
    
    match auth.r#type.as_str() {
        "basic" => {
            let mut username = String::new();
            let mut password = String::new();
            if let Some(params) = &auth.basic {
                for kv in params {
                    match kv.key.as_str() {
                        "username" => username = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        "password" => password = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        _ => {}
                    }
                }
            }
            Ok(Some(DrillAuth {
                auth_type: "basic".to_string(),
                username: Some(username),
                password: Some(password),
                key: None, value: None, location: None, token: None,
                flow: None, token_url: None, client_id: None, client_secret: None,
                scope: None, save_token_as: None,
            }))
        }
        "bearer" => {
            let mut token = String::new();
            if let Some(params) = &auth.bearer {
                for kv in params {
                    if kv.key == "token" {
                        token = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or("")));
                    }
                }
            }
            Ok(Some(DrillAuth {
                auth_type: "bearer".to_string(),
                token: Some(token),
                key: None, value: None, location: None, username: None, password: None,
                flow: None, token_url: None, client_id: None, client_secret: None,
                scope: None, save_token_as: None,
            }))
        }
        "apikey" => {
            let mut key = String::new();
            let mut value = String::new();
            let mut location = "header".to_string();
            if let Some(params) = &auth.apikey {
                for kv in params {
                    match kv.key.as_str() {
                        "key" => key = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        "value" => value = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        "in" => location = kv.value.as_str().unwrap_or("header").to_string(),
                        _ => {}
                    }
                }
            }
            Ok(Some(DrillAuth {
                auth_type: "apikey".to_string(),
                key: Some(key),
                value: Some(value),
                location: Some(location),
                username: None, password: None, token: None,
                flow: None, token_url: None, client_id: None, client_secret: None,
                scope: None, save_token_as: None,
            }))
        }
        "oauth2" => {
            let mut token_url = String::new();
            let mut client_id = String::new();
            let mut client_secret = String::new();
            let mut scope = String::new();
            if let Some(params) = &auth.oauth2 {
                for kv in params {
                    match kv.key.as_str() {
                        "accessTokenUrl" | "tokenUrl" => token_url = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        "clientId" => client_id = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        "clientSecret" => client_secret = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        "scope" => scope = normalize_interpolations(&var_ctx.interpolate(kv.value.as_str().unwrap_or(""))),
                        _ => {}
                    }
                }
            }
            // Only support client_credentials flow
            warnings.warn(location, "OAuth2: only client_credentials flow supported; other flows require manual setup");
            Ok(Some(DrillAuth {
                auth_type: "oauth2".to_string(),
                flow: Some("client_credentials".to_string()),
                token_url: Some(token_url),
                client_id: Some(client_id),
                client_secret: Some(client_secret),
                scope: if scope.is_empty() { None } else { Some(scope) },
                save_token_as: Some("access_token".to_string()),
                key: None, value: None, location: None, username: None, password: None, token: None,
            }))
        }
        "digest" | "hawk" | "awsv4" | "ntlm" | "oauth1" => {
            warnings.warn(location, format!("Auth type '{}' not supported in Drill; manual header construction required", auth.r#type));
            Ok(None)
        }
        "noauth" | "none" => Ok(None),
        _ => {
            warnings.warn(location, format!("Unknown auth type '{}'; skipping", auth.r#type));
            Ok(None)
        }
    }
}