# RFP — Drill Result-Stream (NDJSON) for Sutomu

> **Status**: draft, implementation-ready
> **Audience:** `sgmovea2z/drill` maintainers (fork of `fcsonline/drill`)
> **Requesting project:** Sutomu — a load-testing product whose only engine is Drill
> **Current pin:** Drill `v0.1.0` @ `edfda1bb293f47c169e6c215a2598df5556e1123`
> **Research baseline:** Drill `v0.10.1` @ `e62adcb21eee8cc3daa023d86380b3c301480b3d` (source-verified 2026-08-07)
> **Related docs:** `docs/drill_dependencies.md` (DD-01/DD-04/FR-06), `docs/drill-capability-gap.md`, `docs/task-01-results-loop.md`

---

## 1. Problem statement

Sutomu runs Drill as its sole load-test engine and needs **structured, streaming,
machine-readable results** to power live time-series charts, stable parsing, a
terminal run status, and assertion-failure surfacing. Today Sutomu pins Drill
`v0.1.0` and parses the human-oriented `drill --stats` console table in
`libs/drill-stats` (`parse_drill_stats`). That text surface is insufficient for
the product (see §2).

Research confirms Drill `v0.10.1` already ships `--stats-json` (NDJSON),
`--stats-interval`, rich endpoint/global statistics, and a terminal final
record. This RFP asks the Drill maintainers to **harden, version, and complete**
that stream into a stable contract Sutomu can depend on. Capabilities already
reported in `v0.10.1` are marked **[reported]**; requirements that still need
confirmation or implementation are marked **[required]**.

---

## 2. Why plain-text `--stats` is insufficient

| Need | `--stats` text | `--stats-json` stream |
|---|---|---|
| Structured endpoint metrics | Fragile label→value regex; per-request blocks parsed then discarded | Native JSON objects per endpoint |
| Live time-series charts | No periodic samples at all | One NDJSON record per interval |
| Stable parsing | Any label rename / column shift silently breaks two duplicated parsers | Versioned schema (§6) |
| Final status | Implied by exit code only | Explicit terminal record |
| Assertion failures | Only a stderr line + exit code | Needs explicit field (see §5) |

Sutomu's `run_detail` charts 2/3 (Throughput Over Time, Error Rate Over Time)
are gated on a `history` series **no producer currently emits**. The text
format cannot ever provide it; a streamed NDJSON interval record can.

---

## 3. In-scope / out-of-scope

### 3.1 In scope (this RFP)

- A **normative NDJSON result stream** on stdout (§4).
- **CLI flags** to enable and configure the stream (§5).
- **Terminal final-record and exit semantics** (§6).
- **Schema versioning and backward compatibility** (§7).
- **Acceptance criteria** (§8) and **fixtures** (§9).

### 3.2 Out of scope (explicitly NOT requested here)

- TLS/JA3 fingerprint impersonation (tracked separately, user-owned).
- Network-profile latency/jitter shaping (tracked separately, user-owned).
- Per-VU context persistence (`persist_context`), wall-clock `--run-time`,
  non-aborting assertions, configurable success codes, signal-based graceful
  cancel — these are **separate** Drill features; this RFP only requires that
  the result stream **reflect** their outcomes (e.g. a partial final record on
  early termination), not that Drill implement them.
- CSV export (`--stats-csv`) and HTML/CSV report files — out of scope; the
  NDJSON stream is the contract Sutomu consumes.

---

## 4. Normative NDJSON schema

The stream is **line-delimited JSON** (NDJSON): exactly one JSON object per
line, `\n`-terminated, written to **stdout**. Every line is either an
**interval record** or the **final record**. A consumer MUST be able to parse
the stream incrementally, line by line, without buffering the whole run.

### 4.1 Record framing

- **[reported]** Interval records carry `"interval"` (1-based integer) and
  `"endpoints"` (array). They MUST NOT carry `"final": true`.
- **[reported]** The terminal record carries `"final": true` and is the LAST
  line of the stream.
- **[required]** Every record MUST carry a `"version"` field (integer) so
  consumers can reject unknown schemas (§7). *Not present in v0.10.1.*

### 4.2 Endpoint object (shared by interval and final records)

**[reported]** Field names and units:

| Field | Type | Unit | Meaning |
|---|---|---|---|
| `name` | string | — | Request/plan-item name |
| `total_requests` | int | count | Requests in this slice |
| `successful_requests` | int | count | Requests accepted by Drill's configured success-code policy |
| `failed_requests` | int | count | `total − successful` (3xx/4xx/5xx/conn errors) |
| `avg_ms` | float | ms | Mean response time |
| `median_ms` | float | ms | p50 |
| `stdev_ms` | float | ms | Sample stddev |
| `p50_ms` … `p9999_ms` | float | ms | `p50,p66,p75,p80,p90,p95,p98,p99,p999,p9999` |
| `max_ms` | float | ms | Max response time |

**[required]** Add per-endpoint throughput so consumers need not derive it:
`rps` (float, `total_requests / interval_duration`) and `failures_per_sec`
(float). *Not present in v0.10.1 endpoint objects.*

### 4.3 Interval record

**[reported]** Shape:

```json
{
  "version": 1,
  "interval": 1,
  "time_elapsed_sec": 1.0,
  "endpoints": [ { "name": "Get root", "total_requests": 12, "successful_requests": 12, "failed_requests": 0, "avg_ms": 3.2, "median_ms": 3.0, "stdev_ms": 0.4, "p50_ms": 3.0, "p66_ms": 3.1, "p75_ms": 3.2, "p80_ms": 3.3, "p90_ms": 3.5, "p95_ms": 3.8, "p98_ms": 4.0, "p99_ms": 4.2, "p999_ms": 4.5, "p9999_ms": 4.8, "max_ms": 5.0, "rps": 12.0, "failures_per_sec": 0.0 } ]
}
```

- **[reported]** `time_elapsed_sec` is the **end** of the interval.
- **[required]** Add a `"global"` object to the interval record (same shape as
  the final record's `global`, §4.4) so consumers get per-interval aggregate
  RPS / error-rate without summing endpoints. *Not present in v0.10.1 interval
  records.* This is the single most important gap for Sutomu's Throughput and
  Error-Rate time-series charts.
- **[required]** `time_elapsed_sec` MUST reflect **actual wall-clock elapsed
  time**, not a normalized `duration / interval_count` value. *v0.10.1 computes
  `interval_duration = duration / ceil(duration/interval_secs)`, which can
  differ from the requested `--stats-interval`; see §5.*

### 4.4 Final record

**[reported]** Shape:

```json
{
  "version": 1,
  "final": true,
  "endpoints": [ { /* endpoint object, §4.2 */ } ],
  "global": {
    "total_requests": 120,
    "successful_requests": 118,
    "failed_requests": 2,
    "avg_ms": 3.1, "median_ms": 3.0, "stdev_ms": 0.5,
    "p50_ms": 3.0, "p66_ms": 3.1, "p75_ms": 3.2, "p80_ms": 3.3,
    "p90_ms": 3.6, "p95_ms": 4.0, "p98_ms": 4.4, "p99_ms": 4.9,
    "p999_ms": 5.5, "p9999_ms": 6.0, "max_ms": 8.0,
    "rps": 40.0,
    "duration_sec": 3.0,
    "time_elapsed_sec": 3.0
  }
}
```

- **[reported]** `global` carries aggregate counts, percentiles, `rps`,
  `duration_sec`, and `time_elapsed_sec`.
- **[required]** Add `failures_per_sec` to `global`. *Not present in v0.10.1.*
- **[required]** A final record MUST be emitted even when the run produced zero
  requests (empty benchmark / immediate termination). *v0.10.1 returns early
  with no output when `all_reports.is_empty()`.*

---

## 5. CLI behavior

**[reported]** Flags (verified in `v0.10.1` `--help`):

| Flag | Type | Behavior |
|---|---|---|
| `--stats-json` | flag | Emit NDJSON stream to stdout |
| `--stats-interval <sec>` | u64 | Interval in seconds (default `1`); help text says "requires `--stats-json`" |
| `--stats-csv` | flag | Emit CSV to stdout (out of scope here) |
| `--stats` | flag | Existing console table (unchanged, backward compatible) |

**[required]** CLI contract:

- `--stats-json` MUST be combinable with `--stats` (both may print; the NDJSON
  stream goes to stdout, the table to stdout as today — Sutomu will invoke
  `--stats-json` alone and ignore the table).
- `--stats-interval` **MUST** be rejected (clap error, non-zero exit) when
  `--stats-json` is not also passed. *v0.10.1 only documents this; it is not
  enforced.*
- `--stats-interval` MUST be a positive integer; `0` MUST be rejected.
- The stream MUST be flushed per line (line-buffered) so a consumer can read
  intervals live, not only at process exit.
- `--stats-json` MUST NOT write anything other than NDJSON to stdout (no
  progress, no banner). Diagnostics go to stderr.

---

## 6. Final-record and exit semantics

- **[reported]** Exit code `0` = run completed with **no assertion failures**.
- **[reported]** Exit code `1` = at least one assertion failure (with
  `--continue-on-assert-fail`) or an aborted run (panic on first assertion
  failure without the flag).
- **[required]** The final record MUST be emitted **before** the process exits,
  on every path: normal completion, assertion-failure exit, and early
  termination. A consumer MUST be able to treat "final record received" as the
  authoritative end-of-run signal, independent of the exit code.
- **[required]** The final record MUST carry a `"status"` field in `global`
  with one of `"completed"`, `"failed"`, `"cancelled"` (or an equivalent
  machine-readable terminal state), so Sutomu can map it to its
  `RunResultMessage.status` without inferring from the exit code. *Not present
  in v0.10.1.*
- **[required]** On early termination (signal / `--run-time` cap), the final
  record MUST reflect **partial** results (counts and percentiles over the
  requests actually completed) and MUST set `status` accordingly. It MUST NOT
  fabricate a full-duration `rps`.

---

## 7. Compatibility and versioning

- **[required]** The `--stats` console table format MUST remain unchanged for
  backward compatibility with existing consumers (including Sutomu's current
  parser).
- **[required]** The NDJSON schema MUST be versioned via the `"version"` field
  (§4.1). Consumers MUST reject unknown versions rather than guess.
- **[required]** New fields are additive; a consumer MUST ignore unknown
  fields within a known version.
- **[required]** Field names and units (ms, count, per-second) MUST be stable
  across releases. Any rename is a new schema version.
- **[required]** Document the schema in the Drill README and/or a
  `docs/stats-json.md` with the exact field table from §4.

---

## 8. Acceptance criteria

The Drill maintainers accept this RFP when all of the following hold:

1. `drill --benchmark b.yml --stats-json --stats-interval 1` emits one NDJSON
   line per interval plus a terminal `"final": true` line, all parseable by a
   strict JSON parser.
2. Every line carries `"version"`; the final line carries `"final": true` and
   a `"global"` object with `status`, `rps`, `failures_per_sec`, and
   `duration_sec`.
3. Each interval line carries a `"global"` object and per-endpoint `rps` /
   `failures_per_sec`.
4. `--stats-interval` without `--stats-json` exits non-zero with a clear error.
5. A run with zero requests still emits a final record.
6. A run terminated early (signal) emits a partial final record with
   `status: "cancelled"` (or equivalent) before exit.
7. `--stats` text output is byte-identical to the pre-change behavior for the
   same benchmark.
8. The stream is line-buffered: a consumer reading stdout sees interval lines
   before the run finishes.

---

## 9. Fixtures

Sutomu will validate against these fixtures (to be provided by Drill as
`tests/fixtures/stats-json/`):

- `basic.ndjson` — 2 endpoints, 3 intervals, final record; matches §4.3/§4.4.
- `empty.ndjson` — zero requests; must contain a final record only.
- `assert-fail.ndjson` — run with `--continue-on-assert-fail`; final record
  `status: "failed"`, exit code 1.
- `early-cancel.ndjson` — SIGTERM mid-run; final record `status: "cancelled"`,
  partial counters.
- `malformed.ndjson` — a consumer MUST be able to detect and skip a corrupt
  line without failing the whole stream (see §10).

---

## 10. Malformed / partial output behavior

- **[required]** A consumer MUST treat a non-JSON line as a stream error and
  MAY skip it, but MUST NOT abort the entire parse; the final record, if
  present, still terminates the stream.
- **[required]** If the stream ends **without** a final record (e.g. process
  killed), the consumer MUST treat the run as incomplete and MUST NOT render
  terminal charts from partial data.
- **[required]** Drill MUST NOT emit a partial final record and then continue
  emitting interval records; the final record is terminal.
- **[required]** If `--stats-json` is enabled but the run produces no data and
  no final record can be computed, Drill MUST still emit a final record with
  zeroed counters and `status` reflecting the actual outcome.

---

## 11. Sutomu integration impact

When the stream lands, Sutomu will:

1. **Replace** the `--stats` text parser in `libs/drill-stats` with a strict
   NDJSON consumer (`parse_drill_stream`), keeping the existing text parser as
   a fallback for the pinned `v0.1.0` binary.
2. **Consume interval records** to populate `RunResultMessage`/`Run` history
   and drive the live Throughput and Error-Rate time-series charts (currently
   empty).
3. **Consume the final record** to populate `Run.stats` (global) and
   `Run.endpoint_stats` (per-endpoint), replacing the current derived
   `requests_per_sec = num_requests / time_taken` approximation with Drill's
   native `rps`.
4. **Map `global.status`** to `RunResultMessage.status`
   (`completed`/`failed`/`cancelled`) and surface assertion failures as a
   terminal reason.
5. **Pin** the Drill binary to a release that ships this schema and add a
   schema-version check at startup.

---

## 7. Sutomu-side work, not Drill work

The following are explicitly **Sutomu's** responsibility and are NOT part of
this RFP:

- The NDJSON consumer/parser in `libs/drill-stats`.
- Persisting interval history and endpoint stats into `Run` / `RunResultMessage`.
- Rendering live charts and the terminal status in the web UI.
- The `--stats` text parser fallback for the pinned `v0.1.0` binary.
- Schema-version negotiation and graceful degradation when Drill is older than
  the required version.
- Upgrading the pinned Drill commit in the worker Dockerfile and `Makefile`.

---

## 8. Capability status summary

| Capability | v0.10.1 status | RFP requirement |
|---|---|---|
| `--stats-json` NDJSON flag | **[reported]** | MUST keep |
| `--stats-interval` | **[reported]** (default 1) | MUST enforce `requires --stats-json` |
| Interval records | **[reported]** | MUST add `global` + endpoint `rps`/`failures_per_sec` |
| Final record (`"final": true`) | **[reported]** | MUST add `version`, `status`, `failures_per_sec`; emit on empty/early-exit |
| `version` field | **[required]** | MUST add |
| `status` field | **[required]** | MUST add |
| Line-buffered streaming | **[required]** | MUST confirm |
| `--stats` text unchanged | **[reported]** | MUST keep |
| `--stats-csv` | **[reported]** | out of scope |

---

*Document version: 1.0 · Prepared for `sgmovea2z/drill` maintainers · 2026-08-07*
