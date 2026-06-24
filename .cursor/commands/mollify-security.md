Scan this repository for security candidates with Mollify and summarize the findings. Mollify emits evidence (stable fingerprint, confidence tier, reason), not decisions. You are the verifier.

Steps:
1. Run `mollify security --format json` (or call the mollify MCP `mollify_security` tool). Add `--path <dir>` if a subproject was specified.
2. Group findings by `rule` (`dangerous-eval`, `subprocess-shell-true`, `sql-injection`, `unsafe-yaml-load`, `unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret`, `weak-hash`, `weak-cipher`, `insecure-random`, `request-without-timeout`) and by `confidence`, with counts.
3. List each finding with its file:line, its `reason`, and the `fingerprint`.
4. Do NOT modify files. Present a remediation plan ordered by severity and ask for approval first.

Notes:
- These are candidates, not confirmed vulnerabilities — verify each in context.
- Act automatically only on `confidence: certain`; surface `likely` / `uncertain` and let the user decide.
