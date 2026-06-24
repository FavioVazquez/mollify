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

## Commands (21)
Analysis engines (each takes the global flags below):
- `mollify audit` — unified report across all engines + `quality_score` (0–100).
- `mollify dead-code` (alias `check`) — unused files/exports/imports.
- `mollify deps` — dependency hygiene (unused / missing distributions).
- `mollify arch` — circular deps, layer/boundary/policy violations.
- `mollify complexity` (alias `health`) — complexity + churn×complexity hotspots.
- `mollify dupes` — duplication / clone families.
- `mollify types` — type-annotation health (untyped public functions).
- `mollify security` — security candidates (eval/exec, shell=True, secrets, …).
- `mollify coverage --coverage-file <f>` — cold-path analysis from a coverage.py report.
- `mollify supply-chain [--offline] [--refresh] [--advisory-db <f>]` — versions vs
  OSV (live by default; offline DB fallback) → `vulnerable-dependency`.

Actions / utilities:
- `mollify fix [--apply]` — remove `certain` + `auto_fixable` unused symbols and
  unused imports. Dry-run unless `--apply`.
- `mollify explain [<rule>]` — explain a rule id; no arg lists all rules.
- `mollify trace <module>` — import neighborhood of a module.
- `mollify inspect <file>` — evidence bundle for one file.
- `mollify metrics` — project-wide quantitative metrics (LOC, counts, complexity distribution).
- `mollify graph [--mermaid]` — module dependency graph; `--mermaid` emits a Mermaid diagram.
- `mollify list [entry-points|files|frameworks]` — project topology.
- `mollify watch [--interval-ms]` — re-run `audit` on any `.py` change (CLI-only).
- `mollify init` — write a starter `.mollifyrc.json`.
- `mollify mcp` — run the MCP stdio server (for coding agents).
- `mollify lsp` — run a stdio Language Server publishing real-time Python diagnostics (CLI-only).

## Global flags (analysis commands)
- `--path <dir>` (default `.`),
  `--format human|json|sarif|github|junit` (default `human`; `github` =
  GitHub Actions annotations, `junit` = JUnit XML).
- `--gate all|new-only` (`new-only` keeps only findings in changed files),
  `--base <ref>` (git base for the gate).
- `--save-baseline <f>`, `--baseline <f>`, `--fail-on-regression`.
- `--brief` (advisory: print but exit 0),
  `--min-confidence certain|likely|uncertain`.

Both `--gate new-only` and `--format sarif` are fully implemented. Drop `--format
json` for a readable human summary.

## Reading the JSON (the contract)
The envelope has a discriminating top-level `kind` (`audit` | `dead-code` |
`deps`), `schema_version` `"0.1"`, a `summary` `{total, errors, warnings,
files_analyzed}`, and `findings[]`. `audit` also has `quality_score` (0–100).

Each finding:
- `fingerprint` — stable id (e.g. `unused-export:931a82e6`).
- `rule` — one of: `unused-file`, `unused-export`, `unused-import`,
  `unused-variable`, `unused-parameter`,
  `commented-code`, `unused-dependency`, `missing-dependency`,
  `circular-dependency`, `layer-violation`, `forbidden-import`,
  `independence-violation`, `high-complexity`, `duplication`, `untyped-function`,
  `cold-code`, `hotspot`, `dangerous-eval`, `subprocess-shell-true`,
  `sql-injection`, `unsafe-yaml-load`, `unsafe-deserialization`,
  `tls-verify-disabled`, `hardcoded-secret`, `weak-hash`, `weak-cipher`,
  `insecure-random`, `request-without-timeout`, `vulnerable-dependency`, plus any
  custom policy ids from `.mollifyrc.json` `policies`.
- `category` — `dead-code` | `dependency-hygiene` | `circular-dependency` |
  `complexity` | `architecture` | `duplication` | `type-health` | `security`.
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
- `mollify fix` removes only `certain` + `auto_fixable` unused symbols and unused
  imports (dry-run unless `--apply`).
- `--gate new-only` and `--format sarif` are fully implemented working features —
  use them for PR gating and code-scanning output.

## MCP Server Tools
`mollify mcp` runs a stdio MCP server exposing 16 tools (`watch` and `lsp` are
CLI-only): `mollify_audit`, `mollify_dead_code`, `mollify_deps`, `mollify_arch`,
`mollify_complexity`, `mollify_dupes`, `mollify_types`, `mollify_security`,
`mollify_coverage`, `mollify_supply_chain`, `mollify_explain`, `mollify_trace`,
`mollify_inspect`, `mollify_list`, `mollify_metrics`, `mollify_fix`. Params: `mollify_coverage`
requires `coverage_file`; `mollify_trace` requires `module`; `mollify_inspect`
requires `file`; `mollify_supply_chain` takes optional `advisory_db`;
`mollify_list` takes optional `kind`; all others take optional `path` (default `.`).

## LSP server
`mollify lsp` runs a stdio Language Server (Content-Length framed JSON-RPC) that
publishes real-time diagnostics on document open/save. Register it as the Python
language server in any LSP-capable editor (command: `mollify lsp`).

## Exit codes
- `0` — no `error`-severity findings.
- `1` — one or more `error`-severity findings, or a command error.
