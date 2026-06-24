Report project-wide quantitative metrics with Mollify and summarize them. Mollify is a deterministic engine: metrics are computed, not estimated. You are the verifier.

Steps:
1. Run `mollify metrics --format json` (or call the mollify MCP `mollify_metrics` tool). Add `--path <dir>` if a subproject was specified.
2. Summarize the headline numbers: total lines of code, file count, symbol count, the complexity distribution, and the per-category finding tallies.
3. Call out any outliers (e.g. unusually large files, heavy complexity buckets) and tie them back to the relevant analysis command (`complexity`, `dupes`, `dead-code`) the user could run next.
4. Do NOT modify any files — this is read-only reporting.

Notes:
- `metrics` is a quantitative summary; for actionable findings run `mollify audit` or a specific engine.
- All numbers are deterministic for a given input — identical input yields byte-identical output.
