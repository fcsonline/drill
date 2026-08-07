# Drill — Dependencies, Deficiencies & Feature Requests

> Status: living reference — every item below is a **parallelizable work unit** with an owner, acceptance criteria, and no cross-item blocking dependency unless stated.
> Source of truth for Drill: upstream `fcsonline/drill` (master `0964bded`, v0.9.1) — this repo builds the fork `sgmovea2z/drill` (same tree).
> Related docs: `docs/drill-capability-gap.md` (capability matrix), `docs/task-list.md` (product-side task tracking, T-/D-/N- prefixes).

---

## 0. TL;DR

Sutomu's only load-test engine is Drill (Rust, single-process, declarative YAML). The product loop is severed at the results step because **Drill's `--stats` text output is the only machine-readable surface**: it prints per-request summary blocks plus one global block, with no time-series, no p95, and no JSON/CSV export. The control-plane template contract (`run_detail.html`) expects a per-endpoint row shape (`name`, `avg_response_time`, `p95`, `requests_per_sec`) that Drill's flat global stats dict never produces — so charts render empty even after the T-01 data-path groundwork landed.

Sections: [1. Dependencies](#1-dependencies) · [2. Deficiencies](#2-deficiencies) · [3. Feature requests](#3-feature-requests) · [4. Parallel work streams](#4-parallel-work-streams)

---

## 1. Dependencies

### 1.1 Engine build / runtime dependencies

| Dependency | Purpose | Where pinned |
|---|---|---|
| Rust toolchain | Compiles Drill binary | Worker Dockerfile multi-stage build → `/usr/local/bin/drill` |
| `reqwest` (rustls) | HTTP client | upstream `fcsonline/drill` Cargo.toml |
| `h2` | HTTP/2, auto-negotiated (no force/disable flags) | upstream Cargo.toml |
| hdrhistogram | 99.0/99.5/99.9 percentile stats | upstream Cargo.toml |
| `postman2drill` (Rust binary) | Postman collection → benchmark conversion | worker image |
| `boto3` (Python) | R2 artifact download / result upload | `apps/worker/container_server.py` |

### 1.2 sutomu → Drill contract dependencies (what sutomu relies on from Drill)

- **Text format of `--stats` output** — the only machine-readable surface. Two parsers consume it:
  - `apps/worker/drill_runner.py::parse_drill_stats` (worker side)
  - `apps/control-plane/sutomu_orchestration/local_executor.py::_parse_drill_stats` (control-plane side, mirrors without importing across the boundary)
  - **Parsing rule**: global-block lines start with a metric label at column zero; per-request lines are prefixed with the request name. Matching `line.startswith(label)` isolates the global block and discards per-request blocks.
- **`benchmark.yml` format** — composed by `libs/translator` (`drill_generator.py`), consumed by `drill --benchmark <file> --stats`.
- **Exit code** — 0 = completed; non-zero = failed. Non-200 assertion mismatch currently aborts (panics) the run (see DD-07).
- **`--report` / `--compare <file>` / `--threshold <ms>`** — regression-gate flags exist upstream but are **unused by sutomu** (see FR-08).

### 1.3 Duplication risk (sutomu-internal)

`parse_drill_stats` (worker) and `_parse_drill_stats` (control-plane) are **two copies of the same label→key map** (`_REPORT_LABELS` in worker, inline `labels` dict in control-plane). They cannot drift — a format change must be applied twice. Candidate for a shared `libs/` parser (see FR-02).

---

## 2. Deficiencies

All items verified against the checked-in repo (fixture `tests/unit/test_drill_runner.py::SAMPLE_OUTPUT`, `local_executor.py`, `container_server.py`, `views.py`, `run_detail.html`). Severity: P0 = ship-blocking · P1 = high · P2 = medium · P3 = low. Owner: `upstream` = change the Drill engine (fork) · `control-plane` / `worker` / `translator` / `web` = change this repo.

### Output & parsing

#### DD-01 — P0 · No time-series data (charts 2/3 cannot ever render)
- **What**: Drill `--stats` emits per-request summary blocks + one global block only. There are **no periodic samples** (no per-second RPS, latency, or error-rate series) anywhere in the output.
- **Impact**: `run_detail.html` charts 2 (Throughput Over Time) and 3 (Error Rate Over Time) are gated on `results.history` — a key **no producer ever emits**. Even after T-01 ingestion lands, these charts must show an honest empty state.
- **Where**: `SAMPLE_OUTPUT` (all blocks are summaries); chart JS in `run_detail.html` L246-293 (`history.length > 0`).
- **Owner**: upstream (Drill must emit interval samples) → unblocks charts 2/3. Sutomu-side graceful degradation is a prerequisite (see FR-01).
- **Blocked by**: nothing (upstream feature request is independent of the degradation work).

#### DD-02 — P0 · No p95 percentile; template contract demands it
- **What**: Drill emits only 99.0 / 99.5 / 99.9 percentiles. `run_detail.html` results table and Chart 1 (Response Time by Endpoint) read `stat.p95` and `s.p95` — which never exist in any producer output (text parse, `stats.json`, or `Run.stats`).
- **Impact**: Chart 1's p95 dataset renders as zeros/undefined; the table's "p95 (ms)" column is blank even when stats exist.
- **Where**: `_REPORT_LABELS` (no p95 key); `run_detail.html` L79, L90, L229.
- **Owner**: upstream (emit p95) OR web (relabel p99 → keep template honest — decision required, see FR-07).
- **Blocked by**: nothing.

#### DD-03 — P0 · Per-request blocks are parsed, then discarded
- **What**: Drill prints a per-request summary block per request name (e.g. `Get  Total requests 5`), but the `startswith(label)` rule treats them as noise; `test_ignores_per_request_blocks` asserts `"Get" not in stats`.
- **Impact**: Per-endpoint latency chart (Chart 1) and per-endpoint table rows have **no data source** today. Per-endpoint rows are the primary visualization — a single global row is a regression from the intended UI.
- **Where**: `drill_runner.py::parse_drill_stats` L74-83; `local_executor.py::_parse_drill_stats` L146-181; `test_drill_runner.py` L67-71.
- **Owner**: control-plane + worker (add a per-request parser alongside the global parser — no upstream change needed, the data is already in the text).
- **Blocked by**: nothing. **Highest-value quick win.**

#### DD-04 — P1 · Console-only stats; no native JSON/CSV export
- **What**: Drill's only machine-readable output is the console table. `stats.json` on the worker is **re-serialized by sutomu code** from parsed text, not produced by Drill. Any format change (label rename, column shift) silently breaks both parsers.
- **Impact**: fragile parser dependency; no `--json`/`--csv` flag exists to harden against.
- **Where**: `container_server.py::_execute_drill_run` L114-117 (`stats.json` written from parsed dict).
- **Owner**: upstream (native export, see FR-06) — sutomu mitigation via FR-02.

#### DD-05 — P2 · Per-request blocks omit RPS and "Time taken"
- **What**: Per-request blocks contain Total/Successful/Failed/Median/Average/Stddev/99.x/Max — but **no "Requests per second"** and no "Time taken for tests" lines. Only the global block has RPS.
- **Impact**: per-endpoint `requests_per_sec` column (template L92) has no per-endpoint source; must either be omitted per-row or derived (requests / time_taken) — a derivation, not a direct read.
- **Owner**: upstream (emit per-request RPS) OR web (drop/derive the column).
- **Blocked by**: DD-03 parser must land first if the column is kept.

### Lifecycle

#### DD-06 — P0 · No stop/cancel path for Drill runs (T-02)
- **What**: `stop_run` (local) and the worker `/run` endpoint have no Drill-aware cancellation. `stop_locust_run` only understands Locust PID files.
- **Where**: `local_executor.py::stop_run` L282-306; `container_server.py` (no stop endpoint); run-launcher payload.
- **Owner**: worker + control-plane. Tracked as T-02.
- **Blocked by**: nothing.

#### DD-07 — P1 · Assertion mismatch aborts the whole benchmark (T-08)
- **What**: generated `assert {status:200}` **panics and aborts** on the first non-200; drill exits non-zero and the run fails wholesale. Locust counted failures and continued.
- **Where**: translator-emitted `assert` in `drill_generator.py`; upstream panic semantics.
- **Owner**: upstream (count-don't-abort, FR-05) and/or translator (stop emitting `assert`, rely on failure counts).
- **Blocked by**: nothing.

#### DD-08 — P2 · Duration semantics are approximate (T-09)
- **What**: `iterations = vus * duration_seconds`; no wall-clock bound — slow requests extend the run.
- **Where**: `drill_generator.py`; upstream has no `--run-time` mode.
- **Owner**: upstream (FR-04) or translator refinement.

### Protocol fidelity

#### DD-09 — P1 · No JA3/TLS fingerprint impersonation (D-02, user-owned)
- **What**: rustls only; no fingerprint support. Product claims browser-like profiles (see T-18).
- **Owner**: upstream fork work (in progress, user-owned).

#### DD-10 — P1 · No network profile latency/jitter shaping (D-01, user-owned)
- **What**: no latency model; only fixed per-item `delay`. `SUTOMU_NETWORK_PROFILE` env is set by the executor but ignored by Drill.
- **Owner**: upstream fork work (in progress, user-owned).

#### DD-11 — P2 · No per-VU state; context resets each iteration (T-10)
- **What**: fresh context per iteration; cookies/tokens captured in iteration N are gone in N+1.
- **Owner**: upstream (FR-03) or documented limitation.

#### DD-12 — P3 · Success = 2xx only; connection errors counted as 520
- **What**: "Successful" is 2xx-only; connection failures surface as status 520 in stats rather than a clean failure classification.
- **Owner**: upstream.

### Integration (this repo)

#### DD-13 — P1 · R2 results are orphaned (T-01 core)
- **What**: worker uploads `results/{run_id}/stats.json` + `output.log` to R2; **control plane never reads R2 back** — `get_run_results` only reads the local filesystem, `Run.stats`, or a JSON blob in `metrics_dashboard_url`. `metrics_dashboard_url` is never set for cloudflare-executed Drill runs.
- **Where**: `container_server.py::_upload_results` L51-62; `local_executor.py::get_run_results` L317-337; `drain_results_queue.py` (T-01 groundwork committed: `RunResultMessage.stats` → `Run.stats`).
- **Owner**: control-plane. Tracked as T-01. The committed data path (message stats → `Run.stats` JSONField, migration `0014`, serializer exposure, `/results` include) covers the **global stats** case; the per-endpoint shape (DD-03) and time-series (DD-01) are the remaining gaps.
- **Blocked by**: DD-03 parser decision (shape of what gets persisted).

#### DD-14 — P1 · Template key contract mismatches producer keys
- **What**: template/chart expect per-endpoint rows `{name, num_requests, avg_response_time, p95, num_failures, requests_per_sec}`; producers emit a flat dict `{total_requests, average_time_ms, p99_ms, failed_requests, requests_per_second, …}`. `_normalize_results` is a pass-through, so today a single global row renders with blank name/p95/RPS cells.
- **Where**: `views.py::_normalize_results` L40-42; `run_detail.html` table L73-97 + Chart 1 L211-243.
- **Owner**: control-plane (normalize Drill keys → template keys) + web (DD-02/DD-05 column decisions).
- **Blocked by**: DD-03 (per-endpoint rows) + DD-02 (p95) + DD-05 (RPS) decisions.

---

## 3. Feature requests

Each FR is an independent unit with acceptance criteria. Owner: `upstream` = change `sgmovea2z/drill` fork (user-owned); `sutomu` = this repo.

### FR-01 — (sutomu) Graceful degradation for missing time-series
- **What**: run_detail renders charts 2/3 as an honest empty state ("no time-series data") when `history` is absent, instead of blank canvases; chart 1 renders from per-endpoint stats.
- **Acceptance**: run_detail with real global stats shows populated Chart 1 + table; charts 2/3 show a clear "not available for this run" message; no JS errors.
- **Blocked by**: DD-03 parser (for chart 1 data).

### FR-02 — (sutomu) Shared Drill stats parser in `libs/`
- **What**: extract the label→key map + per-request/global block parsing into `libs/` so worker and control-plane import one implementation (kills the 1.3 duplication).
- **Acceptance**: worker + control-plane import the same parser; existing `test_drill_runner.py` + `test_run_execution.py` assertions pass unchanged.
- **Blocked by**: nothing (can land before or with DD-03).

### FR-03 — (upstream) Persist context across iterations / VU isolation
- **Acceptance**: cookie/session state captured in iteration N is available in N+1 (or a documented, explicit opt-in mode).
- **Blocked by**: nothing.

### FR-04 — (upstream) Wall-clock duration cap (`--run-time`-like)
- **Acceptance**: run stops at the configured wall-clock bound regardless of per-request latency.
- **Blocked by**: nothing.

### FR-05 — (upstream) Non-aborting assertion semantics
- **Acceptance**: assertion mismatch records a failure and the run continues; exit code reflects pass/fail without aborting mid-run.
- **Blocked by**: nothing.

### FR-06 — (upstream) Native machine-readable stats export (JSON/CSV)
- **Acceptance**: `drill --stats-json` (or equivalent) emits global + per-request blocks (incl. p95 if landed) as JSON, removing the console-text parser dependency.
- **Blocked by**: nothing. High leverage: resolves DD-04 and hardens DD-03/DD-14.

### FR-07 — (web, decision) p95 vs p99 column
- **What**: either map Drill's p99_ms → template p95 label honestly renamed, or request p95 upstream. Decide before Chart 1 polish.
- **Acceptance**: table column header matches the value shown.
- **Blocked by**: nothing (pure UI decision).

### FR-08 — (sutomu) Wire Drill CI regression gate (`--report`/`--compare`/`--threshold`)
- **What**: upstream already supports historical comparison with regression thresholds (exit 1 on regressions). Sutomu never passes these flags.
- **Acceptance**: scheduled runs can attach a compare file + threshold; regression exits surface as a run status/terminal reason.
- **Blocked by**: nothing (independent of results loop; complements N-04 CI/CD integration).

---

## 4. Parallel work streams

Independent tracks — safe to run concurrently. Items inside a track are sequential.

| Track | Owner | Items | Depends on |
|---|---|---|---|
| **A. Per-endpoint results** | control-plane + worker | DD-03 parser → persist per-endpoint shape → normalize in `_normalize_results` → Chart 1/table render (FR-01) | — |
| **B. Data path (T-01)** | control-plane | DD-13 R2 read-back / message stats → `metrics_dashboard_url` → `/results` API → compare wiring | — (committed groundwork already covers global stats) |
| **C. Engine features** | upstream (fork, user-owned) | FR-03, FR-04, FR-05, FR-06 (in-progress D-01/D-02 join here) | — |
| **D. UI honesty** | web | DD-02 p95 column decision (FR-07), DD-05 RPS column, DD-01 empty-state polish | Track A output |
| **E. Shared parser** | libs | FR-02 consolidation | — (can run before/alongside A) |
| **F. CI gate** | control-plane | FR-08 | — |

Suggested parallel start: **A + B + C + E** (all independent), then **D** after A.
