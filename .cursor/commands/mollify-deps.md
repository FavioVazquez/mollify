Check dependency hygiene in this repository with Mollify and summarize the findings. Mollify emits evidence (stable fingerprint, confidence tier, reason), not decisions. You are the verifier.

Steps:
1. Run `mollify deps --format json` (or call the mollify MCP `mollify_deps` tool). Add `--path <dir>` if a subproject was specified.
2. Group findings by `rule` (`unused-dependency`, `missing-dependency`) and by `confidence`, with counts.
3. List each finding with its file:line, its `reason`, and the `fingerprint`.
4. Do NOT edit `pyproject.toml` or lockfiles. Present a plan and ask for approval first.

Notes:
- `missing-dependency` can be a false positive for namespace packages or local shadowing; verify before adding to `pyproject.toml`.
- Act automatically only on `confidence: certain`; surface `likely` / `uncertain` with their reason and let the user decide.
