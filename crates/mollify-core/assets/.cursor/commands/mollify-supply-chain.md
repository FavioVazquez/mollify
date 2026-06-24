Check this repository's dependencies against known vulnerabilities with Mollify and summarize the findings. Mollify emits evidence (stable fingerprint, confidence tier, reason), not decisions. You are the verifier.

Steps:
1. Run `mollify supply-chain --format json` (or call the mollify MCP `mollify_supply_chain` tool). It queries live OSV by default; add `--offline` to use the bundled DB, `--refresh` to refresh it, or `--advisory-db <f>` for a custom DB. Add `--path <dir>` if a subproject was specified.
2. Group findings by `confidence`, with counts. The `rule` is `vulnerable-dependency`.
3. List each finding with the affected package/version, its `reason` (advisory id), and the `fingerprint`.
4. Do NOT bump versions automatically. Present an upgrade plan and ask for approval first.

Notes:
- Findings come from pinned/locked versions vs OSV; verify the advisory applies to your usage.
