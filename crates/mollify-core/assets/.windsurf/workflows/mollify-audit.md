# Mollify audit

Read-only Mollify triage of the current repository.

1. Run `mollify audit --format json` (add `--path <dir>` to scope a subproject).
2. Parse the envelope: read `quality_score` and iterate `findings[]`.
3. Produce a table grouped by `category` (dead-code, dependency-hygiene) with
   counts by `confidence`.
4. For each `confidence: certain` finding, show `path:line`, the `reason`, and the
   `fingerprint`.
5. Do NOT modify any files. End with a verdict: PR-ready / needs cleanup. If
   cleanup is needed, suggest running `/mollify-cleanup`.
