# postman2drill

Convert Postman Collection v2.1 into a [Drill](https://github.com/fcsonline/drill) benchmark YAML.

## Usage

```bash
# Convert a collection to Drill YAML
postman2drill collection.json -o benchmark.yml

# Optional: apply a Postman environment
postman2drill collection.json environment.json -o benchmark.yml

# Capture warnings to a JSON report
postman2drill collection.json -o benchmark.yml -w warnings.json -f json

# Fail on warnings
postman2drill collection.json -o benchmark.yml --strict
```

## Options

```
postman2drill [OPTIONS] <COLLECTION> [ENVIRONMENT]

Arguments:
  <COLLECTION>     Postman collection JSON file
  [ENVIRONMENT]    Optional Postman environment JSON file

Options:
  -o, --output <FILE>      Output Drill YAML file (default: stdout)
  -w, --warnings <FILE>    Warnings report file (default: stderr)
  -f, --format <FORMAT>    Warnings format: json or text [default: json]
      --strict             Treat warnings as errors (exit non-zero)
  -h, --help               Print help
  -V, --version            Print version
```

## Supported features

- Postman Collection v2.1 JSON
- Optional Postman environment JSON
- Request methods, URL, headers, query parameters
- Body modes: `raw`, `urlencoded`, `formdata`, `graphql`, `file`
- Authentication: `basic`, `bearer`, `apikey`, `oauth2` (client_credentials only)
- Folder-level auth inheritance
- Test scripts:
  - `pm.expect(pm.response.code).to.eql(...)` → status assertion
  - `pm.expect(pm.response.json().path).to.eql(...)` → JSON path assertion
  - `pm.expect(pm.response.jsonPath("$.path")).to.eql(...)` → JSON path assertion
  - `pm.expect(pm.response.responseTime).to.be.below(...)` → duration assertion
  - `pm.expect(pm.response.headers.get("...")).to.eql(...)` → header assertion
  - `pm.collectionVariables.set(...)` / `pm.environment.set(...)` → `assign` or `save`
  - `pm.globals.set(...)` is not supported
- Collection variables become top-level `vars:`
- Dynamic variables (`{{ var }}`, `{{ $randomEmail }}`) are preserved as Drill interpolations

## Warnings

The converter reports patterns it cannot translate so you can review them manually. Warnings can be written as JSON or text.

```json
[
  {
    "level": "warn",
    "location": "item[0]",
    "message": "OAuth2: only client_credentials flow supported; other flows require manual setup"
  }
]
```

## Building

```bash
cargo build --release
./target/release/postman2drill collection.json -o benchmark.yml
```

## Running tests

```bash
cargo test --workspace
```

## Limitations

- Pre-request scripts are not translated yet.
- `pm.globals.set(...)` is not supported.
- OAuth2 flows other than `client_credentials` require manual setup.
- Complex Postman test scripts may need manual adjustment.
