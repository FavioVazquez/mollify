---
name: mollify
description: >
  Run Mollify — a Rust-native, deterministic Python code-intelligence CLI — to
  find dead code (unused files/functions/classes) and dependency-hygiene issues
  (unused / missing distributions). Use whenever the user asks whether Python
  code is used, what is safe to delete, what's duplicated, wants a repo
  health/quality report, or before opening a PR that touches Python.
# Fields below are honored by Devin CLI / Devin Local; ignored by Cascade IDE.
allowed-tools: [read, grep, glob, exec]
---

# Mollify code intelligence

Mollify is a **deterministic candidate-producer**: it emits *evidence* — every
finding has a stable `fingerprint`, a `confidence` tier, and a `reason`. You are
the verifier. **Never invent findings, and never hand-delete code on a guess.**

## Running an audit
1. Full report:        `mollify audit --format json`
2. Dead code only:     `mollify dead-code --format json`
3. Dependency hygiene: `mollify deps --format json`

(Add `--path <dir>` to target a subproject. Drop `--format json` for a readable
summary.)

## Reading the JSON (the contract)
The envelope has a discriminating top-level `kind` (`audit` / `dead-code` /
`deps`), a `summary`, and `findings[]`. `audit` also has `quality_score` (0–100).
Each finding:
- `rule` — e.g. `unused-export`, `unused-file`, `unused-dependency`, `missing-dependency`
- `category` — `dead-code` | `dependency-hygiene` | … 
- `confidence` — `certain` | `likely` | `uncertain`
- `severity` — `error` | `warn` | `off`
- `reason`, `location {path, line, end_line}`, `fingerprint`
- `actions[]` — each has `type`, `description`, `auto_fixable` (bool), `suppression_comment`

See `references/json-contract.md` for the full schema and `references/cli-reference.md`
for all commands.

## Acting on findings
1. Summarize, leading with `confidence: certain`; cite `path:line` and the fingerprint.
2. An action with `auto_fixable: true` **and** the finding `confidence: certain`
   is safe to apply. Everything else: explain the trace and let the user decide.
3. For `likely`/`uncertain`, or any deletion: confirm before editing. To silence a
   known-good finding, add its `suppression_comment` on the relevant line instead
   of deleting code.
4. Re-run the audit afterward and confirm the fingerprint is gone.

## Honesty rules
- Mollify reachability is static; dynamic imports (`getattr`/`importlib`) downgrade
  confidence to `uncertain` — treat those as review-only.
- A `missing-dependency` may be a false positive for namespace packages or local
  shadowing; verify before adding to `pyproject.toml`.
