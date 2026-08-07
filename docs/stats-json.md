# JSON Lines (NDJSON) statistics streaming

`--stats-json` emits machine-readable statistics as **NDJSON** (one JSON object per line) on
**stdout**, so a consumer can tail a live benchmark run and read per-interval plus a terminal
record. Nothing else is written to stdout while this mode is active — banners, per-request logs,
and diagnostics go to **stderr** (stdout purity, RFP §5).

## Usage

```bash
drill --benchmark benchmark.yml --stats-json
drill --benchmark benchmark.yml --stats-json --stats-interval 2
```

- `--stats-json` — enable NDJSON streaming to stdout.
- `--stats-interval <sec>` — wall-clock seconds between interval records (default `1`).
  Requires `--stats-json`; a value of `0` is rejected.
- `--verbose`, banners, and per-request lines remain available on stderr — stdout carries only
  the NDJSON stream.
- The human `--stats` console table is unchanged (`--stats` and `--stats-json` are independent
  flags; the table output is byte-identical to its pre-`--stats-json` behavior).

## Stream shape

Every line is one strict JSON object. Records are either **interval** records (one per
`--stats-interval` slice) or the terminal **final** record. A consumer can use `"version"` to
reject unknown schemas, and `"final"` to detect the end of the stream.

### Record framing

| Field | Interval record | Final record |
|---|---|---|
| `version` | `1` (integer) | `1` |
| `interval` | slice number, `1`-based | absent |
| `final` | absent | `true` |
| `time_elapsed_sec` | seconds since run start | absent |
| `endpoints` | array of endpoint objects | array of endpoint objects |
| `global` | aggregate object, **no** `status` | aggregate object with `status` |

### Endpoint object

| Field | Type | Meaning |
|---|---|---|
| `name` | string | plan item name |
| `total_requests` | int | requests in this slice |
| `successful_requests` | int | non-failed requests |
| `failed_requests` | int | failed requests (non-success status or transport error) |
| `avg_ms` / `median_ms` / `stdev_ms` | float | latency stats in milliseconds |
| `p50_ms` … `p9999_ms` | float | percentile latencies |
| `max_ms` | float | slowest request |
| `rps` | float | requests per second in this slice |
| `failures_per_sec` | float | failures per second in this slice |

### Global object

The `global` object aggregates **all** reports seen so far, with the same fields as the endpoint
object plus:

| Field | Type | Meaning |
|---|---|---|
| `duration_sec` | float | wall-clock duration of the slice (interval) / whole run (final) |
| `time_elapsed_sec` | float | seconds since run start |
| `status` | string | **final record only**: `completed`, `failed`, or `cancelled` |

A slice with no completed requests still emits an interval record with zeroed counters and an
empty `endpoints` array, so consumers can draw an idle timeline.

### Final `status`

| Value | When |
|---|---|
| `completed` | run finished normally, no assertion failures |
| `failed` | at least one assertion failure recorded |
| `cancelled` | interrupted by SIGINT/SIGTERM, or the `--run-time` cap was reached |

A benchmark with an **empty plan** still emits exactly one final record with zeroed counters and
`status: "failed"`, then exits with code `1`.

## Example

```bash
drill --benchmark benchmark.yml --stats-json --stats-interval 1
```

```jsonl
{"version":1,"interval":1,"time_elapsed_sec":1.002,"endpoints":[{"name":"Get root","total_requests":2,"successful_requests":2,"failed_requests":0,"avg_ms":3.5,"median_ms":3.2,"stdev_ms":0.4,"p50_ms":3.1,"p66_ms":3.4,"p75_ms":3.6,"p80_ms":3.8,"p90_ms":4.0,"p95_ms":4.1,"p98_ms":4.2,"p99_ms":4.2,"p999_ms":4.2,"p9999_ms":4.2,"max_ms":4.1,"rps":2.0,"failures_per_sec":0.0}],"global":{"total_requests":2,"successful_requests":2,"failed_requests":0,"avg_ms":3.5,"median_ms":3.2,"stdev_ms":0.4,"p50_ms":3.1,"p66_ms":3.4,"p75_ms":3.6,"p80_ms":3.8,"p90_ms":4.0,"p95_ms":4.1,"p98_ms":4.2,"p99_ms":4.2,"p999_ms":4.2,"p9999_ms":4.2,"max_ms":4.1,"rps":2.0,"failures_per_sec":0.0,"duration_sec":1.002,"time_elapsed_sec":1.002}}
{"version":1,"interval":2,"time_elapsed_sec":2.001,"endpoints":[{"name":"Get root","total_requests":3,"successful_requests":3,"failed_requests":0,"avg_ms":3.1,"median_ms":3.0,"stdev_ms":0.3,"p50_ms":2.9,"p66_ms":3.1,"p75_ms":3.2,"p80_ms":3.3,"p90_ms":3.5,"p95_ms":3.6,"p98_ms":3.7,"p99_ms":3.7,"p999_ms":3.7,"p9999_ms":3.7,"max_ms":3.5,"rps":3.0,"failures_per_sec":0.0}],"global":{"total_requests":5,"successful_requests":5,"failed_requests":0,"avg_ms":3.3,"median_ms":3.1,"stdev_ms":0.4,"p50_ms":2.9,"p66_ms":3.3,"p75_ms":3.5,"p80_ms":3.7,"p90_ms":4.0,"p95_ms":4.1,"p98_ms":4.2,"p99_ms":4.2,"p999_ms":4.2,"p9999_ms":4.2,"max_ms":4.1,"rps":2.5,"failures_per_sec":0.0,"duration_sec":0.999,"time_elapsed_sec":2.001}}
{"version":1,"final":true,"endpoints":[{"name":"Get root","total_requests":5,"successful_requests":5,"failed_requests":0,"avg_ms":3.3,"median_ms":3.1,"stdev_ms":0.4,"p50_ms":2.9,"p66_ms":3.3,"p75_ms":3.5,"p80_ms":3.7,"p90_ms":4.0,"p95_ms":4.1,"p98_ms":4.2,"p99_ms":4.2,"p999_ms":4.2,"p9999_ms":4.2,"max_ms":4.1,"rps":2.5,"failures_per_sec":0.0}],"global":{"total_requests":5,"successful_requests":5,"failed_requests":0,"avg_ms":3.3,"median_ms":3.1,"stdev_ms":0.4,"p50_ms":2.9,"p66_ms":3.3,"p75_ms":3.5,"p80_ms":3.7,"p90_ms":4.0,"p95_ms":4.1,"p98_ms":4.2,"p99_ms":4.2,"p999_ms":4.2,"p9999_ms":4.2,"max_ms":4.1,"rps":2.5,"failures_per_sec":0.0,"duration_sec":2.001,"time_elapsed_sec":2.001,"status":"completed"}}
```

> Note: the example numbers are illustrative; actual latency values vary per run. All lines are
> flushed line-by-line, so a consumer can read interval records before the run finishes.

## Exit codes

The exit code is unchanged by `--stats-json`: `0` when the run completes with no assertion
failures, `1` otherwise (including an empty plan). A signal-interrupted run still emits a final
`cancelled` record before exiting, and exits `0` — consumers should rely on the final record's
`global.status`, not the process exit code.

## Fixture set

`tests/fixtures/stats-json/` ships static NDJSON examples (RFP §9) that consumers can
validate their parsers against without running Drill:

| Fixture | Covers |
|---|---|
| `basic.ndjson` | 2 endpoints, 3 intervals, terminal `completed` record |
| `empty.ndjson` | zero-request run: terminal record only, zeroed counters, `status: "failed"` |
| `assert-fail.ndjson` | assertion failure: terminal `status: "failed"` |
| `early-cancel.ndjson` | SIGTERM mid-run: partial counters, terminal `status: "cancelled"` |
| `malformed.ndjson` | a non-JSON line the consumer must skip while still finding the final record (RFP §10) |

`tests/stats_json_fixtures.rs` guards that these files stay schema-conformant.

## Files

- Stream implementation: `src/stats_stream.rs`
- Wiring: `src/benchmark.rs`, `src/main.rs`, `src/config.rs`, `src/actions/request.rs`
- End-to-end tests: `tests/stats_json_e2e.rs`
- Published fixture set: `tests/fixtures/stats-json/*.ndjson` (guarded by `tests/stats_json_fixtures.rs`)
- Human-output regression guard: `tests/stats_legacy.rs`
