---
name: mollify
description: >
  Run Mollify — a Rust-native, deterministic Python code-intelligence CLI — to
  find dead code (unused files/functions/classes) and dependency-hygiene issues
  (unused / missing distributions). Use whenever the user asks whether Python
  code is used, what is safe to delete, wants a repo health/quality report, or
  before opening a PR that touches Python.
# Fields below are honored by Devin CLI / Devin Local; ignored by Cascade IDE.
allowed-tools: [read, grep, glob, exec]
---

# Mollify code intelligence

Mollify is a **deterministic candidate-producer**: it emits *evidence* — every
finding has a stable `fingerprint`, a `confidence` tier, and a `reason`. You are
the verifier. **Never invent findings, and never hand-delete code on a guess.**

## Prefer the MCP server
If the `mollify` MCP server is connected (launched via `mollify mcp`), call its
tools directly. Otherwise use the CLI below.

## Running an audit
1. Full report:        `mollify audit --format json`
2. Dead code only:     `mollify dead-code --format json` (alias `mollify check`)
3. Dependency hygiene: `mollify deps --format json`

(Add `--path <dir>` to target a subproject. Drop `--format json` for a readable
human summary.)

## Reading the JSON (the contract)
The envelope has a discriminating top-level `kind` (`audit` / `dead-code` /
`deps`), a `summary` ({total, errors, warnings, files_analyzed}), and
`findings[]`. `audit` also has `quality_score` (0-100). `schema_version` is `0.1`.
Switch on `kind`; iterate `findings[]`. Each finding:
- `rule` — `unused-file`, `unused-export`, `unused-dependency`, `missing-dependency`
- `category` — `dead-code` | `dependency-hygiene`
- `confidence` — `certain` | `likely` | `uncertain`
- `severity` — `error` | `warn` | `off`
- `reason`, `location {path, line, end_line}`, `fingerprint`
- `actions[]` — each has `type`, `description`, `auto_fixable` (bool), `suppression_comment`

See `references/json-contract.md` for the full schema and `references/cli-reference.md`
for all commands and flags.

## Acting on findings
1. Summarize, leading with `confidence: certain`; cite `path:line` and the fingerprint.
2. Act only on `confidence: certain` without confirming. Everything else
   (`likely`/`uncertain`): explain the reason and let the user decide.
3. For any deletion, confirm before editing. To silence a known-good finding, add
   its action's `suppression_comment` on the relevant line instead of deleting code.
   (An `auto_fixable` flag marks where automated fixing is intended; `mollify fix`
   is not yet implemented — apply changes manually after review.)
4. Re-run the audit afterward and confirm the fingerprint is gone.

## Honesty rules
- Mollify reachability is static; dynamic imports (`getattr`/`importlib`) downgrade
  confidence to `uncertain` — treat those as review-only.
- A `missing-dependency` may be a false positive for namespace packages or local
  shadowing; verify before adding to `pyproject.toml`.
- Exit code 0 = no error-severity findings; 1 = error-severity findings or a
  command error. `--gate new-only`, SARIF, and `fix` are not yet implemented.
