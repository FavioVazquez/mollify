# Mollify CLI reference

> All commands below are implemented and tested.

## Commands
| Command | Description |
|---|---|
| `mollify audit` | Unified report across all engines + `quality_score` (0–100). |
| `mollify dead-code` (alias `check`) | Reachability-based unused files and symbols, unused class members (methods/attributes), enum members, unreachable code, and duplicate re-exports. |
| `mollify deps` | Dependency hygiene: unused / missing / transitive / misplaced-dev distributions + unresolved (broken) internal imports. |
| `mollify arch` | Architecture: circular dependencies, layer-boundary violations, cross-package private-import (interface) violations, policy violations. |
| `mollify complexity` (alias `health`) | Cyclomatic + cognitive complexity hotspots + churn×complexity hotspots. |
| `mollify dupes` | Duplication / clone families (token-based). |
| `mollify types` | Type-annotation health (fully-untyped public functions) + private-type leaks in public signatures. |
| `mollify security` | Security candidates (eval/exec, shell=True, hardcoded secrets, …). |
| `mollify coverage --coverage-file <f>` | Cold-path analysis from a coverage.py JSON report. |
| `mollify supply-chain [--offline] [--refresh] [--advisory-db <f>]` | Pinned/locked versions vs OSV (live by default; offline DB fallback) → `vulnerable-dependency`. |
| `mollify fix [--apply]` | Remove `certain` + `auto_fixable` unused symbols **and unused imports**. Dry-run unless `--apply`. |
| `mollify explain [<rule>]` | Explain a rule id (semantics, confidence, action). No argument lists all rules. |
| `mollify trace <module>` | Import neighborhood of a module: what it imports and what imports it. |
| `mollify inspect <file>` | Evidence bundle for one file: its findings + import neighborhood. |
| `mollify metrics` | Project-wide quantitative metrics (LOC, file/symbol counts, complexity distribution, finding tallies). |
| `mollify graph [--mermaid]` | Module dependency graph; `--mermaid` emits a Mermaid diagram. |
| `mollify list [entry-points\|files\|frameworks]` | Project topology. |
| `mollify watch [--interval-ms]` | Re-run `audit` on any `.py` change (poll-based; Ctrl-C to stop). |
| `mollify init` | Write a starter `.mollifyrc.json`. |
| `mollify mcp` | Run the MCP stdio server (for coding agents). |
| `mollify lsp` | Run a stdio Language Server (Content-Length JSON-RPC) publishing real-time diagnostics on open/save. |

## Global flags (per analysis command)
- `--path <dir>` — project root (default `.`).
- `--format human|json|sarif|github|junit` — output format (default `human`). `json` is the kind-discriminated contract; `sarif` is SARIF 2.1.0 for code scanning; `github` emits GitHub Actions workflow annotations; `junit` emits a JUnit XML report.
- `--min-confidence certain|likely|uncertain` — keep only findings at or above the given confidence tier.
- `--gate all|new-only` — `new-only` keeps only findings in changed files (introduced).
- `--base <ref>` — git base ref for `--gate new-only` (e.g. `origin/main`).
- `--save-baseline <f>` — write a regression baseline (finding fingerprints) and exit 0.
- `--baseline <f>` — keep only findings new since that baseline.
- `--fail-on-regression` — with `--baseline`, exit non-zero if any new findings appeared.
- `--brief` — advisory mode: print the report but always exit 0.

## Command-specific flags
- `mollify supply-chain` — `--offline`, `--refresh`, `--advisory-db <f>`.
- `mollify coverage` — `--coverage-file <f>` (required).
- `mollify graph` — `--mermaid` (emit a Mermaid diagram).
- `mollify watch` — `--interval-ms <n>` (poll interval).
- `mollify fix` — `--apply` (without it, dry-run).

## Exit codes
- `0` — no `error`-severity findings.
- `1` — one or more `error`-severity findings (CI gate) or a command error.

Severities are `warn` by default; raise rules/categories to `error` in `.mollifyrc.json` to gate CI.

## Rules emitted
`unused-file`, `unused-export`, `unused-import`, `unused-variable`,
`unused-parameter`, `unused-method`, `unused-attribute`, `unused-enum-member`,
`unreachable-code`, `commented-code`,
`unused-dependency`, `missing-dependency`, `transitive-dependency`, `misplaced-dev-dependency`,
`unresolved-import`, `duplicate-export`, `private-import`, `circular-dependency`,
`layer-violation`, `forbidden-import`, `independence-violation`,
`high-complexity`, `duplication`, `untyped-function`, `private-type-leak`, `cold-code`, `hotspot`, `low-cohesion`,
`dangerous-eval`, `subprocess-shell-true`, `sql-injection`, `unsafe-yaml-load`,
`unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret`,
`weak-hash`, `weak-cipher`, `insecure-random`, `request-without-timeout`,
`flask-debug-true`, `jinja2-autoescape-false`, `try-except-pass`,
`vulnerable-dependency`, `policy-violation` (+ custom policy ids) from `.mollifyrc.json`
`policies`.

## MCP tools (`mollify mcp`)
The stdio MCP server exposes 16 tools (`watch` and `lsp` are CLI-only):
`mollify_audit`, `mollify_dead_code`, `mollify_deps`, `mollify_arch`,
`mollify_complexity`, `mollify_dupes`, `mollify_types`, `mollify_security`,
`mollify_coverage`, `mollify_supply_chain`, `mollify_explain`, `mollify_trace`,
`mollify_inspect`, `mollify_list`, `mollify_metrics`, `mollify_fix`.
Params: `mollify_coverage` requires `coverage_file`; `mollify_trace` requires
`module`; `mollify_inspect` requires `file`; `mollify_supply_chain` takes optional
`advisory_db`; `mollify_list` takes optional `kind`; all others take optional
`path` (default `.`).

## `.mollifyrc.json`
```json
{
  "severity": { "dead-code": "error", "duplication": "warn", "unused-dependency": "off" },
  "ignore": ["tests/", "migrations/"],
  "max_cyclomatic": 10,
  "max_cognitive": 15,
  "architecture": { "preset": "layered", "layers": ["api", "service", "domain", "infra"] },
  "policies": [
    { "id": "no-requests-in-domain", "forbid_import": "requests", "in_paths": ["domain/"], "severity": "error" },
    { "id": "no-print", "forbid_call": "print", "severity": "warn" }
  ]
}
```
`severity` keys are rule ids or category names (`dead-code`, `duplication`,
`circular-dependency`, `complexity`, `architecture`, `dependency-hygiene`, `type-health`, `security`).
See `references/configuration.md` semantics in `docs/configuration.md` for `architecture` and `policies`.

## LSP server
`mollify lsp` runs a stdio Language Server (Content-Length framed JSON-RPC) that
publishes real-time diagnostics on document open/save. Register it as a language
server for Python in any LSP-capable editor (point the editor at `mollify lsp`).

## Not yet implemented (do not rely on)
Keystroke-incremental LSP reparse (diagnostics refresh on open/save, not per
edit) and LibCST format-preserving fixes (current `fix` is text-based).
