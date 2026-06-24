# Mollify CLI reference

> Status: Phase 1 MVP. Commands below are implemented and tested.

## Commands
| Command | Description |
|---|---|
| `mollify audit` | Unified report (dead-code + dependency hygiene today) + `quality_score`. |
| `mollify dead-code` (alias `check`) | Reachability-based unused files and symbols. |
| `mollify deps` | Dependency hygiene: unused / missing distributions. |
| `mollify init` | Write a starter `.mollifyrc.json`. |
| `mollify mcp` | Launch the stdio MCP server (same JSON contract as the CLI). |

## Global flags (per command)
- `--path <dir>` — project root to analyze (default `.`).
- `--format human|json` — output format (default `human`). `json` is the
  kind-discriminated contract.

## Exit codes
- `0` — no `error`-severity findings (all current dead-code/deps findings are
  `warn` by default).
- `1` — one or more `error`-severity findings (CI gate), or a command error.

## Rules emitted
- `unused-file` — module never imported and not an entry point.
- `unused-export` — top-level function/class/variable with no reachable references.
- `unused-dependency` — declared in `pyproject.toml` but never imported.
- `missing-dependency` — imported (external, non-stdlib) but not declared.

## Not yet implemented (do not rely on)
`--gate new-only`, SARIF output, `fix`, framework entry-point plugins, and
`.mollifyrc` being read by analysis.
