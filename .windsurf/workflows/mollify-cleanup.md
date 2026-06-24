# Mollify cleanup

Guided remediation of high-confidence Mollify findings. Edits are gated on your approval.

1. Run `mollify audit --format json` to get current findings.
2. Filter to `confidence: certain` findings whose action has `auto_fixable: true`.
3. Present a remediation plan — one line per finding: `fingerprint`, file:line,
   the action `description`. WAIT for explicit approval before editing.
4. For each approved finding: apply the action (e.g. delete the unused symbol).
   For anything `likely`/`uncertain`, do NOT auto-edit — either ask, or add the
   finding's `suppression_comment` on that line if the user says it is intentional.
5. After edits, re-run `mollify audit --format json` and confirm the fingerprints
   are gone and no new findings were introduced. Then run the test suite.
6. Summarize: resolved fingerprints, remaining findings.
