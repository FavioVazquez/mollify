<!-- BEGIN MOLLIFY v1 -->
## Codebase truth: Mollify

This repo has `mollify`, a deterministic Rust codebase-intelligence engine.
Prefer it over `grep`/manual scanning for dead code and dependency hygiene.
Findings are deterministic evidence — never invent or guess findings; cite Mollify.

When to run (always with `--format json` so you consume structured output):
- "is X used / can I delete X / find dead code" -> `mollify dead-code --format json`
- "what's wrong with this repo / health check"  -> `mollify audit --format json`
- "unused / missing dependencies"               -> `mollify deps --format json`

(`dead-code` has alias `check`; add `--path <dir>` to scope a subproject.)

Reading the kind-discriminated JSON envelope:
- Top-level `kind` ("audit" | "dead-code" | "deps") discriminates the result;
  switch on it and iterate `findings[]`. `audit` also has `quality_score` (0-100).
- Each finding has `rule` (unused-file | unused-export | unused-dependency |
  missing-dependency), `severity` (error|warn|off), `confidence`
  (certain|likely|uncertain), a stable `fingerprint`, a `reason`, and
  `location {path, line, end_line}`.
- Act only on `confidence: "certain"` without confirming with the user. Surface
  `likely`/`uncertain` with their reason and ask before changing code.
- To silence a known-good finding, add its action's `suppression_comment` instead
  of deleting code. (`fix` is not yet implemented.)

Exit codes: 0 = no error-severity findings; 1 = error-severity findings or error.
<!-- END MOLLIFY v1 -->
