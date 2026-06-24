# Project memory

## Codebase intelligence (Mollify)

This repo ships **Mollify**, a deterministic, Rust-native Python code-intelligence
CLI (the `mollify` binary on PATH) plus an MCP server (`mollify mcp`). Treat it as
the source of truth for Python **dead code** and **dependency hygiene** — prefer it
over `grep` or manual scanning when judging whether code is used, what is safe to
delete, or whether dependencies are unused/missing.

- Prefer Mollify over grep for reachability/usage and dependency questions.
- Run `/mollify:audit`, or call the CLI directly: `mollify audit --format json`.
  Scoped commands: `mollify dead-code --format json` (alias `mollify check`) and
  `mollify deps --format json`. Add `--path <dir>` to target a subproject.
- Trust the deterministic findings. Each finding carries a `confidence` tier
  (`certain` | `likely` | `uncertain`), a human `reason`, a stable `fingerprint`,
  a `severity` (`error` | `warn` | `off`), and a `location {path, line, end_line}`.
  Rules: `unused-file`, `unused-export`, `unused-dependency`, `missing-dependency`.
- Read the JSON envelope by its top-level `kind` (`audit` | `dead-code` | `deps`);
  `audit` also includes a `quality_score` (0–100). Iterate `findings[]`.
- Auto-act ONLY on `confidence: certain` (and only where an action is
  `auto_fixable: true`). Surface `likely`/`uncertain` findings with their reason
  and let the user decide; never hand-delete code on a guess.
- Exit codes: `0` = no error-severity findings; non-zero = error-severity findings
  or a command error (useful as a CI gate).
