# Project memory

## Codebase intelligence (Mollify)

This repo ships **Mollify**, a deterministic, Rust-native Python code-intelligence
CLI (the `mollify` binary on PATH) plus an MCP server (`mollify mcp`). Treat it as
the source of truth for Python **dead code** and **dependency hygiene** — prefer it
over `grep` or manual scanning when judging whether code is used, what is safe to
delete, or whether dependencies are unused/missing.

- Prefer Mollify over grep for reachability/usage and dependency questions.
- Run `/mollify:audit`, or call the CLI directly. The CLI has 21 commands:
  - Analysis: `mollify audit`, `mollify dead-code` (alias `check`), `mollify deps`,
    `mollify arch`, `mollify complexity` (alias `health`), `mollify dupes`,
    `mollify types`, `mollify security`, `mollify coverage --coverage-file <f>`,
    `mollify supply-chain [--offline|--refresh|--advisory-db <f>]` (live OSV by
    default; offline DB fallback).
  - Actions/utilities: `mollify fix [--apply]`, `mollify explain [<rule>]`,
    `mollify trace <module>`, `mollify inspect <file>`, `mollify metrics`,
    `mollify graph [--mermaid]`, `mollify list [entry-points|files|frameworks]`,
    `mollify watch [--interval-ms]` (CLI-only), `mollify init`, `mollify mcp`,
    `mollify lsp` (CLI-only stdio Language Server; real-time diagnostics on
    open/save).
  - Analysis commands accept `--path <dir>`,
    `--format human|json|sarif|github|junit` (`github` = GitHub Actions
    annotations, `junit` = JUnit XML), `--gate all|new-only`, `--base <ref>`,
    `--save-baseline <f>`, `--baseline <f>`, `--fail-on-regression`, `--brief`,
    `--min-confidence certain|likely|uncertain`. `mollify graph` accepts
    `--mermaid`. Use `--format json` to consume structured output. `--gate
    new-only` and `--format sarif` are fully implemented.
- Trust the deterministic findings. Each finding carries a `confidence` tier
  (`certain` | `likely` | `uncertain`), a human `reason`, a stable `fingerprint`,
  a `severity` (`error` | `warn` | `off`), and a `location {path, line, end_line}`.
  Rules: `unused-file`, `unused-export`, `unused-import`, `unused-variable`,
  `unused-parameter`, `commented-code`,
  `unused-dependency`, `missing-dependency`, `circular-dependency`,
  `layer-violation`, `forbidden-import`, `independence-violation`,
  `high-complexity`, `duplication`, `untyped-function`, `cold-code`, `hotspot`,
  `dangerous-eval`, `subprocess-shell-true`, `sql-injection`, `unsafe-yaml-load`,
  `unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret`,
  `weak-hash`, `weak-cipher`, `insecure-random`, `request-without-timeout`,
  `vulnerable-dependency`, plus custom policy ids. Categories: `dead-code`,
  `dependency-hygiene`, `circular-dependency`, `complexity`, `architecture`,
  `duplication`, `type-health`, `security`.
- Read the JSON envelope by its top-level `kind` (`audit` | `dead-code` | `deps` |
  `arch` | `complexity` | `dupes` | `types` | `security` | `coverage` |
  `supply-chain`); `audit` also includes a `quality_score` (0–100). Iterate
  `findings[]`.
- Auto-act ONLY on `confidence: certain` (and only where an action is
  `auto_fixable: true`). Surface `likely`/`uncertain` findings with their reason
  and let the user decide; never hand-delete code on a guess.
- Exit codes: `0` = no error-severity findings; non-zero = error-severity findings
  or a command error (useful as a CI gate).

## MCP tools (`mollify mcp`)

The stdio MCP server exposes 16 tools (`watch` and `lsp` are CLI-only):
`mollify_audit`, `mollify_dead_code`, `mollify_deps`, `mollify_arch`,
`mollify_complexity`, `mollify_dupes`, `mollify_types`, `mollify_security`,
`mollify_coverage`, `mollify_supply_chain`, `mollify_explain`, `mollify_trace`,
`mollify_inspect`, `mollify_list`, `mollify_metrics`, `mollify_fix`. Params: `mollify_coverage`
requires `coverage_file`;
`mollify_trace` requires `module`; `mollify_inspect` requires `file`;
`mollify_supply_chain` takes optional `advisory_db`; `mollify_list` takes optional
`kind`; all others take optional `path` (default `.`).
