# Recipe 05 — Gate a PR on *new* issues only

**Goal:** add Mollify to CI without drowning in pre-existing debt. The winning
move on a legacy codebase isn't "fix all 500 findings" — it's **"don't add a
501st."** Mollify does this two ways.

## Option A — regression baselines (no git required)

Snapshot today's findings once, then fail CI only when a *new* fingerprint
appears. Because fingerprints are content-derived, this survives file moves and
reformatting.

**1. On a clean main, save the baseline:**

```bash
cd cookbook/sample-project
mollify audit --save-baseline .mollify/baseline.json
```

```text
Mollify audit — .
Quality score: 84/100
16 finding(s) across 7 file(s) — 0 error, 16 warn
  …
mollify: wrote baseline with 16 fingerprint(s) to .mollify/baseline.json
```

**2. In CI, compare against it and fail on regressions:**

```bash
mollify audit --baseline .mollify/baseline.json --fail-on-regression
```

As long as nothing new appears, this exits `0`. Now watch what happens when a PR
adds one dead function — here we append `def _brand_new_dead(): ...` and re-run:

```text
Mollify audit — .
Quality score: 99/100
1 finding(s) across 7 file(s) — 0 error, 1 warn
  billing/app.py:25 [warn/certain] unused-export — function `_brand_new_dead` has no reachable references in the project  (unused-export:026db265e8eaea13)
```

```bash
echo $?    # → 1
```

The other 16 findings are filtered out as known debt; CI flags **only the one
thing this PR introduced**, and exits non-zero. That's a review comment a
developer will actually read.

> Prefer advisory mode? `--brief` prints the same new-findings report but always
> exits `0` — visible signal, no hard block.

## Option B — diff against a git ref

If you'd rather scope by changed files than by a stored baseline:

```bash
mollify audit --gate new-only --base origin/main
```

Only findings in files this branch touched (vs `origin/main`) are reported.

## Wire it into GitHub Actions

Mollify speaks CI natively — SARIF for code scanning, GitHub annotations, JUnit
for dashboards:

```yaml
# .github/workflows/quality.yml
name: quality
on: pull_request
jobs:
  mollify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with: { fetch-depth: 0 }          # needed for --base diffing
      - run: pipx install mollify
      # Inline annotations on the PR's changed lines:
      - run: mollify audit --gate new-only --base origin/main --format github
      # …and upload SARIF to the Security tab:
      - run: mollify audit --format sarif > mollify.sarif
        if: always()
      - uses: github/codeql-action/upload-sarif@v4
        if: always()
        with: { sarif_file: mollify.sarif }
```

## Exit codes, precisely

`mollify` exits `0` when there are no **error**-severity findings, `1`
otherwise. Findings are `warn` by default — so to actually *block* a merge, raise
the rules you care about to `error` in `.mollifyrc.json`:

```json
{ "severity": { "dead-code": "error", "missing-dependency": "error" } }
```

…or use `--fail-on-regression` (Option A) to block on *new* findings regardless of
severity. Other formats — `--format junit` (CI dashboards) and `--format json`
(Recipe 07) — round out the integration surface.

**Next:** [Recipe 06 — Map your codebase »](06-map-your-codebase.md)
