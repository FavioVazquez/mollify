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

## Prefer the MCP server
If the `mollify` MCP server is connected (launched via `mollify mcp`), call its
16 tools directly (see "MCP server tools" below). Otherwise use the CLI.

## Commands (21)
Analysis engines (all take the global flags below):
`mollify audit` (unified + `quality_score`), `mollify dead-code` (alias `check`),
`mollify deps`, `mollify arch`, `mollify complexity` (alias `health`),
`mollify dupes`, `mollify types`, `mollify security`,
`mollify coverage --coverage-file <f>`,
`mollify supply-chain [--offline] [--refresh] [--advisory-db <f>]` (live OSV by
default; offline DB fallback).

Actions / utilities:
`mollify fix [--apply]` (remove `certain` + `auto_fixable` unused symbols and
imports; dry-run unless `--apply`), `mollify explain [<rule>]`,
`mollify trace <module>`, `mollify inspect <file>`, `mollify metrics`
(project-wide quantitative metrics), `mollify graph [--mermaid]` (module
dependency graph; `--mermaid` emits a Mermaid diagram),
`mollify list [entry-points|files|frameworks]`,
`mollify watch [--interval-ms]` (CLI-only), `mollify init`, `mollify mcp`,
`mollify lsp` (stdio Language Server with real-time diagnostics; CLI-only).

Global flags (analysis commands): `--path <dir>` (default `.`),
`--format human|json|sarif|github|junit` (`github` = GitHub Actions annotations,
`junit` = JUnit XML), `--gate all|new-only`, `--base <ref>`,
`--save-baseline <f>`, `--baseline <f>`, `--fail-on-regression`, `--brief`,
`--min-confidence certain|likely|uncertain`, `--include <dir>` (repeatable; scan
a directory despite the builtin exclude list or `.mollifyrc.json`'s
`exclude_dirs`). `--gate new-only` and `--format
sarif` are fully implemented. Drop `--format json` for a human summary; add
`--path <dir>` to target a subproject. See `references/cli-reference.md`.

## Reading the JSON (the contract)
The envelope has a discriminating top-level `kind` (`audit` / `dead-code` /
`deps`), a `summary`, and `findings[]`. `audit` also has `quality_score` (0–100).
Each finding:
- `rule` — one of `unused-file`, `unused-export`, `unused-import`,
  `unused-variable`, `unused-parameter`, `unused-method`, `unused-attribute`,
  `unused-enum-member`, `unreachable-code`,
  `commented-code`, `unused-dependency`, `missing-dependency`, `transitive-dependency`,
  `misplaced-dev-dependency`, `unresolved-import`, `duplicate-export`, `private-import`,
  `circular-dependency`, `layer-violation`, `forbidden-import`,
  `independence-violation`, `high-complexity`, `duplication`, `untyped-function`,
  `private-type-leak`, `cold-code`, `hotspot`, `low-cohesion`, `dangerous-eval`, `subprocess-shell-true`,
  `sql-injection`, `unsafe-yaml-load`, `unsafe-deserialization`,
  `tls-verify-disabled`, `hardcoded-secret`, `weak-hash`, `weak-cipher`,
  `insecure-random`, `request-without-timeout`, `flask-debug-true`,
  `jinja2-autoescape-false`, `try-except-pass`, `vulnerable-dependency`, plus
  custom policy ids
- `category` — `dead-code` | `dependency-hygiene` | `circular-dependency` |
  `complexity` | `architecture` | `duplication` | `type-health` | `security`
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
- A `missing-dependency`, `transitive-dependency` may be a false positive for namespace packages or local
  shadowing; verify before adding to `pyproject.toml`.
- `--gate new-only`, `--format sarif`, and `mollify fix` are all fully implemented
  working features.

## MCP server tools
`mollify mcp` exposes 16 tools (`watch` and `lsp` are CLI-only): `mollify_audit`,
`mollify_dead_code`, `mollify_deps`, `mollify_arch`, `mollify_complexity`,
`mollify_dupes`, `mollify_types`, `mollify_security`, `mollify_coverage`,
`mollify_supply_chain`, `mollify_explain`, `mollify_trace`, `mollify_inspect`,
`mollify_list`, `mollify_metrics`, `mollify_fix`. Params: `mollify_coverage` requires
`coverage_file`; `mollify_trace` requires `module`; `mollify_inspect` requires
`file`; `mollify_supply_chain` takes optional `advisory_db`; `mollify_list` takes
optional `kind`; all others take optional `path` (default `.`).

## LSP server
`mollify lsp` runs a stdio Language Server (Content-Length framed JSON-RPC) that
publishes real-time diagnostics on document open/save. Register it as the Python
language server in any LSP-capable editor (command: `mollify lsp`).
