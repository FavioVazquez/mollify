---
description: Run a mollify audit and propose fixes for safe findings (advisory; asks before editing)
argument-hint: "[--path <dir>]"
disable-model-invocation: true
allowed-tools: Bash(mollify *)
---

## Mollify report
!`mollify audit $ARGUMENTS --format json`

## Task
Review the audit output above and propose remediation. This is advisory: do NOT
edit files without explicit user approval.

1. Summarize findings, leading with `confidence: certain`; cite `path:line` and
   the `fingerprint`, grouped by `rule`.
2. Identify the findings whose action has `auto_fixable: true` AND whose
   `confidence` is `certain` — only these are safe to act on. For each, describe
   the exact edit you would make (the action `description`), and ask the user to
   confirm before you apply it.
3. For everything else (`likely` / `uncertain`, or any deletion), explain the
   reason/trace and let the user decide. To silence a known-good finding, offer
   to add its `suppression_comment` on the relevant line instead of deleting code.
4. After any approved edits, re-run `mollify audit --format json` and confirm the
   corresponding fingerprints are gone.

There is no `mollify fix` command — apply approved changes yourself with the
editor, then re-audit.
