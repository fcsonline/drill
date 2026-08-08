# Benchmark syntax

We're going to go through all of the benchmark options to understand all
possibilities.

This is a basic benchmark with 2 requests, run 4 plans concurrently against
`http://example.com` servers, executed 5 times. 40 requests in total.

```yaml
---
concurrency: 4
base: 'http://example.com'
iterations: 5
rampup: 5

plan:
  - name: Fetch users
    request:
      url: /api/users.json

  - name: Fetch organizations
    request:
      url: /api/organizations
```

### Benchmark main properties

- `base`: Base url for all relative URL's in your plan. (Optional)
- `iterations`: Number of loops is going to do (Optional, default: 1)
- `concurrency`: Number of concurrent iterations. (Optional, default: max)
- `rampup`: Amount of time it will take to start all iterations. (Optional)
- `threads`: Number of worker threads for the tokio runtime (Optional, default: number of CPU cores, capped at CPU cores)
- `new_conn_per_iter`: Create fresh HTTP connections (new reqwest client, fresh DNS lookup) for every iteration instead of reusing connections (Optional, default: false)
- `persist_context`: Persist cookies and variables across iterations so state carries forward (Optional, default: false)
- `run_time`: Wall-clock duration limit in seconds. The benchmark stops accepting new iterations after this time (Optional, default: no limit)
- `success_codes`: List of HTTP status codes considered successful. When set, only these codes count as success instead of the default 2xx range (Optional, default: 2xx). Example: `[200, 201]`
- `plan`: List of items to do in your benchmark. (Required)

#### Plan items

- `include`: Include all requests in the given file.
- `request`: Execute a HTTP request.
- `assign`: Assign a value in the context to be interpolated later.
- `exec`: Execute a shell command.
- `assert`: Assert a value in the context.
- `delay`: Introduce a controlled delay.
- `for_each`: Iterate over a JSON array in the context and execute a sub-plan.
- `save`: Extract values from the last HTTP response into the context.

All those items can be combined with `name` property to be show in logs.

#### Additional features

- **Fake data generation:** generate random data with `{{ fake.name }}` and locale-specific variants. See [Fake data generation](#fake-data-generation).
- **Results generation:** write CSV stats and HTML reports after the benchmark. See [Results generation](#results-generation).
- **Lifecycle hooks:** run `setup`, `teardown`, `iteration_start`, and `iteration_stop` phases. See [Lifecycle hooks](#lifecycle-hooks).
- **Dynamic iteration:** iterate over response arrays with `for_each`. See [Iterating over context arrays](#iterating-over-context-arrays).

#### Request item properties

- `url`: Url to be request for this item
- `headers`: List of custom headers you want to add in the requests.
- `method`: HTTP method in the requests. Valid methods are GET, POST, PUT, PATCH, HEAD or DELETE. (default: GET)
- `body`: Request body for methods like POST, PUT or PATCH.
- `with_items`: List of items to be interpolated in the given request url.
- `with_items_range`: Generates items from an iterator from start, step (optional, default: 1), stop.
- `with_items_from_csv`: Read the given CSV values and go through all of them as items.
- `shuffle`: Shuffle given items randomly (default: false).
- `pick`: Number of items to pick and perform requests with.
- `assign`: Save the response in the context to be interpolated later.
- `tags`: List of tags for that item.
- `auth`: Authentication configuration for the request. See [Request authentication](#request-authentication).

#### Request authentication

Drill supports native authentication blocks matching common Postman auth types. Add an `auth` block to any request:

```yaml
- name: API with API key
  request:
    url: /api/data
    auth:
      type: "apikey"
      key: "X-API-Key"
      value: "{{ api_key }}"
      in: "header"      # or "query"
```

```yaml
- name: OAuth2 client credentials
  request:
    url: /api/data
    auth:
      type: "oauth2"
      flow: "client_credentials"
      token_url: "https://auth.example.com/oauth/token"
      client_id: "{{ client_id }}"
      client_secret: "{{ client_secret }}"
      scope: "read write"
      save_token_as: "access_token"   # caches token in context, auto-refreshes on expiry
```

Supported auth types:
- `apikey`: adds header (`in: header`) or query parameter (`in: query`) with `key=value`.
- `oauth2` (flow: `client_credentials`): fetches token from `token_url` using client credentials, caches it under `save_token_as` (and `save_token_as_expires`), and sends as Bearer token. Subsequent requests reuse cached token until expiry.
- `basic`: adds `Authorization: Basic <base64(user:pass)>` header (username/password interpolated).
- `bearer`: adds `Authorization: Bearer <token>` header (token interpolated).

For `oauth2`, all fields (`token_url`, `client_id`, `client_secret`, `scope`) are interpolated, so they can come from context or environment variables.

#### with_items_from_csv item properties

This item can be specified one of two ways.  First, as a simple string specifying the csv file name.

Second, it can be a hash with the following properties:

 - `file_name`: csv file containing the records to be used as items
 - `quote_char`: character to use as quote in csv parsing.  Defaults to `"\""`, but can be set to `"\'"`.  If your csv file has quoted strings that contain commas and that causes parse errors, make sure this value is set correctly.

#### body item properties

The `body` property can be specified in different ways depending on the type of data you want to send in the request. Here are six variants:

1. `body: "string with {{ templates }}"`
  - This variant allows you to use a string with templates that can be interpolated with values from the context.

2. `body: { hex: 65 78 61 6D 70 6C 65 }`
  - This variant allows you to send a raw byte string value in the request body.

3. `body: { file: path/to/file.txt }`
  - This variant allows you to specify a file path, and the content of the file will be used as the request body.

4. `body: { urlencoded: { key1: "value1", key2: "value2" } }`
  - Sends `application/x-www-form-urlencoded` body. Keys and values are interpolated.
  - Example:
    ```yaml
    body:
      urlencoded:
        username: "testuser"
        email: "{{ fake.email }}"
    ```

5. `body: { formdata: [ { key: "field1", value: "text" }, { key: "avatar", file: "path/to/image.png", content_type: "image/png" } ] }`
  - Sends `multipart/form-data` body. Each element is a part with a `key` and either a `value` (text field) or `file` (file field). Optional `content_type` for file parts.
  - Example:
    ```yaml
    body:
      formdata:
        - key: "description"
          value: "test upload"
        - key: "file"
          file: "test.txt"
          content_type: "text/plain"
    ```

6. `body: { graphql: { query: "query { user(id: 1) { name } }", variables: { id: "{{ item.id }}" } } }`
  - Sends a GraphQL request as JSON (`application/json`). The `query` string and `variables` map are interpolated.
  - Example:
    ```yaml
    body:
      graphql:
        query: "query GetUser($id: ID!) { user(id: $id) { name } }"
        variables:
          id: "{{ user_id }}"
    ```

#### Built-in interpolation variables

Besides anything you `assign`, a few variables are always available to `{{ }}` templates in URLs, headers and bodies:

- `base`: the benchmark `base` URL.
- `iteration`: the current iteration number (0-based).
- `item`: the current item, only inside `with_items`, `with_items_range`, `with_items_from_csv` or `with_items_from_file` requests.
- `index`: the position of the current item, only inside the `with_items*` requests above. Outside them it is **not** defined — use `iteration` for the current iteration number. Referencing `{{ index }}` where it is not available raises an error (or a warning under `--relaxed-interpolations`) suggesting `iteration`.

#### Fake data generation

Drill can generate fake data at runtime using the `fake.` namespace in `{{ }}` interpolations. This is useful for creating randomized request bodies, URLs, or headers without maintaining data files.

```yaml
- name: Create random user
  request:
    url: /api/users
    method: POST
    body: '{"name":"{{ fake.name }}","email":"{{ fake.email }}","city":"{{ fake.city }}"}'
    headers:
      Content-Type: 'application/json'
```

Supported fake values include:

- **Name**: `name`, `first_name`, `last_name`, `title`, `suffix`, `name_with_title`
- **Internet**: `email`, `free_email`, `username`, `password`, `ipv4`, `ipv6`, `ip`, `mac`, `user_agent`, `domain_suffix`, `free_email_provider`
- **Phone**: `phone`, `cell`
- **Lorem**: `word`, `words`, `sentence`, `sentences`, `paragraph`, `paragraphs`
- **Address**: `city`, `country`, `country_code`, `street`, `state`, `state_abbr`, `zip`, `postcode`, `building_number`, `secondary_address`, `time_zone`, `latitude`, `longitude`
- **Company**: `company`, `company_suffix`, `catch_phrase`, `buzzword`, `bs`, `profession`, `industry`
- **Finance**: `currency_code`, `currency_name`, `currency_symbol`, `credit_card`
- **Filesystem**: `file_path`, `file_name`, `file_extension`, `dir_path`
- **Misc**: `digit`, `boolean`, `number`, `status_code`, `rfc_status_code`

Context variables take precedence over fake values, so assigning a key named `fake` can shadow the namespace.

##### Locale-specific fake data

Add a locale prefix before the value name to generate culturally localized data:

```yaml
- name: Create localized users
  request:
    url: /api/users
    method: POST
    body: '{"name":"{{ fake.zh_cn.name }}","city":"{{ fake.fr_fr.city }}"}'
    headers:
      Content-Type: 'application/json'
```

Supported locales: `en`, `zh_cn`, `zh_tw`, `fr_fr`, `de_de`, `it_it`, `ja_jp`, `pt_br`, `pt_pt`, `ar_sa`, `cy_gb`.

#### Dynamic variables (Postman-compatible)

Drill supports a set of built-in dynamic variables compatible with Postman's `$` variables. These are resolved at interpolation time and can be used in URLs, headers, and bodies with or without the leading `$`.

| Variable | Description | Example |
|---|---|---|
| `$guid` / `$randomUUID` / `$uuid` | Random UUID v4 | `{{ $guid }}` → `550e8400-e29b-41d4-a716-446655440000` |
| `$timestamp` | Unix timestamp (seconds) | `{{ $timestamp }}` → `1700000000` |
| `$isoTimestamp` | ISO 8601 / RFC3339 timestamp | `{{ $isoTimestamp }}` → `2023-11-14T12:00:00Z` |
| `$randomInt(min,max)` | Random integer in range | `{{ $randomInt(1,100) }}` → `42` |
| `$randomFloat(min,max)` | Random float in range | `{{ $randomFloat(0.0,1.0) }}` → `0.73` |
| `$randomBoolean` | Random boolean | `{{ $randomBoolean }}` → `true` |
| `$randomHex(length?)` | Random hex string (default 16 chars) | `{{ $randomHex(32) }}` → `a1b2c3...` |
| `$randomAlphaNumeric(length?)` | Random alphanumeric (default 16 chars) | `{{ $randomAlphaNumeric(10) }}` → `X7k9P2mQ` |
| `$randomFirstName` | Random first name (via fake) | `{{ $randomFirstName }}` → `John` |
| `$randomLastName` | Random last name (via fake) | `{{ $randomLastName }}` → `Doe` |
| `$randomFullName` | Random full name (via fake) | `{{ $randomFullName }}` → `John Doe` |
| `$randomEmail` | Random email (via fake) | `{{ $randomEmail }}` → `john@example.com` |
| `$randomPhoneNumber` | Random phone (via fake) | `{{ $randomPhoneNumber }}` → `+1-555-123-4567` |
| `$randomCity` | Random city (via fake) | `{{ $randomCity }}` → `Springfield` |
| `$randomStreetAddress` | Random street (via fake) | `{{ $randomStreetAddress }}` → `123 Main St` |
| `$randomCountry` | Random country (via fake) | `{{ $randomCountry }}` → `United States` |
| `$randomIp` | Random IP address (via fake) | `{{ $randomIp }}` → `192.168.1.1` |

The fake-backed variables (`$randomFirstName`, etc.) support locale prefixes like `{{ $fr_fr.randomCity }}` (same locales as `fake.*`).

Context variables take precedence over dynamic variables, which take precedence over `fake.*`.

#### Results generation

Drill can write per-request and overall statistics to files after the benchmark completes, similar to Locust's CSV and HTML reports. Enable it with the top-level `results` key.

Simple usage — output to a directory, generating both CSV and HTML:

```yaml
---
concurrency: 4
base: 'http://example.com'
iterations: 100
results: ./drill-results

plan:
  - name: Fetch users
    request:
      url: /api/users.json
```

Advanced usage:

```yaml
results:
  output_dir: ./drill-results
  csv: true
  html: true
```

Generated files:

- `stats.csv` — per-request and overall stats: request count, failures, median/average/min/max/std-dev response times, requests/s, failures/s, and percentiles.
- `report.html` — self-contained report with a stats table, requests-per-second-over-time chart, average-response-time bar chart, and a failures table.

The default output directory is `drill-results`. Set `csv: false` or `html: false` to skip one of the outputs.

#### Lifecycle hooks

Drill supports optional lifecycle hooks similar to Locust's `test_start`/`test_stop` events. Use the top-level `lifecycle` key to define phases that run outside the main `plan`.

Phases:

- `setup`: runs once before any iteration. Its final context is cloned and used as the starting context for every iteration.
- `teardown`: runs once after all iterations complete, with the same context produced by `setup`.
- `iteration_start`: runs at the beginning of every iteration.
- `iteration_stop`: runs at the end of every iteration.

Each phase is a list of the same plan items supported in `plan`: `request`, `assign`, `exec`, `assert`, `delay`, and `include`.

```yaml
---
concurrency: 4
base: 'http://example.com'
iterations: 100

lifecycle:
  setup:
    - name: Create test user
      request:
        url: /api/users
        method: POST
        body: '{"name":"drill"}'
        headers:
          Content-Type: 'application/json'
      assign: user
  teardown:
    - name: Delete test user
      request:
        url: /api/users/{{ user.body.id }}
        method: DELETE
  iteration_start:
    - name: Set iteration marker
      assign:
        key: started
        value: 'true'
  iteration_stop:
    - name: Clear iteration marker
      assign:
        key: started
        value: 'false'

plan:
  - name: Fetch user
    request:
      url: /api/users/{{ user.body.id }}
```

Lifecycle hooks are optional. If a phase is omitted, Drill behaves as before and runs only the `plan`.

#### Iterating over context arrays

Drill can iterate over a JSON array stored in the context (for example, a response body from a previous request) using the `for_each` action. This is the runtime equivalent of `with_items`.

```yaml
- name: Fetch users
  request:
    url: /api/users
  assign: users

- name: Fetch each user
  for_each:
    items: '{{ users.body }}'
    item_key: user
    index_key: idx
    plan:
      - name: Fetch user details
        request:
          url: /api/users/{{ user.id }}
```

Properties:

- `items`: interpolation expression resolving to a JSON array. Required.
- `item_key`: name of the variable exposed to the sub-plan for each item. Default: `item`.
- `index_key`: optional name of the variable holding the 0-based index.
- `shuffle`: randomly shuffle the array before iterating. Default: `false`.
- `pick`: limit the number of items to iterate.
- `plan`: list of plan items to execute per item. Required.

The sub-plan runs in the current iteration context, with the current item and index added. Items are processed sequentially.

#### Save action

Drill can extract values from the last HTTP response and store them in the context for use in subsequent requests. This is useful for chaining requests (e.g., login → get token → use token).

```yaml
- name: Login
  request:
    url: /login
    method: POST
    body: '{"user":"x","pass":"y"}'

- name: Save auth token
  save:
    source: response_body      # or response_headers, response_status, response_url
    jsonpath: "$.token"        # optional; if omitted, stores entire source
    key: auth_token            # context key to store result
```

```yaml
- name: Use token
  request:
    url: /api/data
    headers:
      Authorization: "Bearer {{ auth_token }}"
```

Properties:
- `source`: where to extract from — `response_body` (JSON or raw), `response_headers`, `response_status`, `response_url`. Required.
- `jsonpath`: JSONPath expression (e.g., `$.data.id`, `$.items[0].name`). Works with `response_body`. If omitted, the entire source is stored.
- `key`: context key to store the extracted value. For `response_headers`, this is the header name (case-insensitive lookup).

#### Extended assertions

The `assert` action supports multiple assertion types beyond simple context key/value equality. All extended types operate on the **last HTTP response** (stored automatically after each request).

**1. Status assertion**
```yaml
- assert:
    type: status
    value: 200              # single status code
```
```yaml
- assert:
    type: status
    value: [200, 201, 204]  # array of acceptable codes
```

**2. Header assertion** (case-insensitive substring match)
```yaml
- assert:
    type: header
    key: "content-type"
    value: "application/json"
```

**3. JSONPath assertion** (on response body)
```yaml
- assert:
    type: jsonpath
    key: "$.data.id"
    value: "123"
```
Uses JSONPath on the parsed response body. The first match is compared to `value` (type-tolerant: number `123` equals string `"123"`).

**4. Duration assertion** (response time in milliseconds)
```yaml
- assert:
    type: duration
    value: 500              # max ms
    operator: "lt"          # lt, lte, gt, gte, eq (default: lt)
```

**5. Context equality** (legacy/default — operates on context variables)
```yaml
- assert:
    key: some_context_key
    value: "expected_value"
```
If `type` is omitted, this is the default behavior (asserts a context variable equals a value).

`weight` applies to `assert` items as with other plan items.

#### Custom load shapes

Add a `weight` property to any plan item to control how often it runs relative to other items. When at least one item has a weight other than `1`, Drill switches to weighted random selection: each iteration picks exactly one item based on the weights.

```yaml
plan:
  - name: Rare task
    request:
      url: /api/rare
    weight: 1

  - name: Common task
    request:
      url: /api/common
    weight: 4
```

In this example, `Common task` runs roughly 4 times more often than `Rare task`. If no weights are present, Drill runs all plan items sequentially in each iteration, as before.

`weight` applies to `request`, `assign`, `exec`, `assert`, `delay`, and `for_each` items.

#### Custom load shapes

By default, Drill starts all iterations as fast as possible, optionally using `rampup` for a linear ramp. For more complex load profiles, use `load_shape` with a list of stages.

```yaml
---
iterations: 1000
base: 'http://example.com'

load_shape:
  stages:
    - duration: 60
      users: 100
    - duration: 120
      users: 100
    - duration: 60
      users: 0

plan:
  - name: Load task
    request:
      url: /api/users
```

Each stage defines:

- `duration`: stage length in seconds. Required.
- `users`: target number of concurrent users at the end of the stage. Required.
- `spawn_rate`: reserved for future use; currently the scheduler uses the target users as the concurrency limit.

Drill spaces iterations over the total duration according to the area under the users curve. The concurrency limit is set to the maximum `users` value across all stages. When `load_shape` is present, `concurrency` and `rampup` are ignored.

#### Open workload model (arrival rate)

The models above are **closed**: a fixed number of iterations (`iterations`) is started, and the server's response time decides how long the run lasts. The open (arrival-rate) model inverts this: arrivals are scheduled at a configured rate, independent of how fast the server responds, until a budget is consumed. When the in-flight ceiling is already reached, an arrival is **dropped** and counted, never queued or blocked.

Constant-rate form:

```yaml
---
base: 'http://example.com'

arrival_rate:
  rate: 20                # constant: 20 arrivals/second
  max_concurrency: 50     # ceiling on simultaneous in-flight iterations
  duration: 30            # budget #1: run for 30 seconds

plan:
  - name: Load task
    request:
      url: /api/users
```

Ramping form:

```yaml
---
base: 'http://example.com'

arrival_rate:
  stages:
    - duration: 60
      rate: 10
    - duration: 120
      rate: 100
    - duration: 60
      rate: 25
  max_concurrency: 100
  max_iterations: 5000    # budget #2: stop after 5000 arrivals

plan:
  - name: Load task
    request:
      url: /api/users
```

Key rules:

- `max_concurrency` is **required**: it caps simultaneous in-flight iterations. When the ceiling is reached, an arrival is dropped and recorded in the `dropped_iterations` counter.
- At least one budget is required — `duration` (whole seconds) and/or `max_iterations` (total arrivals). With both, the first bound reached wins. `duration` is exclusive: an arrival offset landing exactly on the boundary is not scheduled.
- Ramping stages linearly interpolate the rate from each stage's start value to its end value over `duration` seconds; the first stage holds its rate flat.
- `arrival_rate` is mutually exclusive with `concurrency`, `iterations`, `rampup`, and `load_shape` (the closed-model knobs).
- The `queue`, `block`, `preallocated`, and `on_ceiling` keys are not yet supported; `drill validate` rejects them.
- The `run_time` cap and `arrival_rate.duration` both bound the wall-clock run; whichever is reached first ends the run.

Iteration-level counters (`scheduled_iterations`, `started_iterations`, `dropped_iterations`, `in_flight_iterations`) appear in `--stats` output and, for `--stats-json`, as additive values: interval records carry deltas since the previous tick, the terminal record carries cumulative totals, and a residual interval line covers the gap since the last tick, so `sum(interval deltas) + residual == final totals`. Invariant: `started + dropped == scheduled`.

#### tags item properties

[Ansible](https://docs.ansible.com/ansible/latest/user_guide/playbooks_tags.html#special-tags-always-and-never)-like tags.

If you assign list of tags, e.g `[tag1, tag2]`, this item will be executed if `tag1` **OR** `tag2` is passed.

Special tags: `always` and `never`.

If you assign the `always` tag, `drill` will always run that item, unless you specifically skip it (`--skip-tags always`).

If you assign the `never` tag to item, `drill` will skip that item unless you specifically request it (`--tags never`).
