Apply safe automated fixes with Mollify, gated on user approval. Mollify only auto-fixes `certain` + `auto_fixable` findings (unused symbols and unused imports).

Steps:
1. Run `mollify fix` (dry-run, no `--apply`) to preview what would change. Add `--path <dir>` if a subproject was specified.
2. Present the proposed changes — one line per finding: `fingerprint`, file:line, the action `description`. WAIT for explicit approval.
3. On approval, run `mollify fix --apply` to apply the changes.
4. Re-run `mollify audit --format json` to confirm the fingerprints are gone and no new findings were introduced, then run the test suite.
5. Summarize: resolved fingerprints, remaining findings.

Notes:
- `mollify fix` never touches `likely` / `uncertain` findings — handle those manually or via `suppression_comment`.
