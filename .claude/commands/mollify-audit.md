---
description: Run a full mollify audit and summarize findings (read-only)
argument-hint: "[--path <dir>]"
disable-model-invocation: true
allowed-tools: Bash(mollify *)
---

## Mollify report
!`mollify audit $ARGUMENTS --format json`

## Task
Summarize the audit output above. Lead with `confidence: certain` findings, give
`path:line` (and the `fingerprint`) for each, and group by `rule`. Note which
actions have `auto_fixable: true`. For anything `likely` / `uncertain`, explain
the reason and let the user decide.

Do NOT edit any files in this command — it is read-only triage. If cleanup is
warranted, suggest running `/mollify-cleanup`.
