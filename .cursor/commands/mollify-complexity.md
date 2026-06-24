Find complexity hotspots in this repository with Mollify and summarize the findings. Mollify emits evidence (stable fingerprint, confidence tier, reason), not decisions. You are the verifier.

Steps:
1. Run `mollify complexity --format json` (alias `mollify health`; or call the mollify MCP `mollify_complexity` tool). Add `--path <dir>` if a subproject was specified.
2. Group findings by `rule` (`high-complexity`, `hotspot`) and by `confidence`, with counts.
3. List each finding with its file:line, its `reason` (cyclomatic/cognitive metrics, churn×complexity), and the `fingerprint`.
4. Do NOT refactor anything. Present a prioritized plan and ask for approval first.

Notes:
- Thresholds (`max_cyclomatic`, `max_cognitive`) come from `.mollifyrc.json`.
- `hotspot` combines churn with complexity to rank refactor candidates.
