Find dead code in this repository with Mollify and summarize the findings. Mollify is a deterministic candidate-producer: it emits evidence (each finding has a stable fingerprint, a confidence tier, and a reason), not decisions. You are the verifier.

Steps:
1. Run `mollify dead-code --format json` (alias `mollify check`; or call the mollify MCP `mollify_dead_code` tool). Add `--path <dir>` if a subproject was specified.
2. Group findings by `rule` (`unused-file`, `unused-export`, `unused-import`, `commented-code`) and by `confidence` (`certain` / `likely` / `uncertain`), with counts.
3. List every `certain` finding with its file:line, its `reason`, the `fingerprint`, and whether the action's `auto_fixable` is true.
4. Do NOT apply any changes. Present a plan and ask for approval first; preview any proposed deletion (file, line, reason) before the user confirms.

Notes:
- Reachability is static; dynamic imports (`getattr`/`importlib`) downgrade confidence to `uncertain` — treat those as review-only.
- `mollify fix --apply` auto-removes `certain` + `auto_fixable` unused symbols and imports. To silence a known-good finding, suggest its `suppression_comment` instead of deleting code.
