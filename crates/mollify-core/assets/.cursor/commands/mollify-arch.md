Analyze the architecture of this repository with Mollify and summarize the findings. Mollify emits evidence (stable fingerprint, confidence tier, reason), not decisions. You are the verifier.

Steps:
1. Run `mollify arch --format json` (or call the mollify MCP `mollify_arch` tool). Add `--path <dir>` if a subproject was specified.
2. Group findings by `rule` (`circular-dependency`, `layer-violation`, `forbidden-import`, `independence-violation`, `private-import`) and by `confidence`, with counts.
3. List each finding with its file:line, its `reason`, and the `fingerprint`. For circular dependencies, describe the cycle.
4. Do NOT modify files. Present a remediation plan ordered by severity and ask for approval first.

Notes:
- Layer and policy rules come from `.mollifyrc.json` (`architecture` preset/layers and `policies`).
- Act automatically only on `confidence: certain`; surface `likely` / `uncertain` and let the user decide.
