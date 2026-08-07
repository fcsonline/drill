# Drill RFP — Feasibility Analysis & Detailed Tasks

> Generated from `drill_rfp_stats.md` — living reference for implementation planning.
> Source of truth for Drill: upstream `fcsonline/drill` (master `0964bded`, v0.9.1) — this repo builds the fork `sgmovea2z/drill` (same tree).

---

## Executive Summary

| Category | Count | Feasible in Drill | Need External (sutomu) | Blocked / Needs Clarification |
|---|---|---|---|---|
| Dependencies | 2 | — | — | — |
| Deficiencies (DD) | 14 | 10 | 4 | 3 |
| Feature Requests (FR) | 8 | 5 | 3 | 2 |

**Key finding**: The Drill codebase already produces rich per-endpoint stats with p95, RPS, and time-series data **internally** (in `results.rs` CSV/HTML export), but **only emits limited summary text via `--stats`**. Most DD items are solved by exposing existing internal data via new CLI flags (`--stats-json`, `--stats-interval`) rather than new computation.

---

## 1. Feasibility Matrix by RFP Item

### Dependencies
| ID | Item | Status | Notes |
|---|---|---|---|
| 1.1 | Build/runtime deps | ✅ Verified | `tokio` now has `rt-multi-thread` added (v0.9.2) |
| 1.2 | sutomu→Drill contract | ⚠️ Clarify | Parser depends on text format; see FR-02, FR-06 |
| 1.3 | Parser duplication | ✅ Fixable | Shared `libs/` parser (FR-02) kills drift |

### Deficiencies
| ID | Title | Feasibility | Implementation Path | Blockers |
|---|---|---|---|---|
| **DD-01** | No time-series data | ✅ **High** | Add `--stats-interval N` to emit periodic JSON lines (reuse timestamp + hdrhistogram buckets) | Needs FR-06 (JSON export) first |
| **DD-02** | No p95 percentile | ✅ **Trivial** | `hdrhistogram` already supports arbitrary quantiles; emit `value_at_quantile(0.95)` in console + JSON | Decision: rename p99→p95 (FR-07) or add p95 |
| **DD-03** | Per-request blocks parsed then discarded | ✅ **High** | Parser change in sutomu only (data already in text); or add `--stats-json` for structured per-endpoint | None — quick win |
| **DD-04** | Console-only stats, no JSON/CSV | ✅ **High** | Add `--stats-json` flag emitting full Stats struct (global + per-endpoint) | None |
| **DD-05** | Per-request blocks omit RPS | ✅ **Trivial** | RPS = `total / duration` per endpoint; already computed in `results.rs` | None |
| **DD-06** | No stop/cancel path | ⚠️ **Medium** | Requires signal handling + graceful shutdown in tokio runtime | External orchestrator (sutomu worker) must send signal |
| **DD-07** | Assertion aborts benchmark | ✅ **High** | Change `assert` action to record failure + continue (non-zero exit at end) | May need config flag for backward compat |
| **DD-08** | Duration semantics approximate | ⚠️ **Medium** | Add `--run-time` wall-clock cap (like Locust) | Interacts with `iterations`/`load_shape` |
| **DD-09** | No JA3/TLS fingerprint | 🔴 **External** | Requires rustls fork / `boringssl` integration | User-owned (D-02) |
| **DD-10** | No network profile shaping | 🔴 **External** | Requires proxy / tc integration or custom transport | User-owned (D-01) |
| **DD-11** | No per-VU state / context reset | ✅ **Medium** | Add `persist_context: true` config to carry cookies/vars across iterations | Design: opt-in vs default |
| **DD-12** | Success = 2xx only | ✅ **Trivial** | Change classification or add `success_codes` config | Low impact |
| **DD-13** | R2 results orphaned | ⚠️ **External** | sutomu control-plane must read R2 → `metrics_dashboard_url` | Depends on DD-03 shape decision |
| **DD-14** | Template key mismatch | ✅ **Fixable** | `_normalize_results` maps Drill keys → template keys | Blocked by DD-02/DD-05/DD-03 |

### Feature Requests
| ID | Title | Feasibility | Implementation Path | Blockers |
|---|---|---|---|---|
| **FR-01** | Graceful degradation (time-series) | ✅ **High** | sutomu web side: render "no data" when `history` absent | DD-03 parser first |
| **FR-02** | Shared parser in `libs/` | ✅ **High** | Extract label→key map + block parsing to shared crate | None |
| **FR-03** | Persist context across iterations | ✅ **Medium** | Add `persist_context` config; don't clear context between iterations | Design: per-VU vs global |
| **FR-04** | Wall-clock duration cap | ✅ **Medium** | Add `--run-time` flag; track start time; stop accepting new iterations | Interacts with `load_shape` |
| **FR-05** | Non-aborting assertions | ✅ **High** | Change `assert` action: record failure, don't panic; exit code at end | Config flag for compat |
| **FR-06** | Native JSON/CSV export | ✅ **High** | Add `--stats-json` (streaming JSON Lines) + `--stats-csv` | High leverage — unblocks DD-01/DD-04 |
| **FR-07** | p95 vs p99 column decision | 🟡 **Decision** | UI decision: map p99→p95 label OR request p95 upstream | Pure UI — decide before Chart 1 polish |
| **FR-08** | Wire regression gate (`--compare`) | ✅ **High** | Pass `--compare` + `--threshold` in scheduled runs; surface exit 1 as run status | Independent |

---

## 2. Clarifications Required

| # | Question | Impact | Suggested Resolution |
|---|---|---|---|
| **C-01** | **p95 vs p99**: Should Drill emit p95 (easy) or should sutomu template map p99→p95? | FR-07, DD-02, DD-14 | **Decide now**. Recommendation: emit both p95 and p99 in JSON; console keeps p99. Template uses p95 from JSON. |
| **C-02** | **Time-series granularity**: What interval? 1s? Configurable? | DD-01, FR-06 | Add `--stats-interval <seconds>` (default 1). Emit JSON Lines per interval. |
| **C-03** | **Per-VU vs global context persistence**: FR-03 says "persist context across iterations". Is this per-VU (each concurrent user keeps cookies) or global (shared)? | FR-03, DD-11 | **Clarify**. Per-VU matches Locust semantics; global is simpler but less realistic. |
| **C-04** | **Assertion behavior flag**: Non-aborting assertions (FR-05) — should this be default or opt-in via `--continue-on-assert-fail`? | FR-05, DD-07 | **Recommend opt-in flag** for backward compat; default keeps current panic behavior. |
| **C-05** | **Wall-clock vs iterations precedence**: If both `--run-time` and `iterations` set, which wins? | FR-04, DD-08 | **Recommend**: `--run-time` caps wall time; `iterations` caps count. Stop on first reached. |
| **C-06** | **JSON output format**: Single JSON object at end, or streaming JSON Lines per interval? | FR-06, DD-01 | **Recommend JSON Lines** (one per interval + final summary) — enables real-time parsing. |
| **C-07** | **Per-request RPS in console**: Emit "Requests per second" line in per-endpoint console blocks? | DD-05 | **Yes** — trivial compute (`total / duration` per endpoint). |
| **C-08** | **Signal handling for cancel**: Which signals? SIGTERM? SIGINT? Graceful drain in-flight requests? | DD-06 | **SIGTERM** = stop accepting new iterations, wait for in-flight (configurable timeout). |

---

## 3. Detailed Task Breakdown

### Track A: Per-Endpoint Results (Control-Plane + Worker)
**Owner**: sutomu team | **Depends on**: — | **Parallel**: Yes

| Task | Description | Acceptance Criteria | Effort |
|---|---|---|---|
| **A-01** | Add per-request parser in shared `libs/drill-parser` | Parses both global + per-endpoint blocks from `--stats` text; unit tests pass | S |
| **A-02** | Replace worker `parse_drill_stats` with shared lib | Worker imports `libs/drill-parser`; existing tests unchanged | S |
| **A-03** | Replace control-plane `_parse_drill_stats` with shared lib | Control-plane imports `libs/drill-parser`; existing tests unchanged | S |
| **A-04** | Persist per-endpoint shape to `Run.stats` JSONField | `Run.stats` contains `{endpoints: [{name, total, avg_ms, p95_ms, rps, failures}, ...], global: {...}}` | M |
| **A-05** | Update `_normalize_results` to map Drill keys → template keys | Template receives per-endpoint rows with `name`, `avg_response_time`, `p95`, `requests_per_sec` | S |
| **A-06** | Chart 1 (Response Time by Endpoint) renders from per-endpoint data | Chart shows bar per endpoint with p95/avg; table populated | M |

---

### Track B: Data Path / T-01 (Control-Plane)
**Owner**: sutomu team | **Depends on**: A-04 (shape) | **Parallel**: Yes (global stats path done)

| Task | Description | Acceptance Criteria | Effort |
|---|---|---|---|
| **B-01** | Worker uploads `stats.json` (from `--stats-json`) to R2 | Cloudflare worker writes per-run JSON to R2 | S |
| **B-02** | Control-plane reads R2 on `get_run_results` | `GET /api/runs/:id/results` returns per-endpoint + global + history | M |
| **B-03** | Set `metrics_dashboard_url` for Drill runs | Links to R2 object or `/results` endpoint | S |
| **B-04** | Wire regression compare in scheduled runs | Pass `--compare` + `--threshold`; surface exit 1 as run failure | M |

---

### Track C: Engine Features (Upstream Fork — sgmovea2z/drill)
**Owner**: Drill fork maintainer | **Depends on**: — | **Parallel**: Yes

| Task | Description | Acceptance Criteria | Effort |
|---|---|---|---|
| **C-01** | **FR-06**: Add `--stats-json` flag (streaming JSON Lines) | Emits per-interval lines + final summary; includes global + per-endpoint + p95 + RPS + TTFB | M |
| **C-02** | **FR-06**: Add `--stats-csv` flag | Emits same data as CSV to stdout | S |
| **C-03** | **DD-01/FR-06**: Add `--stats-interval <sec>` for time-series | With `--stats-json`, emits JSON Lines every N seconds with interval histogram deltas | M |
| **C-04** | **DD-02**: Emit p95 in console `--stats` and JSON | Console: add "95.0'th percentile" line; JSON: `p95_ms` field | S |
| **C-05** | **DD-05**: Emit per-endpoint RPS in console `--stats` | Add "Requests per second" line in per-endpoint blocks | S |
| **C-06** | **FR-05**: Non-aborting assertions (`--continue-on-assert-fail`) | Assertion failure records failure, continues; exit code 1 if any assertion failed | M |
| **C-07** | **FR-04**: Wall-clock cap `--run-time <seconds>` | Stops accepting new iterations at wall-clock bound; waits for in-flight (timeout configurable) | M |
| **C-08** | **FR-03**: Persist context across iterations (`persist_context: true`) | Cookies/vars from iteration N available in N+1; opt-in config | M |
| **C-09** | **DD-06**: Signal handling for graceful cancel | SIGTERM → stop new iterations, drain in-flight (configurable timeout), emit partial stats | M |
| **C-10** | **DD-12**: Configurable success codes | `success_codes: [200, 201]` in benchmark.yml; default 2xx | S |
| **C-11** | **DD-08**: Duration semantics fix (linked to C-07) | `--run-time` provides wall-clock bound | — |

---

### Track D: UI Honesty (Web)
**Owner**: sutomu web team | **Depends on**: Track A output | **Parallel**: After A

| Task | Description | Acceptance Criteria | Effort |
|---|---|---|---|
| **D-01** | **FR-07**: p95 column decision | Decision recorded; template uses correct field | XS |
| **D-02** | Chart 1 renders per-endpoint bars (p95/avg) | From `Run.stats.endpoints` | M |
| **D-03** | Chart 2/3 empty state: "No time-series data for this run" | When `history` absent, show message not blank canvas | S |
| **D-04** | Per-endpoint table with RPS column | Uses `requests_per_sec` from normalized data | S |

---

### Track E: Shared Parser (libs/)
**Owner**: sutomu team | **Depends on**: — | **Parallel**: Yes (can run before/alongside A)

| Task | Description | Acceptance Criteria | Effort |
|---|---|---|---|
| **E-01** | Create `libs/drill-parser` crate | Exports `parse_drill_stats(text) -> DrillStats { global, endpoints[] }` | S |
| **E-02** | Add JSON parser for `--stats-json` output | `parse_drill_json(lines) -> DrillStats` | S |
| **E-03** | Worker + control-plane migrate to shared lib | Both import `drill-parser`; tests pass | S |

---

### Track F: CI Regression Gate
**Owner**: sutomu control-plane | **Depends on**: — | **Parallel**: Yes

| Task | Description | Acceptance Criteria | Effort |
|---|---|---|---|
| **F-01** | **FR-08**: Pass `--compare` + `--threshold` in scheduled runs | Uses baseline `stats.json` from previous run | M |
| **F-02** | Surface exit code 1 as "regression detected" run status | Run detail shows regression reason | S |

---

## 4. Implementation Priority & Sequencing

### Phase 0 — Decisions (Week 0)
- [ ] **C-01**: p95 vs p99 column decision (FR-07)
- [ ] **C-02**: Time-series interval default (DD-01)
- [ ] **C-03**: Per-VU vs global context (FR-03)
- [ ] **C-04**: Assertion flag default (FR-05)
- [ ] **C-05**: JSON Lines vs single object (FR-06)

### Phase 1 — Quick Wins (Week 1-2) — Independent, High Impact
| Task | Why First |
|---|---|
| **E-01, E-02, E-03** | Shared parser kills duplication; enables A track |
| **C-04** (p95 in console/JSON) | Trivial; unblocks DD-02, DD-14 |
| **C-05** (per-endpoint RPS in console) | Trivial; unblocks DD-05, DD-14 |
| **C-01** (`--stats-json`) | High leverage; unblocks DD-01, DD-04, DD-13, B track |
| **C-02** (`--stats-csv`) | Low effort; alternative machine-readable format |

### Phase 2 — Core Engine Features (Week 2-4)
| Task | Dependencies |
|---|---|
| **C-03** (`--stats-interval`) | Requires C-01 (`--stats-json`) |
| **C-06** (non-aborting assertions) | Independent |
| **C-07** (`--run-time`) | Independent |
| **C-08** (persist context) | Independent |
| **C-09** (signal handling) | Independent |
| **C-10** (success codes) | Independent |

### Phase 3 — Integration & UI (Week 3-5)
| Task | Dependencies |
|---|---|
| **A-01 → A-06** (per-endpoint pipeline) | E-01, C-01, C-04, C-05 |
| **B-01 → B-04** (data path) | A-04 |
| **D-01 → D-04** (UI) | A-05 |
| **F-01, F-02** (CI gate) | C-01, existing `--compare` |

---

## 5. Code Pointers for Implementers

| Area | File(s) | Notes |
|---|---|---|
| Console `--stats` output | `src/main.rs::show_stats()` | Add p95, per-endpoint RPS, `--stats-json` flag dispatch |
| JSON export | New: `src/stats_export.rs` | Emit `Stats` struct (from `results.rs`) as JSON Lines |
| Time-series interval | `src/benchmark.rs::run_iteration` | Track wall time; emit interval histograms |
| Assertions | `src/actions/assert.rs` | Change panic → record failure |
| Context persistence | `src/benchmark.rs::run_iteration` | Don't recreate context; carry cookies/vars |
| Signal handling | `src/main.rs` + `src/benchmark.rs` | Tokio signal handler + shutdown flag |
| Results/CSV/HTML | `src/results.rs` | Already has p95, RPS, TTFB, per-endpoint — reuse! |
| Shared parser | New: `libs/drill-parser/` | Parse both text and JSON formats |

---

## 6. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| JSON format changes break parsers | Medium | High | Version JSON schema (`"version": 1`); shared parser validates |
| `--run-time` vs `load_shape` conflict | Medium | Medium | Document precedence; test both combinations |
| Per-VU context memory growth | Low | Medium | Cap context size; LRU eviction for cookies |
| Signal handling on Windows | Medium | Low | Use `ctrlc` crate; fallback to poll-based check |
| Backward compat for `--stats` text | High | High | Keep existing text format; add new flags only |
| R2 read latency in control-plane | Low | Medium | Cache `Run.stats` in DB; async R2 fetch |

---

## 7. Acceptance Test Scenarios

| Scenario | RFP Items Covered |
|---|---|
| **S-01**: Run with `--stats-json --stats-interval 1` → JSON Lines every 1s + final summary with per-endpoint p95/RPS | DD-01, DD-02, DD-04, DD-05, FR-06 |
| **S-02**: Run with `--continue-on-assert-fail` + failing assert → records failure, continues, exits 1 | DD-07, FR-05 |
| **S-03**: Run with `--run-time 30` + slow endpoint (5s) → stops at ~30s, emits partial stats | DD-08, FR-04 |
| **S-04**: Run with `persist_context: true` → cookies from iter 0 valid in iter 1 | DD-11, FR-03 |
| **S-05**: SIGTERM sent mid-run → stops new iterations, drains in-flight (5s), emits stats | DD-06 |
| **S-06**: Scheduled run with `--compare baseline.json --threshold 100` → exits 1 on regression | FR-08 |
| **S-07**: `run_detail.html` with per-endpoint stats → Chart 1 populated, charts 2/3 show empty state | DD-03, FR-01, Track A, D |

---

## 8. Open Questions for Stakeholders

1. **p95 vs p99**: What does the product team prefer — Drill emits p95, or template renames p99?
2. **Per-VU vs global context**: Which matches the product's "virtual user" mental model?
3. **Assertion default**: Should non-aborting be the new default (breaking change) or opt-in?
4. **Time-series retention**: How many intervals to keep in memory? (Affects `--stats-interval` memory)
5. **Windows signal support**: Is graceful cancel required on Windows runners?

---

*Document version: 1.0 | Generated from RFP analysis | Update after each phase*