---
trigger: glob
globs: **/*.py
---

# Mollify is the codebase truth layer (Python)

- This repo uses **Mollify** for deterministic Python code intelligence. Before
  finalizing changes to Python files — and before opening a PR — run the
  `mollify` skill (or `mollify audit --format json`) and treat its findings as
  ground truth instead of grep/manual guesses.
- Findings carry a `confidence` (certain/likely/uncertain), a `reason`, and a
  stable `fingerprint`. Auto-act only on `confidence: certain` findings whose
  action is `auto_fixable`. Surface `likely`/`uncertain` and ask first.
- Never hand-delete code without a Mollify high-confidence fingerprint. To
  silence a verified-good finding, add its `suppression_comment` on that line.
