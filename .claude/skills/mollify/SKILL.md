---
name: mollify
description: Audit a Python codebase with mollify — a deterministic, Rust-native code-intelligence CLI — for dead code (unused files/exports) and dependency hygiene (unused/missing distributions). Use whenever the user asks whether Python code is used, what is safe to delete, wants a repo health/quality report, or before opening a PR that touches Python.
allowed-tools: Bash(mollify *)
---

# Mollify code intelligence

Mollify is a deterministic candidate-producer: every finding has a stable
`fingerprint`, a `confidence` tier, and a `reason`. It emits evidence, not
decisions. You are the verifier. Never invent findings, and never hand-delete
code on a guess.

## Running an audit
- Full report:        `mollify audit --format json`
- Dead code only:     `mollify dead-code --format json` (alias: `mollify check`)
- Dependency hygiene: `mollify deps --format json`

Add `--path <dir>` to target a subproject. Drop `--format json` for a readable
human summary. There are only two flags: `--path <dir>` (default `.`) and
`--format human|json` (default `human`).

## Reading the JSON (the contract)
The envelope has a discriminating top-level `kind` (`audit` | `dead-code` |
`deps`), `schema_version` `"0.1"`, a `summary` `{total, errors, warnings,
files_analyzed}`, and `findings[]`. `audit` also has `quality_score` (0–100).

Each finding:
- `fingerprint` — stable id (e.g. `unused-export:931a82e6`).
- `rule` — `unused-file`, `unused-export`, `unused-dependency`, `missing-dependency`.
- `category` — `dead-code` | `dependency-hygiene` | …
- `severity` — `error` | `warn` | `off`.
- `confidence` — `certain` | `likely` | `uncertain`.
- `reason`, `location {path, line, end_line}`.
- `actions[]` — each with `type`, `description`, `auto_fixable` (bool),
  `suppression_comment`.

See `references/cli-reference.md` for all commands/flags and
`references/json-contract.md` for the full envelope schema.

## Acting on findings
1. Summarize, leading with `confidence: certain`; cite `path:line` and the
   fingerprint, and group by rule.
2. An action with `auto_fixable: true` **and** the finding `confidence: certain`
   is safe to act on. Everything else: explain the trace and let the user decide.
3. For `likely`/`uncertain`, or any deletion: confirm before editing. To silence
   a known-good finding, add its `suppression_comment` on the relevant line
   instead of deleting code.
4. Re-run the audit afterward and confirm the fingerprint is gone.

## Honesty rules
- Reachability is static; dynamic imports (`getattr`/`importlib`) downgrade
  confidence to `uncertain` — treat those as review-only.
- A `missing-dependency` may be a false positive for namespace packages or local
  shadowing; verify before adding to `pyproject.toml`.
- `mollify fix` removes only `certain` + `auto_fixable` unused symbols (dry-run
  unless `--apply`). `--gate new-only` and `--format sarif` are available. Do
  not reference them as working features.

## Exit codes
- `0` — no `error`-severity findings.
- `1` — one or more `error`-severity findings, or a command error.
