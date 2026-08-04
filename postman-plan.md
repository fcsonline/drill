# Drill: Postman Collection Conversion Support Plan (B1–B5)

This plan covers the Drill core features needed so a converter can emit faithful Drill YAML from a Postman collection v2.1. Scope: single-process load testing. Converter itself is out of scope here (separate tool later).

---

## B1. Body modes — native YAML support for Postman body types

**Problem**: Drill currently only supports raw string (`Template`), binary `hex`, and `file`. Postman collections use `urlencoded`, `formdata` (multipart), `graphql`, and `file` (different semantics).

**Changes**:
1. **Extend `Body` enum** in `src/actions/request.rs`:
   ```rust
   pub enum Body {
     Template(String),
     Binary(Vec<u8>),
     UrlEncoded(HashMap<String, String>),   // NEW
     FormData(Vec<FormPart>),                // NEW
     GraphQL { query: String, variables: Option<HashMap<String, String>> }, // NEW
   }
   ```
   `FormPart` = text field | file field (with `file_path`, `content_type` optional).

2. **Parse new YAML structures** in `Request::new()`:
   ```yaml
   # application/x-www-form-urlencoded
   body:
     urlencoded:
       key1: "value1"
       key2: "{{ fake.email }}"
   ```
   ```yaml
   # multipart/form-data
   body:
     formdata:
       - key: "field1"
         value: "text"
       - key: "avatar"
         file: "path/to/image.png"
         content_type: "image/png"  # optional
   ```
   ```yaml
   # graphql
   body:
     graphql:
       query: "query { user(id: 1) { name } }"
       variables:
         id: "{{ item.id }}"
   ```

3. **Build reqwest multipart/form-data** in `send_request()` using `reqwest::multipart::Form` for `FormData`; for `UrlEncoded` set `Content-Type: application/x-www-form-urlencoded` and serialize as `key=value&...`; for `GraphQL` serialize JSON `{"query":..., "variables":...}` with `Content-Type: application/json`.

4. **Tests**: add unit tests for each new body variant (parsing + request building).

**Files**: `src/actions/request.rs`, `src/actions/request.rs` tests.

---

## B2. Dynamic variables — map Postman `$vars` to Drill built-ins

**Problem**: Postman provides dynamic variables (`$guid`, `$timestamp`, `$randomInt`, etc.) that users expect to work. Drill has `{{ fake.* }}` but no 1:1 mapping for all Postman vars.

**Changes**:
1. **Add built-in dynamic variable resolution** in `src/interpolator.rs`:
   - New resolver function `resolve_dynamic(capture: &str) -> Option<String>` called before `resolve_faker`.
   - Recognize Postman dynamic variable names (with or without `$` prefix):
     | Postman var | Drill resolution |
     |---|---|
     | `$guid` / `$randomUUID` | `fake.uuid` (add to faker) |
     | `$timestamp` | `SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string()` |
     | `$isoTimestamp` | `chrono::Utc::now().to_rfc3339()` |
     | `$randomInt(min,max)` | random integer in range (parse args) |
     | `$randomFloat(min,max)` | random float in range |
     | `$randomFirstName` | `fake.first_name` |
     | `$randomLastName` | `fake.last_name` |
     | `$randomFullName` | `fake.name` |
     | `$randomEmail` | `fake.email` |
     | `$randomPhoneNumber` | `fake.phone` |
     | `$randomCity` | `fake.city` |
     | `$randomStreetAddress` | `fake.street` |
     | `$randomCountry` | `fake.country` |
     | `$randomIp` | `fake.ip` |
     | `$randomHex(length?)` | random hex string |
     | `$randomAlphaNumeric(length?)` | random alphanumeric |
     | `$randomBoolean` | `fake.boolean` |

2. **Add `uuid` to faker** in `src/faker.rs` (use `fake::faker::internet::raw::Uuid` if available, or generate via `rand`).

3. **Add `chrono` dependency** for ISO timestamp (already in transitive deps? check; if not, add).

4. **Tests**: unit tests for each dynamic variable in `interpolator.rs` tests.

**Files**: `src/interpolator.rs`, `src/faker.rs`, `Cargo.toml` (if chrono needed).

---

## B3. Save action — extract values from response into context

**Problem**: Postman scripts do `pm.collectionVariables.set("token", pm.response.json().token)` to chain requests. Drill has `assign` but it only writes literal/interpolated values, not response data.

**Changes**:
1. **Store last response in context** in `src/benchmark.rs` (inside `run_iteration` after each `Request::execute`):
   - Add `response_body`, `response_headers`, `response_status`, `response_url` to context under a reserved key (e.g., `_last_response`).
   - Or store in thread-local / pass through `Context` in `Request::execute` (cleaner: add to context at end of `Request::execute`).

2. **Add `Save` action** in `src/actions/save.rs` (new file) + register in `src/actions/mod.rs` + `src/expandable/include.rs`:
   ```yaml
   - name: Save auth token
     save:
       source: response        # or "response_body", "response_headers", "response_status"
       jsonpath: "$.token"     # JSONPath expression (use `jsonpath_lib` crate)
       key: auth_token         # context key to store result
   ```
   - If `jsonpath` omitted and `source: response_body`, store entire body as string.
   - If `jsonpath` returns array, store array; if single value, store that value.

3. **Add `jsonpath_lib` dependency** (or `serde_json::Value` pointer for simple paths; JSONPath is more powerful — use `jsonpath` crate).

4. **Tests**: unit test for Save action with various JSONPath expressions.

**Files**: `src/actions/save.rs` (new), `src/actions/mod.rs`, `src/expandable/include.rs`, `src/benchmark.rs`, `Cargo.toml`.

---

## B4. Auth helpers — native support for common Postman auth types

**Problem**: Postman collections declare auth at collection/folder/request level. Converter must emit Drill YAML that reproduces the auth behavior without manual header construction.

**Changes**:
1. **Add `auth` field to `Request` struct** in `src/actions/request.rs`:
   ```rust
   pub struct Request {
     // ... existing fields
     pub auth: Option<AuthConfig>,
   }
   ```

2. **Parse `auth` block** in `Request::new()`:
   ```yaml
   auth:
     type: "bearer"        # or "basic", "apikey", "oauth2"
     # type-specific fields
   ```

3. **Implement auth types**:
   - `basic`: already works via `Authorization: Basic {{ base64(user:pass) }}` header — just document.
   - `bearer`: `Authorization: Bearer {{ token }}` — document.
   - `apikey`:
     ```yaml
     auth:
       type: "apikey"
       key: "X-API-Key"
       value: "{{ api_key }}"
       in: "header"  # or "query"
     ```
     Adds header or query param `key=value`.
   - `oauth2` (client credentials flow — minimum for converter):
     ```yaml
     auth:
       type: "oauth2"
       flow: "client_credentials"
       token_url: "https://auth.example.com/oauth/token"
       client_id: "{{ client_id }}"
       client_secret: "{{ client_secret }}"
       scope: "api"
       # optional: save token to context key for reuse
       save_token_as: "access_token"
     ```
     Implementation: in `Request::execute`, before sending, if `save_token_as` is set and token not in context (or expired), POST to `token_url` with client credentials, parse `access_token` from response, store in context, then use as Bearer token.

4. **Tests**: unit tests for each auth type (mock HTTP for oauth2).

**Files**: `src/actions/request.rs`, maybe new `src/auth.rs` for oauth2 token logic.

---

## B5. Extended assertions — status, header, jsonpath, response-time

**Problem**: Postman tests use `pm.response.to.have.status(200)`, `pm.response.to.have.header("Content-Type")`, `pm.expect(pm.response.jsonPath("$.id")).to.eql("123")`, `pm.expect(pm.response.responseTime).to.be.below(500)`. Drill `assert` only does context key/value equality.

**Changes**:
1. **Extend `Assert` struct** in `src/actions/assert.rs` with `assert_type` enum:
   ```rust
   enum AssertType {
     Equals,      // existing: key == value in context
     Status,      // response status code
     Header,      // response header
     JsonPath,    // JSONPath on response body
     Duration,    // response time in ms
   }
   ```

2. **Parse new YAML formats**:
   ```yaml
   - assert:
       type: status
       value: 200              # or [200, 201] for multiple
   ```
   ```yaml
   - assert:
       type: header
       key: "content-type"
       value: "application/json"  # substring match
   ```
   ```yaml
   - assert:
       type: jsonpath
       key: "$.data.id"
       value: "123"
   ```
   ```yaml
   - assert:
       type: duration
       value: 500              # max ms
       operator: "lt"          # lt, lte, gt, gte, eq (default lt)
   ```

3. **Execute using last response** (from B3 context storage) — read `_last_response` from context.

4. **Tests**: unit tests for each assert type.

**Files**: `src/actions/assert.rs`, `src/benchmark.rs` (ensure response in context).

---

## Implementation Order & Dependencies

| Step | Task | Depends On |
|------|------|------------|
| 1 | B3: Store response in context + Save action | — |
| 2 | B5: Extended assertions (uses B3 response context) | B3 |
| 3 | B1: Body modes (independent) | — |
| 4 | B2: Dynamic variables (independent) | — |
| 5 | B4: Auth helpers (uses B3 for oauth2 token save) | B3 |

**Recommended sequence**: B3 → B5 → B1 → B2 → B4 (B3/B5 share response-context work; B1/B2 are independent; B4 uses B3 for token caching).

---

## Acceptance Criteria (per feature)

- **B1**: `cargo test` passes; manual YAML with each body type sends correct `Content-Type` and body; `multipart` uploads file.
- **B2**: `cargo test` passes; `{{ $guid }}`, `{{ $timestamp }}`, `{{ $randomInt(1,100) }}`, etc. interpolate correctly; locale `fake.*` still works.
- **B3**: `save` action extracts JSONPath from prior response into context; subsequent request uses `{{ saved_key }}`.
- **B4**: `auth: {type: apikey, ...}` adds header/query; `auth: {type: oauth2, flow: client_credentials, ...}` fetches token on first use, caches, reuses; token auto-refresh on 401 optional stretch.
- **B5**: `assert: {type: status, value: 200}`, `type: header`, `type: jsonpath`, `type: duration` all work and panic on mismatch.

---

## Verification

After each feature:
- `cargo test` — all 93+ tests pass
- `cargo clippy --all-targets` — clean
- Add at least 1 integration-style test in `tests/` or module `#[cfg(test)]` covering the new feature end-to-end.

---

## Documentation Updates

Update `SYNTAX.md` with new sections:
- **Body modes** (B1)
- **Dynamic variables** (B2) — table mapping Postman `$vars`
- **Save action** (B3)
- **Auth helpers** (B4)
- **Extended assertions** (B5)

---

## Out of Scope (for converter to handle with warnings)

- `pm.sendRequest`, `postman.setNextRequest` — control flow, unsupported.
- Arbitrary JavaScript in pre-request/test scripts — converter emits subset (status/header/jsonpath asserts, env set) + warns.
- Digest, Hawk, NTLM, AWS SigV4, OAuth1 — skip with warning; user adds manually.
- Cookie jar persistence across iterations — Drill has basic cookie support via context.
- Data file iteration (CSV/JSON) — map to `for_each` or `with_items_from_csv` (already exists).