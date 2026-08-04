# Postman → Drill Converter Tool Plan

## Overview
Standalone CLI tool (`postman2drill`) that reads a Postman Collection v2.1 JSON file (+ optional environment file) and emits a Drill YAML benchmark file + a warnings report.

## Scope
- Input: Postman Collection v2.1 (`schema: "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"`)
- Optional input: Postman Environment file (v2.1)
- Output: Drill YAML benchmark file
- Output: Warnings report (JSON or text) listing unsupported/partially-converted constructs

## Architecture

### Crate Structure
```
postman2drill/
├── Cargo.toml
├── src/
│   ├── main.rs           # CLI entry point
│   ├── model/
│   │   ├── mod.rs
│   │   ├── collection.rs # Postman collection structures
│   │   ├── environment.rs
│   │   └── drill.rs      # Drill output structures
│   ├── convert/
│   │   ├── mod.rs
│   │   ├── request.rs    # Request → Drill request
│   │   ├── body.rs       # Body conversion
│   │   ├── auth.rs       # Auth conversion
│   │   ├── scripts.rs    # Pre-request/test scripts → save/assert
│   │   ├── variables.rs  # Variable resolution
│   │   └── assertions.rs # Chai/pm.* → Drill assertions
│   └── warnings.rs       # Warning collection & reporting
```

### Dependencies
- `serde_json` — parse Postman JSON
- `serde_yaml` — emit Drill YAML
- `clap` — CLI
- `jsonpath_lib` — for script assertion translation (same as Drill)
- `regex` — for script pattern matching

## Conversion Rules

### 1. Collection → Drill Config
| Postman | Drill |
|---|---|
| `collection.info.name` | Top-level comment |
| `collection.variable[]` | `vars:` map (string values) |
| `collection.auth` | Default auth for all requests (inherited) |
| `collection.event[]` (pre-request/test) | Global `lifecycle.setup` / `lifecycle.iteration_start` with warnings |

### 2. Items (Folders/Requests) → Plan
- Folder → sequential sub-plan (no Drill folder concept, just sequence)
- Request → Drill `request` item
- Order preserved

### 3. Request Conversion
| Postman Request | Drill Request |
|---|---|
| `method` | `method` |
| `url.raw` + `url.path` + `url.query` | `url` (interpolated) |
| `header[]` (enabled) | `headers:` map |
| `body` (mode) | `body:` per B1 modes |
| `auth` (request-level overrides collection) | `auth:` block |
| `event[]` pre-request | `save`/`assign` before request (with warnings) |
| `event[]` test | `assert`/`save` after request (with warnings) |

#### URL Construction
- `url.raw` preferred if present; else build from `protocol` + `host` + `path` + `query`
- Postman `{{var}}` → Drill `{{ var }}` (strip `$` if present)

#### Body Modes
| Postman mode | Drill body type |
|---|---|
| `raw` (JSON/text/XML) | `body: "string"` or `body: { graphql: ... }` if GraphQL detected |
| `urlencoded` | `body: { urlencoded: { key: val } }` |
| `formdata` | `body: { formdata: [ { key, value? file? content_type? } ] }` |
| `file` | `body: { file: "path" }` (warn: path may not exist) |
| `graphql` | `body: { graphql: { query, variables } }` |

#### Auth Types
| Postman auth | Drill auth |
|---|---|
| `noauth` | (omit) |
| `basic` | `auth: { type: basic, username, password }` |
| `bearer` | `auth: { type: bearer, token }` |
| `apikey` | `auth: { type: apikey, key, value, in: header|query }` |
| `oauth2` (client_credentials) | `auth: { type: oauth2, flow: client_credentials, token_url, client_id, client_secret, scope, save_token_as }` |
| `digest`, `hawk`, `awsv4`, `ntlm`, `oauth1` | Warn + skip (emit commented placeholder) |

### 4. Script Conversion (The Hard Part)

#### Pre-request Scripts
Common patterns to detect and convert:
```js
pm.environment.set("key", "value")          → assign: { key, value }
pm.collectionVariables.set("key", "value")  → assign: { key, value }
pm.variables.set("key", "value")            → assign: { key, value }
pm.globals.set("key", "value")              → warn (globals not supported)
```
- `pm.sendRequest()` → **warn: unsupported**
- `postman.setNextRequest()` → **warn: unsupported**
- Arbitrary JS → **warn: cannot convert, manual review needed**

#### Test Scripts (Post-response)
```js
pm.test("name", () => { ... })
pm.expect(...).to.eql(...)
pm.response.to.have.status(200)
pm.response.to.have.header("Content-Type", "application/json")
pm.response.json().data.id
pm.response.jsonPath("$.data.id")
pm.collectionVariables.set("token", pm.response.json().token)
pm.environment.set("id", pm.response.jsonPath("$.id"))
pm.response.responseTime
```
Convert to:
- `assert: { type: status, value: 200 }`
- `assert: { type: header, key: "content-type", value: "application/json" }`
- `assert: { type: jsonpath, key: "$.data.id", value: "123" }`
- `assert: { type: duration, value: 500, operator: "lt" }`
- `save: { source: response_body, jsonpath: "$.token", key: "token" }`
- `assign: { key: "id", value: "..." }` (for env vars)

Heuristics: parse AST-lite via regex for common Chai/pm.* patterns. Emit warning for each unrecognized line.

### 5. Variable Resolution
- Collection variables → `vars:`
- Environment file values → merged into `vars:` (env overrides collection)
- Dynamic variables (`$guid`, `$timestamp`, etc.) → preserved as `{{ $guid }}` etc. (Drill B2 supports them)
- `{{var}}` in strings → `{{ var }}` (space normalization)

### 6. Warnings Report
Each warning entry:
```json
{
  "level": "warn|error|info",
  "location": "collection.auth / item[3].request.body / item[5].event[0].script.line[2]",
  "message": "Digest auth not supported; manual header required",
  "original": "...original Postman construct..."
}
```
Output as JSON (default) or human-readable text.

## CLI Interface
```
postman2drill [OPTIONS] <collection.json> [environment.json]

OPTIONS:
  -o, --output <FILE>        Output Drill YAML file (default: stdout)
  -w, --warnings <FILE>      Warnings report file (default: stderr)
  -f, --format <json|text>   Warnings format (default: json)
  --strict                   Treat warnings as errors (exit non-zero)
  -h, --help                 Print help
  -V, --version              Print version
```

## Implementation Phases

### Phase 1: Core Parsing & Structures
1. Postman collection/environment models (serde deserialize)
2. Drill output models (serde serialize to YAML)
3. Basic CLI skeleton

### Phase 2: Request Conversion
1. URL building
2. Headers
3. Body modes (all 5)
4. Auth types

### Phase 3: Script Conversion
1. Pre-request → assign/save
2. Test scripts → assert/save
3. Pattern matching for common Chai/pm.* assertions

### Phase 4: Variable Handling & Inheritance
1. Collection vars + environment merge
2. Auth inheritance (collection → folder → request)
3. Variable interpolation normalization

### Phase 5: Warnings & Polish
1. Warning collection throughout
2. Report output
3. Edge case handling
4. Tests with real Postman collections

## Testing
- Sample Postman collections covering:
  - Basic CRUD with auth
  - Form data / file upload
  - OAuth2 flow
  - Chained requests (token extraction)
  - Pre-request scripts (env manipulation)
  - Test scripts (status, header, jsonpath, response time assertions)
- Golden file tests: input collection + env → expected Drill YAML + warnings

## Out of Scope (explicit warnings)
- `pm.sendRequest()`, `postman.setNextRequest()`
- Digest, Hawk, AWS SigV4, NTLM, OAuth1 auth
- `pm.visualizer`, `pm.cookies` advanced
- Arbitrary JavaScript in scripts
- GraphQL introspection, `pm.request` manipulation
- Data file iteration (CSV/JSON) → map to `for_each` with warning

## File Placement
Create as new crate in repo root: `/Users/sandeepgoje/Work/repos/src/github.com/sgmovea2z/drill/postman2drill/`