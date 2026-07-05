# Corpus evaluation — running mollify on real codebases

Mollify has so far been verified against its own test suite and small
fixtures. This directory holds the missing step: running every engine against
real, living Python codebases — mature and messy alike — and judging the
output. The model is ruff's *ecosystem check* and mypy's `mypy_primer` (149
projects, ~10M lines): a pinned corpus of real projects that doubles as a
regression harness between mollify versions.

## Layout

```
eval/
  README.md        this plan (committed)
  corpus.tsv       repo manifest: tier, name, clone URL (committed)
  corpus.lock      name → pinned commit, written on first clone (committed)
  clone-corpus.sh  clones the corpus at the pinned commits (committed)
  corpus/          the cloned third-party repos        (GITIGNORED)
  results/         raw per-repo, per-engine JSON runs  (GITIGNORED)
  REPORT.md        curated findings from a corpus pass (committed, written later)
```

Third-party code and raw outputs never enter git history — only the manifest,
the lock file, and hand-curated conclusions do.

## The corpus (19 repos, 4 tiers)

Chosen from the canonical mature-package set (top-PyPI / mypy_primer overlap)
plus currently trending GitHub repos, so we see both ends of code hygiene.

**Tier 1 — small mature libraries** (requests, click, httpx, attrs).
Fast iteration targets. These are intensely groomed; mollify should be
*quiet* here. Every finding on Tier 1 is worth reading — a flood of findings
on requests almost certainly means false positives.

**Tier 2 — medium mature, modern typing/packaging** (flask, pydantic, rich,
fastapi, black). Stress the types engine (pydantic is aggressively typed),
`__all__`/re-export handling (fastapi and rich have huge re-export surfaces —
the classic dead-code false-positive trap), and src-layout/package detection.

**Tier 3 — large mature applications** (django, pandas, airflow,
home-assistant, zulip). The scale and precision stress test: django's
meta-programming and dynamic dispatch, pandas' C-extension boundary, airflow's
provider-package monorepo (architecture + deps engines), home-assistant's
>1M LOC and thousands of integrations (the performance ceiling), zulip as a
big real-world Django app that mypy_primer also tracks.

**Tier 4 — trending, less mature** (MoneyPrinterTurbo ~96k★,
MediaCrawler ~55k★, strix ~37k★, supervision ~47k★, LMCache ~10k★ — from
GitHub trending, July 2026). App-style, fast-moving codebases where real dead
code, duplication, and dependency drift actually live. This is where mollify
should *earn its keep* with true positives.

Smoke subset for the first pass: `requests`, `rich`, `flask`,
`moneyprinterturbo`, `mediacrawler` (one per interesting shape, all small
enough to hand-audit).

## What to look for (evaluation dimensions)

1. **Robustness.** No panics, no hangs, sane exit codes on every repo. Count
   files that fail to parse (airflow and home-assistant use current syntax:
   PEP 695 generics, `match`, walrus). A crash on any corpus repo is a P0 bug.
2. **Determinism** (invariant #1). Run each engine twice per repo and `diff`
   the JSON: must be byte-identical. Also re-run from a different CWD with
   `--path` to catch path-ordering leaks.
3. **Performance.** Wall time and peak RSS (`/usr/bin/time -v`) per repo,
   plotted against LOC. Tier 3 defines the scaling curve; home-assistant is
   the ceiling. Record numbers in REPORT.md so regressions are visible.
4. **Precision by confidence** (invariant #2). Hand-audit a random sample
   (~10 findings per engine per repo, seeded/sorted selection for
   reproducibility). `Certain` findings must be ~100% correct — any wrong
   `Certain` is a calibration bug, because only `Certain` + `auto_fixable`
   may auto-apply. Lower confidences may be wrong but must carry honest
   evidence/reason fields.
5. **Known false-positive traps** — check dead-code output specifically for:
   pytest fixtures (used by name), Django model/`Meta`/signal machinery,
   celery tasks, entry-point/plugin registrations, `__all__` re-exports,
   `TYPE_CHECKING`-only imports, platform-conditional code, namespace
   packages, optional-extra dependencies (deps engine).
6. **Cross-tool agreement.** Run vulture (dead code), deptry (deps), radon
   (complexity), bandit (security) on the same pinned commits. Disagreements
   are not verdicts — they are the audit queue. Where mollify finds strictly
   more with evidence, that is a win to document; where it finds less,
   explain why (higher confidence bar is a valid answer).
7. **Signal-to-noise.** Findings per KLOC per engine per tier. If `audit` on
   django emits tens of thousands of findings, the tool is unusable
   regardless of precision; tune default severities/confidence gates from
   this data.
8. **Contract stability** (invariants #3, #5). Every JSON output must be the
   versioned kind-discriminated envelope; every finding carries fingerprint +
   confidence + reason. Fingerprint stability check: touch an unrelated file,
   re-run, and confirm fingerprints of untouched findings did not move
   (baseline workflow depends on this: `--save-baseline` / `--baseline`).
9. **Fix safety (stretch).** On a Tier 4 repo: `mollify fix --apply`, then
   run that repo's own test suite. Green tests after auto-fix is the whole
   candidate/verifier promise made concrete.

## How to run a pass

```sh
eval/clone-corpus.sh 1 moneyprinterturbo mediacrawler   # smoke set
cargo build --release
for repo in eval/corpus/*/; do
  name=$(basename "$repo")
  mkdir -p "eval/results/$name"
  for engine in audit dead-code deps arch complexity dupes types security; do
    /usr/bin/time -v target/release/mollify "$engine" --path "$repo" --format json \
      > "eval/results/$name/$engine.json" 2> "eval/results/$name/$engine.time"
  done
done
```

Then a determinism re-run (`diff` against the first), the sampled hand-audit,
and the cross-tool comparison. Conclusions, metrics tables, and every
confirmed bug go into `eval/REPORT.md`; bugs get filed and fixed with a
minimal fixture distilled from the corpus repo (the corpus itself stays out
of the test suite — tests must not depend on network clones).

## Managing the output volume

Raw JSON will be large (Tier 3 especially) — that is why `results/` is
gitignored. The committed artifacts are: per-repo metric rows (LOC, wall
time, RSS, findings-per-engine, findings/KLOC), the sampled precision table
(engine × confidence → correct/incorrect), and prose for anything surprising.
Once a pass looks good, `--save-baseline` snapshots per repo turn the corpus
into a standing regression harness: future mollify changes re-run the corpus
and diff against baselines, ruff-ecosystem-check style.
