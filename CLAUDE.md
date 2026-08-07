# Drill — GitHub Flow Branching Strategy

This project follows [GitHub Flow](https://docs.github.com/en/get-started/using-github/github-flow).

## Core Rules

- **`main` is always deployable.** Never push broken or WIP code to main.
- **Every change starts as a branch from `main`.**
  ```bash
  git checkout main && git pull
  git checkout -b feat/my-descriptive-name
  ```
- **Open a Pull Request** for review before merging.
- **Merge to main only via PR** (squash or merge commit).
- **Delete the branch after merging.**

## Branch Naming Convention

```
<type>/<short-description>
```

Types: `feat`, `fix`, `style`, `refactor`, `docs`, `chore`, `perf`, `test`

Examples: `feat/stats-json-export`, `fix/pre-commit-hook`, `docs/api-reference`

## Workflow Commands

### `/start-feature` — Create a feature branch

Creates a branch from main with the given name and optional type prefix:

```bash
# Interactive: prompts for name and type
# Or one-shot: git checkout -b feat/<description>
```

### `/finish-feature` — Commit, push, open PR

```bash
# Stage all changes
git add -A
# Run quality gates
cargo fmt --check && cargo clippy -- -D warnings && cargo test
# Commit with conventional message
git commit -m "<type>: <description>"
# Push and open PR
git push -u origin HEAD
gh pr create --fill
```

## Pre-push Hook

A pre-push hook is installed at `.git/hooks/pre-push`. It blocks direct pushes to `main` — all changes must go through a PR.

To bypass (emergency only):
```bash
git push origin HEAD:main --no-verify
```

## Quality Gates (must pass before PR)

```bash
cargo fmt --check    # Formatting
cargo clippy -- -D warnings  # No warnings
cargo test           # All tests pass
```

## After Merge

```bash
git checkout main && git pull
git branch -d <branch-name>       # Delete local
git push origin --delete <branch-name>  # Delete remote
```
