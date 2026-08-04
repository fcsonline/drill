# Drill vs Locust — Capability Gap Analysis

> Status: analysis (no implementation)
> Scope: gap inventory for closing the capability gap between the two load-test engines in sutomu.
> Source of truth for Drill: upstream `fcsonline/drill` (master `0964bded`, v0.9.1) — this repo builds a fork `sgmovea2z/drill` (same tree). Locust side: as actually implemented in this repo (verified at `main`/`dfd2dfb`), not Locust's theoretical feature set.

---

## 0. TL;DR

Drill is a **single-process, iteration-driven, declarative-YAML** HTTP load tester. Locust, as used in sutomu, is a **Python-scriptable, per-VU-isolated, CSV-reporting** load tester. Drill is weaker on almost every axis that matters for the product (isolation, scripting, lifecycle, protocol fidelity, observability, control), with a handful of genuinely missing items that are cheap to add.

**The one structural mismatch to fix first**: drill runs complete as opaque `completed`/`failed` with a parsed `stats.json`, but the control plane **never consumes that stats JSON** — `metrics_dashboard_url` is only set on the Locust path, and the `/results` endpoint returns no drill statistics. Drill results currently vanish after the worker uploads them to R2.

---

## 1. Engine identity (correcting the premise)

| | Drill | Locust |
|---|---|---|
| Author / repo | **`fcsonline/drill`** (NOT `furio/drill` — that repo does not exist) | `locustio/locust` |
| Language | **Rust** (NOT Go) | Python |
| Runtime used by sutomu | `sgmovea2z/drill` fork (same tree, built in worker Dockerfile multi-stage) | Locust ≥ 2.32 via pip |
| Execution model | Single process, async tasks, iteration-driven | Single process (standalone) or distributed (master/worker) |
| Test definition | Declarative `benchmark.yml` | Python `HttpUser` class (generated or hand-authored) |
| License | GPL-3.0 | MIT |

---

## 2. Current sutomu wiring (per engine)

### 2.1 Locust (complete)
- **Runtime** (`libs/locust-runtime/`): `MobileHttpUser` + curl_cffi TLS/JA3 impersonation; `SUTOMU_NETWORK_PROFILE` latency/jitter sleep; `VuVariableStore` per-VU (greenlet-scoped) isolation; `pm.*` Postman shim.
- **Translator** (`libs/translator/`): Postman v2.1 JSON → `LocustHttpUser` source with per-request `@task`, headers, raw body, 4xx/5xx failure checks; raw `.py` upload accepted; compatibility report produced and stored.
- **Execution** (`apps/control-plane/sutomu_orchestration/`): `execute_locust_run` runs headless `locust --users N --run-time Ds --csv=results`; `stop_locust_run` SIGTERMs via PID file; `ScheduledRun` one-off/hourly/daily/weekly; queue backends (local / Cloudflare); artifact upload.
- **Results**: locust CSV files parsed post-run → `FinalRunResult`; `metrics_dashboard_url` set to `results_stats.csv`; web `run_detail` renders normalized stats/failures; `/results` API returns dashboard URL.
- **Worker** (`apps/worker/container_server.py`): HTTP `/run` endpoint; locust subprocess; R2 artifact download + result upload; validation (vus 1-1000, duration 1-600).
- **Distributed**: `eks` backend + Locust Operator are **DOC-ONLY** in this repo (`deploy/helm|terraform` contain only READMEs/AGENTS.md); the only wired non-local path is Cloudflare Containers (single-process locust in a container).

### 2.2 Drill (as shipped in Waves 1-2)
- **Translator**: `DrillPlugin` — Postman collection → `benchmark.yml` (`concurrency=vus`, `base=target_host`, `iterations=vus*duration_seconds`, one `request` + one status-200 `assert` per request); warns on pm scripts/events and non-raw bodies.
- **Worker**: multi-stage Dockerfile builds drill binary → `/usr/local/bin/drill`; `drill_runner.py` runs `drill --benchmark benchmark.yml --stats`, parses the global stats block into a `stats.json` dict; `container_server._execute_drill_run` writes stats.json, uploads run dir to R2, returns `completed`/`failed`.
- **Run-launcher**: validates `engine ∈ {locust, drill}`, forwards `engine` to container payload.
- **Control plane**: `engine` field on `Run`/`ScheduledRun`; scheduler passes `engine` into `RunRequestMessage`.
- **Stop**: drill runs have **no stop path** — `RunViewSet.stop` / web `run_stop` call `stop_locust_run`, which only knows locust PID files.
- **Results**: `stats.json` uploaded to R2; **nothing in the control plane reads it back**; `metrics_dashboard_url` is never set for drill runs.

---

## 3. Capability comparison matrix

Legend: ✅ wired & works · ⚠️ partial / schema-only / doc-only · ❌ absent.

| # | Capability | Locust (sutomu) | Drill | Notes |
|---|---|---|---|---|
| 1 | Per-VU state isolation | ✅ `VuVariableStore` (greenlet-scoped, get/set/has/clear/all) | ❌ context is **per-iteration**, shared across all tasks; no VU concept | Drill's `concurrency` is async-task fan-out, not VUs |
| 2 | Scriptable test logic | ✅ Python tasks; `pm.*` shim (`pm.test`, `pm.expect`, `pm.response`, variables) | ❌ declarative YAML only; escape hatch is `exec:` shell commands | Drill cannot do loops/conditionals/derived values beyond interpolation |
| 3 | TLS/JA3 impersonation | ✅ curl_cffi `impersonate=chrome120` default, per-VU `mobile_profile` override | ❌ rustls only; no JA3/fingerprint support (grep: zero hits) | **Biggest protocol-fidelity gap** |
| 4 | Network profile (latency/jitter) | ✅ `SUTOMU_NETWORK_PROFILE` → per-request latency sleep (4g/3g/slow_3g) | ❌ no latency model; only fixed `delay: {seconds}` per plan item | Profile is a product-level concept; drill has nothing |
| 5 | Network profile (bandwidth caps) | ⚠️ `curl_options_for_profile` defined but **not called** | ❌ | Locust bandwidth caps are dead code today |
| 6 | Duration-bound runs | ✅ `--run-time Ds` | ❌ iterations only (`iterations` × plan); no soak/duration mode | Generator approximates via `iterations = vus*duration` — no hard wall-clock |
| 7 | Rate control (RPS limit) | ❌ not exposed (spawn-rate = VUs, no pacing) | ❌ no `rate-limit`/`rps` key (grep zero hits) | Parity — neither does it |
| 8 | Ramp-up | ⚠️ instant spawn (`--spawn-rate <vus>`), `RAMPING` status never set | ⚠️ `rampup` linearly staggers iteration starts only; no ramp-down | Different semantics; both weak |
| 9 | Request methods | ✅ any method via Python/requests/curl_cffi | ⚠️ GET/POST/PUT/PATCH/DELETE/HEAD only (no OPTIONS/TRACE) | |
| 10 | Request body modes | ⚠️ raw only (translator warns on other modes) | ⚠️ raw/hex/file, POST/PATCH/PUT only; GET body silently dropped | Parity-ish |
| 11 | Response assertions | ⚠️ generated code checks `>=400 → failure`; `pm.expect` shim exists for hand-authored | ⚠️ `assert` = string `==`; **mismatch panics and aborts the whole run** | Drill asserts abort; locust counts failures |
| 12 | Regex/JSONPath extraction | ✅ Python `re`/`json` in tasks | ❌ JSON-pointer paths into assigned body only (no regex) | |
| 13 | Cookie/session chaining | ✅ per-VU cookie jars (via curl_cffi session) | ⚠️ Set-Cookie → context → Cookie header, per iteration | Drill context is per-iteration; cookies do not persist across iterations |
| 14 | Variable persistence across iterations | ✅ per-VU store persists for the VU's life | ❌ fresh context each iteration | Drives session-replay differences |
| 15 | Data-driven params | ⚠️ not in translator output | ✅ `with_items`, `with_items_range`, `with_items_from_csv`, `with_items_from_file` | Drill advantage |
| 16 | Plan reuse / includes | ❌ | ✅ `include:` (recursive, relative paths) | Drill advantage |
| 17 | Tags / task selection | ❌ | ✅ `--tags`/`--skip-tags`, `always`/`never` | Drill advantage |
| 18 | Delay / think time | ✅ `time.sleep` in Python | ✅ `delay: {seconds}` (fixed) | |
| 19 | WebSockets / gRPC / non-HTTP | ✅ Python can use any client | ❌ HTTP(S) only | |
| 20 | Digest/NTLM/OAuth auth helpers | ✅ Python + requests auth | ❌ manual headers only (Basic via header example) | |
| 21 | HTTP/2 control | ✅ curl_cffi impersonation sets fingerprint incl. h2 | ⚠️ `h2` crate present, auto-negotiated; **no flags to force/disable** | |
| 22 | TLS cert checks | ✅ curl_cffi options | ⚠️ `--no-check-certificate` only; no client certs/CA/SNI | |
| 23 | Per-request-name latency percentiles | ✅ Locust stats | ✅ 99.0/99.5/99.9 global + per-name (hdrhistogram) | Parity |
| 24 | Stats export (JSON/CSV) | ✅ CSV files (`results_stats.csv`, `results_failures.csv`) | ❌ console only; `--stats` prints a table; report mode writes per-request YAML | **sutomu's drill parser depends on parsing console text** |
| 25 | Historical/comparative gate | ❌ | ✅ `--report` + `--compare <file>` + `--threshold <ms>` (exits 1 on regressions) | Drill advantage |
| 26 | Success definition | ✅ per-request success/failure events (non-2xx → failure via generator) | ⚠️ "Successful" = 2xx only; connection errors counted as 520 | |
| 27 | Status lifecycle (RAMPING/COOLDOWN/VALIDATING) | ⚠️ defined in model but never set by executor | ❌ N/A | Locust partial; drill N/A |
| 28 | Stop/cancel mid-run | ✅ `stop_locust_run` SIGTERM (local); API/web wired | ❌ **no stop path at all** | **Operational gap** |
| 29 | Results surfaced to control plane | ✅ CSV → `metrics_dashboard_url` → `/results` + `run_detail` | ❌ **stats.json never consumed; `/results` returns nothing for drill** | **The biggest integration gap** |
| 30 | VictoriaMetrics streaming | ❌ `MetricEnvelope` schema-only, no producer | ❌ | Parity — neither streams |
| 31 | Distributed/multi-node | ⚠️ `eks` backend DOC-ONLY; Cloudflare container = single process | ❌ single process | Parity in practice |
| 32 | Scheduler integration (ScheduledRun) | ✅ | ⚠️ `engine` field forwarded on ScheduledRun; drill scheduled runs produce drill benchmark | Wiring exists; runtime untested end-to-end |
| 33 | Live status polling / HTMX UI | ✅ `run_status` JSON+HTMX, progress %, elapsed | ⚠️ same UI shows run object (status/elapsed) but **no drill stats** | |

---

## 4. Gap catalog (what to close, in priority order)

### P0 — Drill results never reach the product
- **What**: worker parses drill `stats.json` and uploads it to R2; control plane never reads it. `/results` API returns `{run_id, status, test_name, terminal_reason, metrics_dashboard_url}` with `metrics_dashboard_url` empty for drill. Web `run_detail` renders `get_run_results` (locust CSVs) — nothing for drill.
- **Where**: `apps/worker/container_server.py` `_execute_drill_run` (writes stats.json); `apps/control-plane/sutomu_api/views.py` `results`; `apps/control-plane/sutomu_orchestration/local_executor.py` `get_run_results`; web `sutomu_web/views.py` `run_detail`.
- **Close with**: a result-contract path for drill (persist `stats.json` under `Run.metrics_dashboard_url` as JSON blob, mirroring locust's `{...}` JSON fallback at `local_executor.py:259`), extend `/results` to return drill stats, and render them in `run_detail` (normalize drill keys → same template keys: `requests_per_second`, `p99_ms`, etc.).

### P1 — No stop/cancel for drill runs
- **What**: `RunViewSet.stop` and web `run_stop` → `stop_locust_run` (PID-file SIGTERM, locust-specific). A running drill container is not cancelled.
- **Where**: `apps/control-plane/sutomu_orchestration/local_executor.py:217`; `apps/worker/container_server.py` (no stop endpoint); run-launcher payload.
- **Close with**: extend the worker `/run` contract with a stop signal (or kill the drill subprocess on container shutdown / timeout), and route `stop` to it for drill-engine runs.

### P1 — TLS/JA3 impersonation absent in Drill
- **What**: product differentiator ("under real mobile network profiles") is curl_cffi impersonation for locust; drill uses rustls with no fingerprint support. Upstream `fcsonline/drill` has no JA3. `sgmovea2z/drill` fork is same-tree.
- **Close with**: a fork change (swap reqwest for an impersonating client, e.g. `curl_cffi`-style, or wrap drill behind a curl_cffi proxy), or accept and document the limitation per-test. This is the hardest gap to close because it requires upstream Rust work.

### P1 — Network profiles (latency/jitter) not applied to Drill
- **What**: profile latency/jitter shaping lives in the locust runtime (`profile.py`); drill has no latency model — only fixed per-item `delay`.
- **Where**: `libs/locust-runtime/sutomu_locust_runtime/profile.py`, `client.py:70,106-109`.
- **Close with**: a pre-request latency shim in `drill_runner.py` (read `SUTOMU_NETWORK_PROFILE`, sleep per request) or inject `delay` items; cleaner: apply `tc`-style shaping at container level (future, matches EKS shaper intent).

### P2 — Variable/cookie state does not persist across iterations in Drill
- **What**: fresh context per iteration (`benchmark.rs:37-47`); session tokens captured in iteration N are gone in N+1. Locust VU store persists for the VU's life.
- **Where**: upstream behavior.
- **Close with**: generator-side workaround — emit a single-iteration benchmark with `concurrency=vus` and high `iterations` is already how we approximate duration; document that cross-iteration state requires locust. Or fork to persist context.

### P2 — Duration semantics are approximate for Drill
- **What**: generator sets `iterations = vus * duration_seconds`; wall-clock time is not bounded (slow requests → longer run). Locust `--run-time` is a hard bound.
- **Close with**: generator refinement (e.g. derive iterations from measured RPS) or fork-level duration cap; low priority if P0 results land first.

### P3 — Assertions differ (abort vs count)
- **What**: generated drill `assert` on status 200 **panics and aborts the whole benchmark** on first non-200; locust counts failures and continues.
- **Close with**: drop the `assert` from generated drill plans (rely on stats failure counts), matching locust behavior; or map statuses into per-request stats only.

### P3 — Locust-only gaps are actually parity gaps to document, not fix
- Bandwidth caps dead code (`curl_options_for_profile` uncalled) — parity with drill (neither shapes bandwidth).
- VictoriaMetrics streaming absent for both (`MetricEnvelope` schema-only).
- EKS/Locust Operator distributed mode doc-only for both in practice.

---

## 5. Drill advantages worth keeping

1. **Data-driven expansion**: `with_items_from_csv` / `with_items_range` — not currently emitted by the translator; could improve generated plans.
2. **CI regression gate**: `--report`/`--compare`/`--threshold` — unique; useful for scheduled-run regression checks.
3. **Plan reuse**: `include:` — matches Postman collection reuse.
4. **Tags**: `--tags`/`--skip-tags` — test selection without editing YAML.
5. **Single static binary**: trivial container footprint vs Python+gevent+curl_cffi image.

---

## 6. Implementation readiness notes (for whoever closes gaps)

- All drill wiring lives in: `libs/translator/sutomu_translator/drill_generator.py`, `libs/translator/sutomu_translator/plugins.py` (`DrillPlugin`), `apps/worker/drill_runner.py`, `apps/worker/container_server.py` (`_execute_drill_run`), `apps/cloudflare/run-launcher/src/entry.py`.
- Control-plane result plumbing to mirror: `apps/control-plane/sutomu_orchestration/local_executor.py:200` (`metrics_dashboard_url` = CSV URL) and `:259` (`{...}` JSON fallback in `get_run_results`).
- Test seams already exist: `tests/unit/test_drill_runner.py` (stats parsing), `tests/unit/test_container_server.py` (drill branch), `tests/unit/test_run_launcher.py` (engine forwarding), `tests/unit/test_drill_generator.py` (YAML emission).
- No gap here requires touching `libs/metric-schema` contracts to land; P0/P1 are control-plane + worker changes only.
