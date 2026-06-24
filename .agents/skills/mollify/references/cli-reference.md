# Mollify CLI reference

> Status: Phases 0–2 complete + Phase-1 polish. All commands below are implemented and tested.

## Commands
| Command | Description |
|---|---|
| `mollify audit` | Unified report across all engines + `quality_score` (0–100). |
| `mollify dead-code` (alias `check`) | Reachability-based unused files and symbols. |
| `mollify deps` | Dependency hygiene: unused / missing distributions. |
| `mollify arch` | Architecture: circular-dependency detection. |
| `mollify complexity` (alias `health`) | Cyclomatic + cognitive complexity hotspots. |
| `mollify dupes` | Duplication / clone families (token-based). |
| `mollify types` | Type-annotation health (fully-untyped public functions). |
| `mollify security` | Security candidates (eval/exec, shell=True, hardcoded secrets, …). |
| `mollify fix [--apply]` | Remove `certain` + `auto_fixable` unused symbols. Dry-run unless `--apply`. |
| `mollify init` | Write a starter `.mollifyrc.json`. |
| `mollify mcp` | Run the MCP stdio server (for coding agents). |

## Global flags (per analysis command)
- `--path <dir>` — project root (default `.`).
- `--format human|json|sarif` — output format (default `human`). `json` is the kind-discriminated contract; `sarif` is SARIF 2.1.0 for code scanning.
- `--gate all|new-only` — `new-only` keeps only findings in changed files (introduced).
- `--base <ref>` — git base ref for `--gate new-only` (e.g. `origin/main`).

## Exit codes
- `0` — no `error`-severity findings.
- `1` — one or more `error`-severity findings (CI gate) or a command error.

Severities are `warn` by default; raise rules/categories to `error` in `.mollifyrc.json` to gate CI.

## Rules emitted
`unused-file`, `unused-export`, `unused-dependency`, `missing-dependency`,
`circular-dependency`, `high-complexity`, `duplication`, `untyped-function`, `dangerous-eval`, `subprocess-shell-true`, `unsafe-yaml-load`, `unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret`.

## `.mollifyrc.json`
```json
{
  "severity": { "dead-code": "error", "duplication": "warn", "unused-dependency": "off" },
  "ignore": ["tests/", "migrations/"],
  "max_cyclomatic": 10,
  "max_cognitive": 15
}
```
`severity` keys are rule ids or category names (`dead-code`, `duplication`,
`circular-dependency`, `complexity`, `architecture`, `dependency-hygiene`, `type-health`, `security`).

## Not yet implemented (do not rely on)
Line-level gate attribution (current gate is file-level), named architecture
presets, churn×complexity ranking, LSP, runtime/type intelligence. See docs/STATUS.md.
