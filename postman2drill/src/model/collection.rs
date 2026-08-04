// Postman Collection v2.1 structures
// Fields mirror Postman JSON keys, so camelCase names are intentional.
#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Collection {
    pub info: Info,
    #[serde(default)]
    pub variable: Vec<Variable>,
    #[serde(default)]
    pub auth: Option<Auth>,
    #[serde(default)]
    pub event: Vec<Event>,
    pub item: Vec<Item>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Info {
    pub name: String,
    #[serde(rename = "_postman_id")]
    pub postman_id: Option<String>,
    pub schema: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Variable {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Auth {
    pub r#type: String,
    #[serde(default)]
    pub basic: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub bearer: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub apikey: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub oauth2: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub digest: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub hawk: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub awsv4: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub ntlm: Option<Vec<KeyValue>>,
    #[serde(default)]
    pub oauth1: Option<Vec<KeyValue>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KeyValue {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Event {
    pub listen: String, // "prerequest" or "test"
    pub script: Script,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Script {
    #[serde(default)]
    pub r#type: Option<String>,
    pub exec: Vec<String>, // lines of JavaScript
    #[serde(default)]
    pub src: Option<String>, // alternative: single string
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Item {
    Request(RequestItem),
    Folder(FolderItem),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestItem {
    pub name: String,
    pub request: Request,
    #[serde(default)]
    pub response: Vec<Response>,
    #[serde(default)]
    pub event: Vec<Event>,
    #[serde(default)]
    pub protocolProfileBehavior: Option<ProtocolProfileBehavior>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FolderItem {
    pub name: String,
    #[serde(default)]
    pub item: Vec<Item>,
    #[serde(default)]
    pub event: Vec<Event>,
    #[serde(default)]
    pub auth: Option<Auth>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Request {
    pub method: Option<String>,
    pub url: Url,
    #[serde(default)]
    pub header: Vec<Header>,
    #[serde(default)]
    pub body: Option<Body>,
    #[serde(default)]
    pub auth: Option<Auth>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Url {
    pub raw: Option<String>,
    pub protocol: Option<String>,
    #[serde(default)]
    pub host: Vec<String>,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(default)]
    pub query: Vec<QueryParam>,
    #[serde(default)]
    pub variable: Vec<Variable>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryParam {
    pub key: String,
    pub value: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Header {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Body {
    Raw { raw: String, options: Option<RawBodyOptions> },
    UrlEncoded { urlencoded: Vec<KeyValue> },
    FormData { formdata: Vec<FormDataPart> },
    File { file: FileBody },
    GraphQL { graphql: GraphQLBody },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawBodyOptions {
    #[serde(rename = "content-type")]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FormDataPart {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>, // "text" or "file"
    #[serde(default)]
    pub src: Option<String>, // file path for type=file
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileBody {
    pub src: String,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GraphQLBody {
    pub query: String,
    #[serde(default)]
    pub variables: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Response {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub originalRequest: Option<Request>,
    pub status: String,
    pub code: u16,
    #[serde(default)]
    pub _postman_previewlanguage: Option<String>,
    #[serde(default)]
    pub header: Vec<Header>,
    #[serde(default)]
    pub cookie: Vec<Cookie>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub expires: Option<String>,
    #[serde(default)]
    pub httpOnly: Option<bool>,
    #[serde(default)]
    pub secure: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtocolProfileBehavior {
    #[serde(default)]
    pub disableBodyPruning: Option<bool>,
    #[serde(default)]
    pub followRedirects: Option<bool>,
    #[serde(default)]
    pub followOriginalHttpMethod: Option<bool>,
}

// Postman Environment v2.1

#[derive(Debug, Deserialize, Serialize)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub values: Vec<EnvValue>,
    #[serde(default)]
    pub _postman_variable_scope: Option<String>,
    #[serde(default)]
    pub _postman_exported_at: Option<String>,
    #[serde(default)]
    pub _postman_exported_using: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EnvValue {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub description: String,
}