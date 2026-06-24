Run a full Mollify audit of this repository and summarize the highest-priority findings. Mollify is a deterministic candidate-producer: it emits evidence (each finding has a stable fingerprint, a confidence tier, and a reason), not decisions. You are the verifier.

Steps:
1. Run `mollify audit --format json` (or call the mollify MCP `audit` tool). Add `--path <dir>` if a subproject was specified.
2. Group the findings by `category` and by `confidence` (`certain` / `likely` / `uncertain`), with counts.
3. List every `certain` dead-code finding with its file:line, its `reason`, and whether the action's `auto_fixable` is true.
4. Do the same for dependency-hygiene findings (`unused-dependency`, `missing-dependency`), noting that `missing-dependency` can be a false positive for namespace packages or local shadowing.
5. Do NOT apply any changes. Present a plan and ask for approval first. Preview any proposed deletion (file, line, reason) before the user confirms.

Notes:
- The audit envelope has top-level `kind`, `schema_version`, `summary`, `quality_score` (0-100), and `findings[]`. Each finding has `rule`, `category`, `confidence`, `severity`, `reason`, `location`, `fingerprint`, and `actions[]`.
- Act automatically only on `confidence: certain`; surface `likely` / `uncertain` with their reason and let the user decide.
- To silence a known-good finding, suggest adding its `suppression_comment` on the relevant line instead of deleting code.
