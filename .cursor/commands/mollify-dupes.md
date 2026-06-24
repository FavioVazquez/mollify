Find duplicated / cloned code in this repository with Mollify and summarize the findings. Mollify emits evidence (stable fingerprint, confidence tier, reason), not decisions. You are the verifier.

Steps:
1. Run `mollify dupes --format json` (or call the mollify MCP `mollify_dupes` tool). Add `--path <dir>` if a subproject was specified.
2. Group findings by `confidence`, with counts. The `rule` is `duplication`.
3. For each clone family, list the member locations (file:line), the `reason`, and the `fingerprint`.
4. Do NOT refactor anything. Suggest a de-duplication plan and ask for approval first.

Notes:
- Duplication is token-based clone detection; some clones are intentional — let the user decide.
- Act automatically only on `confidence: certain`.
