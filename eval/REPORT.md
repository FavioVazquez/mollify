# Corpus evaluation — pass 1 (smoke set)

**Date:** 2026-07-05 · **mollify:** release build @ branch
`claude/python-testing-codebases-a19ljf` · **Corpus:** 5 repos pinned in
`corpus.lock` (requests, flask, rich, MediaCrawler, MoneyPrinterTurbo) ·
**Engines:** audit, dead-code, deps, arch, complexity, dupes, types, security
(coverage and supply-chain deferred — they need external inputs).

## Verdict

The engine core is solid: **zero crashes, zero non-zero exits, empty stderr
across all 80 runs; byte-identical output on re-run; excellent performance**
(≤2.5 s and ≤52 MB peak RSS per full audit). Two real defects found, one of
them P1: **`unused-import` produces wrong `Certain` + `auto_fixable`
findings** on three deliberate-import idioms, so `mollify fix --apply` would
break flask. Signal-to-noise on groomed Tier-1/2 code needs tuning.

## Metrics (audit engine, run1)

| repo | py files | KLOC | wall s | RSS MB | findings | /KLOC | certain | likely | uncertain |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| requests | 37 | 12.0 | 0.46 | 19 | 740 | 62 | 17 | 419 | 304 |
| flask | 83 | 18.3 | 0.31 | 24 | 935 | 51 | 35 | 631 | 269 |
| moneyprinterturbo | 48 | 14.3 | 2.50 | 22 | 284 | 20 | 23 | 180 | 81 |
| mediacrawler | 165 | 26.9 | 1.26 | 29 | 666 | 25 | 71 | 334 | 261 |
| rich | 213 | 51.9 | 1.33 | 52 | 669 | 13 | 77 | 371 | 78 |

Sanity property that held everywhere: `audit` findings = exact sum of the
seven per-engine reports. Envelope is the versioned contract
(`schema_version: "0.1"`), and every finding carries fingerprint +
confidence + reason (invariants #3, #5 hold).

## Defects found

### D1 (P1) — wrong `Certain` on deliberate imports; auto-fix would break flask

`unused-import` is `Certain` + `auto_fixable` — the only class the
candidate/verifier contract allows to auto-apply — yet it fires on three
idioms that are deliberate:

1. **PEP 484 explicit re-export** — `flask/src/flask/blueprints.py:11`
   `from .sansio.blueprints import BlueprintSetupState as BlueprintSetupState  # noqa`.
   The `X as X` spelling *is* the re-export convention.
2. **Entry-point import** — `flask/tests/test_apps/helloworld/wsgi.py:1`
   `from hello import app  # noqa: F401`. The WSGI-server idiom; the import
   is the module's entire purpose.
3. **try/except probe + cross-module re-export** —
   `requests/tests/compat.py:4,6` (`try: import StringIO / except
   ImportError: import io as StringIO`). Both arms flagged; the name is
   imported *from* this module by `tests/test_utils.py:47` and
   `tests/test_requests.py:58`, so the fix breaks two other files.

Confirmed end-to-end: `mollify fix` (dry-run) on flask proposes exactly
these two deletions as "safe". Fix directions, roughly in order of value:
respect `# noqa`/`# noqa: F401`, treat `X as X` as a re-export (never
unused), never mark imports inside `try/except ImportError` above
`Uncertain`, and account for `from <module> import <name>` consumers before
calling an import unused in re-export/compat modules.

Positive control: fingerprint sameness means one suppression fixes both the
engine report and the audit report; and the 20 `Certain` unused-imports on
MediaCrawler (Tier 4) spot-checked as plausible true positives — the rule
has real signal, it is the calibration that is off.

### D2 (P2) — output paths echo the `--path` spelling

`--path .` vs `--path /abs/dir` on the same tree produce different bytes:
`location.path` is emitted as given (`./x.py` vs `/home/.../x.py`).
Run-to-run determinism holds, and fingerprints are path-spelling-independent
(187/187 overlap on requests — baselines port across machines), but
"identical input → byte-identical output" should not depend on how the path
was spelled, and machine-specific absolute paths in JSON hurt CI diffing.
Fix: emit paths relative to the resolved project root.

## Signal-to-noise (needs design attention, not a bug)

62 findings/KLOC on **requests** — the most-groomed repo in the corpus —
is unusable as a default report. Three drivers, all defensible individually:

- **Test/doc trees dominate:** 110/187 of requests' and 162/245 of flask's
  dead-code findings point into `tests/`, `docs/`, or `examples/`.
  116 of requests' 130 security findings are `request-without-timeout`,
  overwhelmingly in its own test suite.
- **types engine on untyped-by-choice libs:** 327 findings on requests —
  technically true, but a per-function finding for a library that never
  adopted typing reads as noise; a per-project rollup would carry the same
  evidence in one finding.
- **`unused-parameter` on interface-required params:** 100 on flask —
  callbacks and overrides must keep signatures; needs an override/protocol
  heuristic.

Tier contrast is encouraging: findings/KLOC on messy Tier-4 repos (20–25)
vs rich (13) — but requests/flask being *noisier* than MediaCrawler inverts
the expected gradient purely because of the test-tree and typing effects
above. Likely directions: default path-class tags (src/test/doc/example)
with test-tree down-weighting for security + dead-code, and rollup findings
for project-wide conditions.

## Determinism detail

- run1 vs run2 (all 5 repos × 8 engines): `cmp` byte-identical, 40/40.
- Same absolute `--path` from different CWDs: identical.
- Relative vs absolute `--path`: differs (D2).

## Next steps

1. Fix D1 (respect noqa, `X as X`, try/except imports, cross-module
   re-export consumers) — add corpus-distilled fixtures to the test suite.
2. Fix D2 (root-relative paths).
3. Re-run pass 1 and diff — corpus doubles as the regression harness.
4. Extend to Tier 2/3 (pydantic, fastapi for re-export stress; django,
   home-assistant for scale) once D1 lands, since Tier 2 is where re-export
   idioms are densest.
5. Cross-tool comparison (vulture, deptry, radon, bandit) — not yet run.
