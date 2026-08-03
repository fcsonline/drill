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
- `plan`: List of items to do in your benchmark. (Required)

#### Plan items

- `include`: Include all requests in the given file.
- `request`: Execute a HTTP request.
- `assign`: Assign a value in the context to be interpolated later.
- `exec`: Execute a shell command.
- `assert`: Assert a value in the context.
- `delay`: Introduce a controlled delay.
- `for_each`: Iterate over a JSON array in the context and execute a sub-plan.

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

#### with_items_from_csv item properties

This item can be specified one of two ways.  First, as a simple string specifying the csv file name.

Second, it can be a hash with the following properties:

 - `file_name`: csv file containing the records to be used as items
 - `quote_char`: character to use as quote in csv parsing.  Defaults to `"\""`, but can be set to `"\'"`.  If your csv file has quoted strings that contain commas and that causes parse errors, make sure this value is set correctly.

#### body item properties

The `body` property can be specified in different ways depending on the type of data you want to send in the request. Here are three variants:

1. `body: "string with {{ templates }}"`
  - This variant allows you to use a string with templates that can be interpolated with values from the context.

2. `body: { hex: 65 78 61 6D 70 6C 65 }`
  - This variant allows you to send a raw byte string value in the request body.

3. `body: { file: path/to/file.txt }`
  - This variant allows you to specify a file path, and the content of the file will be used as the request body.

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

#### tags item properties

[Ansible](https://docs.ansible.com/ansible/latest/user_guide/playbooks_tags.html#special-tags-always-and-never)-like tags.

If you assign list of tags, e.g `[tag1, tag2]`, this item will be executed if `tag1` **OR** `tag2` is passed.

Special tags: `always` and `never`.

If you assign the `always` tag, `drill` will always run that item, unless you specifically skip it (`--skip-tags always`).

If you assign the `never` tag to item, `drill` will skip that item unless you specifically request it (`--tags never`).
