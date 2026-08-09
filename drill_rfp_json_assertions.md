# RFP: Value Assertions Inside JSON Replies

- **Status:** Open for proposal
- **Target milestone:** TBD
- **Related issue:** (attached via GitHub issue)

---

## 1. Background

Drill's `assert` action supports five assertion types driven by a `type:` field:

| Type       | YAML                                                               | Checks against                                          |
| ---------- | ------------------------------------------------------------------ | ------------------------------------------------------- |
| (default) `Equals` | `assert: {key: bar, value: '2'}`               | a **context variable** == literal                       |
| `status`   | `assert: {type: status, value: 200}` or `[200, 201]` | last response `status` in list                  |
| `header`   | `assert: {type: header, key: content-type, value: application/json}` | last response header, case-insensitive substring |
| `jsonpath` | `assert: {type: jsonpath, key: '$.data.id', value: '123'}` | first JSONPath match on last response `body` |
| `duration` | `assert: {type: duration, value: 500, operator: lt}` | last response `duration` ms; ops `lt/lte/gt/gte/eq` |

Per-response data available to assertions (see `src/actions/request.rs`): `status`, `body`, `headers`, `url`, `duration`, `time_starttransfer_ms`, `time_total_ms`, `size_*`.

## 2. Problem Statement

The `jsonpath` assertion type is the tool for *value assertions inside JSON replies*, but it is the weakest of the five:

1. **Equality only.** `execute_jsonpath` uses a string-coercing `value_eq` — a JSON number `123` matches the literal `"123"` string implicitly. There are no operators: no `gt`, `lt`, `gte`, `lte`, `neq`, `contains`, `regex`, `exists`, or `null`.
2. **First match only.** `matches.first()` is used; `$.items[*]` asserts only the first element and silently ignores the rest.
3. **Expected value is a string.** `Assert.value` is a `String`. It is impossible to assert typed values — `null`, booleans, numbers-as-numbers, nested objects, arrays, or an entire JSON document.
4. **No presence/absence check.** No way to assert a field exists, or is absent, or is `null`.
5. **No length/count assertion.** Cannot express "`$.items` has exactly 3 entries" or "matches at least N".
6. **Failures are panics** caught via `catch_unwind`; failure handling is all-or-abort vs. global `--continue-on-assert-fail`.

`duration` already demonstrates a generalized `operator` + typed `value` pattern in this codebase; `jsonpath` has not inherited it.

## 3. Scope — Requirements (must-haves)

Proposals must satisfy the following acceptance criteria:

### R3.1 — Typed expected values

- The `value` field of a `jsonpath` assertion accepts any YAML scalar or collection (number, boolean, `null`, string, array, object) and is parsed into `serde_json::Value`.
- When `operator` is omitted (default `eq`), comparison is **structural** JSON equality, NOT string coercion. Whether loose string->number coercion is retained as an opt-in flag (`strict: true` disables it) is at the proposer's discretion, but strict-mode is preferred and must be documented.
- Existing benchmarks must keep working: the current string-coercing behavior must remain the behavior when `value` is provided as a YAML string AND operator is `eq` (backward compatibility).

### R3.2 — Generalized operators for `jsonpath`

- Support at least: `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `contains`, `in`, `exists`, `is_null`, `regex` — mirroring the `operator` field already used by `duration`.
- `contains`: for strings — substring; for arrays — element presence (structural equality of any element).
- `in`: expected `value` is an array; actual value must member-match any element.
- `is_null`: asserts the matched value is JSON `null` (no `value` needed).
- Unsupported operator values must produce a clear parse-time panic listing supported operators (matching the `duration` operator error style).

### R3.3 — Multi-match semantics

- New optional field `every: true|false` (default `false`, preserving current first-match behavior).
- When `every` is `true`, the JSONPath SELECT all matches using `jsonpath_lib::select`; every match must satisfy the operator; a single failing match fails the assertion.
- New optional field `match_count:` — an integer (or integer with `operator`) asserting the number of matches. E.g. `match_count: 3` or `match_count: {gte: 2}` (syntax at proposer's discretion, documented, and must include a default).

### R3.4 — `exists` / `is_null` presence checks

- `operator: exists` — passes when at least one match exists.
- `operator: not_exists` (or `exists: false` — schema at proposer discretion, must be documented) — passes when no match exists.
- Must be combinable with `every: true` where sensible.

### R3.5 — Full-document equality

- When `key` is omitted on a `jsonpath` type assertion, compare the whole response body (parsed as JSON) against `value` structurally.
- This reuses R3.1's structural comparison — no new engine needed.

### R3.6 — Regex

- `operator: regex` with a `value` string: the matched value (string-coerced) Full-match against the regex. Invalid regex must fail fast at parse time with a clear message.
- Add `regex` crate dependency if not already present (verify `Cargo.toml`).

## 4. Out of scope

- New `assert` types not listed above (e.g. `schema` validation against JSON Schema — separate RFP).
- Changes to `status`, `header`, `Equals` (context), or `duration` semantics beyond what is required for shared operator infrastructure.
- Threshold/compare functionality.
- Performance micro-optimizations.

## 5. Compatibility

- All existing benchmark YAML files must remain valid and produce identical behavior (defaults preserve `first-match`, string-coercing `eq`).
- No breaking changes to the `assert` YAML schema; new fields (`operator`, `every`, `match_count`) are additive.
- Grammar: new fields exactly the `duration` operator's pattern for consistency.

## 6. Testing requirements

- Follow existing test conventions in `src/actions/assert.rs` (`#[tokio::test]`, `#[should_panic(expected = ...)]`, the `last_response(status, body, headers, duration)` fixture helper).
- Cover, at minimum:
  - Typed value: number, boolean, `null`, nested object, array.
  - Each operator: `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `contains`, `in`, `exists`, `not_exists`, `is_null`, `regex` (success + failure each).
  - `every: true` all-match semantics (success + failure when one element fails).
  - `match_count` success + failure.
  - Full-document equality (no `key`) success + failure.
  - Backward compatibility: existing string-coercion `eq` tests still pass unchanged.
  - Parse-time errors: unknown operator, invalid regex, malformed `match_count`.
- New assertions parse and execute-only in existing `Runnable::execute` flow; no changes to `Config` are required unless operator infra demands it.

## 7. Quality gates (must pass)

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## 8. Submission guidelines (for proposal)

- Branch from `master`; PR opened per GitHub Flow (see `AGENTS.md`).
- PR must include: implementation, tests (per §6), `SYNTAX.md` documentation updates for the `assert` extended sections, and example (`example/`) if a new capability demonstrates nicely.
- No `unsafe`, no new heavyweight dependencies unless justified in the PR description.

## 9. Acceptance criteria

- All §6 tests pass, §7 quality gates pass.
- `SYNTAX.md` updated with: typed `value`, `operator`, `every`, `match_count`, and full-document equality documentation with at least one example each.
- Manually verified: a benchmark that asserts a numeric value with `gt`, a `contains`, an array membership, and an `every: true` all-matches on a small JSON fixture.
- Stakeholder sign-off on the behavior of loose string coercion default vs opt-in (see §3.1).

## 10. Timeline

- Proposal submission window: TBD
- Review period: TBD
- Implementation expectation: 1–2 working days for a focused submission of the above scope.