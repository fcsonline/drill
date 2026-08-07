# Benchmark YAML validation

`drill validate` runs a pre-flight check on a benchmark YAML file **before** the test starts. It
walks the parsed document tree (mirroring the field tables the runtime uses) and reports every
problem it finds — without aborting on the first one.

## Usage

```bash
drill validate <benchmark.yml>
drill validate --format json <benchmark.yml>
```

## Exit codes

| Exit | Meaning |
|---|---|
| `0` | No errors (warnings/suggestions are non-fatal) |
| `1` | At least one **error** (the benchmark would fail to run) |

## Severity levels

| Level | Meaning | Blocks run? |
|---|---|---|
| `error` | Breaks the run: missing required field, bad type, cross-field invariant violation, malformed YAML, unresolvable `include` | Yes (exit 1) |
| `warning` | Runnable but risky: unknown top-level key, uppercase HTTP method, relative `url` without `base`, `{{ item }}` outside a matrix scope | No |
| `suggestion` | Standards guidance from `SYNTAX.md`: unnamed plan item, request body without an explicit method | No |

## Output

**Human mode** (default) prints a per-diagnostic line with severity, location, and message,
followed by a summary count:

```
Validation results:
  [    error] plan[0].request: `request` block requires a `url`
  [  warning] /path/bench.yml: relative `request.url` has no top-level `base`
[suggestion] plan[0]: plan item has no `name`; naming items improves report readability

1 error(s), 1 warning(s), 1 suggestion(s)
```

**JSON mode** (`--format json`) prints a single strict JSON array, one object per diagnostic:

```json
[{"severity":"error","location":"plan[0].request","message":"`request` block requires a `url`"}]
```

A clean file prints `OK — no errors, warnings, or suggestions found.` (human) or `[]` (JSON).

## What is checked

- **Top level**: known keys (`base`, `iterations`, `concurrency`, `rampup`, `threads`,
  `new_conn_per_iter`, `persist_context`, `run_time`, `success_codes`, `results`, `lifecycle`,
  `load_shape`, `vars`, `plan`); `plan` present and non-empty; scalar types and signs;
  `concurrency <= iterations` unless a `load_shape` is present (the runtime aborts otherwise).
- **Plan items**: one recognized action discriminator per item (`request`, `include`, `delay`,
  `exec`, `assign`, `save`, `assert`, `for_each`); type checks on each action's fields; HTTP
  method whitelist; mutually-exclusive `with_items*` matrix discriminators.
- **Request bodies**: string/number/scalar, or a descriptor mapping (`file`/`hex`/`urlencoded`/
  `formdata`/`graphql`).
- **Auth**: `type` must be `basic`, `bearer`, or `oauth2`.
- **Lifecycle hooks** and **`for_each.plan`** sub-trees are validated recursively.
- **`include`**: target files are resolved relative to the including file, validated recursively,
  and cyclic includes are reported as errors (depth-capped).

## Files

- Validator source: `src/validate/` (`mod.rs` orchestrates `load`, `top`, `plan`, `recursion`,
  `suggest`, `out`, `diag`).
- Fixtures: `tests/fixtures/validate/`
- End-to-end tests: `tests/validate_e2e.rs`
