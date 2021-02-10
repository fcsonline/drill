# Drill

[![](http://meritbadge.herokuapp.com/drill?ts=2)](https://crates.io/crates/drill)
![](https://travis-ci.com/fcsonline/drill.svg?branch=master)

Drill is a HTTP load testing application written in Rust. The main goal
for this project is to build a really lightweight tool as alternative to other
that require JVM and other stuff.

You can write benchmark files, in YAML format, describing all the stuff you
want to test.

It was inspired by [Ansible](http://docs.ansible.com/ansible/playbooks_intro.html)
syntax because it is really easy to use and extend.

Here is an example for **benchmark.yml**:

```yaml
---

concurrency: 4
base: 'http://localhost:9000'
iterations: 5
rampup: 2

plan:
  - name: Include comments
    include: comments.yml

  - name: Fetch users
    request:
      url: /api/users.json

  - name: Fetch organizations
    request:
      url: /api/organizations

  - name: Fetch account
    request:
      url: /api/account
    assign: foo

  - name: Fetch manager user
    request:
      url: /api/users/{{ foo.body.manager_id }}

  - name: Assign values
    assign:
      key: bar
      value: "2"

  - name: Fetch user from assign
    request:
      url: /api/users/{{ bar }}

  - name: Fetch some users
    request:
      url: /api/users/{{ item }}
    with_items:
      - 70
      - 73
      - 75

  - name: Fetch some users by hash
    request:
      url: /api/users/{{ item.id }}
    with_items:
      - { id: 70 }
      - { id: 73 }
      - { id: 75 }

  - name: Fetch some users by range, index {{ index }}
    request:
      url: /api/users/{{ item }}
    with_items_range:
      start: 70
      step: 5
      stop: 75

  - name: Fetch some users from CSV, index {{ index }}
    request:
      url: /api/users/contacts/{{ item.id }}
    with_items_from_csv: ./fixtures/users.csv
    shuffle: true

  - name: POST some crafted JSONs stored in CSV, index {{ index }}
    request:
      url: /api/transactions
      method: POST
      body: '{{ item.txn }}'
      headers:
        Content-Type: 'application/json'
    with_items_from_csv:
      file_name: ./fixtures/transactions.csv
      quote_char: "\'"

  - name: Fetch no relative url
    request:
      url: http://localhost:9000/api/users.json

  - name: Interpolate environment variables
    request:
      url: http://localhost:9000/api/{{ EDITOR }}

  - name: Support for POST method
    request:
      url: /api/users
      method: POST
      body: foo=bar&arg={{ bar }}

  - name: Login user
    request:
      url: /login?user=example&password=3x4mpl3

  - name: Fetch counter
    request:
      url: /counter
    assign: memory

  - name: Fetch counter
    request:
      url: /counter
    assign: memory

  - name: Fetch endpoint
    request:
      url: /?counter={{ memory.body.counter }}

  - name: Reset counter
    request:
      method: DELETE
      url: /

  - name: Custom headers
    request:
      url: /admin
      headers:
        Authorization: Basic aHR0cHdhdGNoOmY=
        X-Foo: Bar
        X-Bar: Bar {{ memory.headers.token }}
```

As you can see, you can play with interpolations in different ways. This
will let you specify a benchmark with different requests and
dependencies between them.

If you want to know more about the benchmark file syntax, [read this](./SYNTAX.md)

## Install

The easiest way right now is to install with [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html):

```
cargo install drill
drill --benchmark benchmark.yml --stats
```

or download the source code and compile it:

```
git clone git@github.com:fcsonline/drill.git && cd drill
cargo build --release
./target/release/drill --benchmark benchmark.yml --stats
```

**Note:** You will need to install `libssl-dev` and `pkg-config` packages.

## Demo

[![demo](https://asciinema.org/a/164615.png)](https://asciinema.org/a/164615?autoplay=1)

## Features

This is the list of all features supported by the current version of `drill`:

- **Concurrency:** run your benchmarks choosing the number of concurrent iterations.
- **Multi iterations:** specify the number of iterations you want to run the benchmark.
- **Ramp-up:** specify the amount of time it will take `drill` to start all iterations.
- **Delay:** introduce controlled delay between requests. Example: [assigns.yml](./example/delay.yml)
- **Dynamic urls:** execute requests with dynamic interpolations in the url, like `/api/users/{{ item }}`
- **Dynamic headers:** execute requests with dynamic headers. Example: [headers.yml](./example/headers.yml)
- **Interpolate environment variables:** set environment variables, like `/api/users/{{ EDITOR }}`
- **Request dependencies:** create dependencies between requests with `assign` and url interpolations.
- **Split files:** organize your benchmarks in multiple files and include them.
- **CSV support:** read CSV files and build N requests fill dynamic interpolations with CSV data.
- **HTTP methods:** build request with different http methods like GET, POST, PUT, PATCH, HEAD or DELETE.
- **Cookie support:** create benchmarks with sessions because cookies are propagates between requests.
- **Stats:** get nice statistics about all the requests. Example: [cookies.yml](./example/cookies.yml)
- **Thresholds:** compare the current benchmark performance against a stored one session and fail if a threshold is exceeded.

## Test it

Go to the `example` directory and you'll find a [README](./example) how
to test it in a safe environment.

**Disclaimer**: We really recommend not to run intensive benchmarks against
production environments.

## Command line interface

Full list of cli options, which is available under `drill --help`

```
drill 0.7.1
HTTP load testing application written in Rust inspired by Ansible syntax

USAGE:
    drill [OPTIONS] --benchmark <benchmark>

FLAGS:
    -h, --help       Prints help information
        --no-check-certificate    Disables SSL certification check. (Not recommended)
        --relaxed-interpolations    Do not panic if an interpolation is not present. (Not recommended)
    -s, --stats      Shows request statistics
    -q, --quiet      Skips output of individual request statistics
    -n, --nanosec    Shows statistics in nanoseconds
    -V, --version    Prints version information

OPTIONS:
    -b, --benchmark <benchmark>        Sets the benchmark file
    -c, --compare <compare>            Sets a compare file
    -p, --concurrency <concurrency>    Number of concurrent requests
    -i, --iterations <iterations>      Total number of requests to perform
    -e, --rampup <rampup>              Amount of time it takes to reach full concurrency
    -r, --report <report>              Sets a report file
    -t, --threshold <threshold>        Sets a threshold value in ms amongst the compared file
    -u, --url <url>                    Base URL for requests

```

## Roadmap

- Complete and improve the interpolation engine
- Add writing to a file support

## Contribute

This project started as a side project to learn Rust, so I'm sure that is full
of mistakes and areas to be improve. If you think you can tweak the code to
make it better, I'll really appreciate a pull request. ;)

