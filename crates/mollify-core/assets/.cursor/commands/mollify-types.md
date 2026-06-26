Check type-annotation health in this repository with Mollify and summarize the findings. Mollify emits evidence (stable fingerprint, confidence tier, reason), not decisions. You are the verifier.

Steps:
1. Run `mollify types --format json` (or call the mollify MCP `mollify_types` tool). Add `--path <dir>` if a subproject was specified.
2. Group findings by `rule` (`untyped-function` for fully-untyped public functions, `private-type-leak` for public signatures exposing a private `_Type`) and by `confidence`, with counts.
3. List each finding with its file:line, its `reason`, and the `fingerprint`.
4. Do NOT add annotations automatically. Present a plan and ask for approval first.

Notes:
- Act automatically only on `confidence: certain`; surface `likely` / `uncertain` and let the user decide.
