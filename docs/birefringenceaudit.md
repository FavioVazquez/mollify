# Mollify accuracy findings — Birefringence audit

Source: ran `mollify audit` (v0.1.2) on **Birefringence** — a Python 3.11 package
(FastAPI service + statistics engine + pytest suite, ~1.6k LOC library / ~2.4k
LOC tests). Result: **20/100, 384 findings (0 error, 384 warn)**. Most are false
positives from a few root causes. Tasks below are ordered by impact.

## P1 — Intra-package relative imports don't resolve (root cause of ~200 findings)

- **Symptom:** every relative import in `birefringence/__init__.py` (`.aa`,
  `.decision`, `.metrics`, …) → `unresolved-import — does not resolve to any
  module in the project`, although the modules exist right beside `__init__.py`.
  This then cascades: those modules are judged `unused-file` ("never imported"),
  and their public functions become `unused-export` ("no reachable references"),
  because the cross-module reference graph never gets built.
- **Counts caused:** 9 `unresolved-import` + 11 `unused-file` + most of the 194
  `unused-export`.
- **Repro:** any package that re-exports submodules via relative imports in
  `__init__.py`.
- **Fix area:** the module/package resolver — resolve `.x` / `from . import x`
  against the current package directory before declaring it unresolved.
- **Why P1:** the quality score is not trustworthy until this is fixed.

## P1 — `session.exec` mis-flagged as `dangerous-eval` (CWE-95) [security FP]

- **Symptom:** SQLModel's `session.exec(select(...))` (in `store.py`) flagged as
  *"executes dynamic code [CWE-95]"*. There is no `eval`/`exec` in the package.
- **Cause:** name-match on any `.exec(` rather than the builtin `exec`.
- **Fix:** only flag the builtins `eval` / `exec` / `compile` (and maybe
  `os.system`, `subprocess(..., shell=True)` under their own rules). Don't match
  arbitrary methods named `exec` (ORMs, drivers, etc.).
- **Why P1:** a false *security* finding is the costliest kind (alarm fatigue,
  erodes trust in the whole report).

## P1 — pytest functions flagged `unused-export`

- **Symptom:** every `test_*` function reported "no reachable references." The
  suite is a standard `tests/` dir with `testpaths = ["tests"]` in
  `pyproject.toml`; advertised pytest-awareness didn't engage.
- **Fix:** treat `test_*` / `Test*` methods (and pytest fixtures) within test
  paths as reachability roots; honor `[tool.pytest.ini_options].testpaths`.

## P2 — `from __future__ import annotations` flagged `unused-import` (45×)

- 45 of 46 `unused-import` findings are this one future-import (has a runtime
  effect; never "unused"). Whitelist all `__future__` imports.

## P2 — Dependency analysis misses entry points, lazy & optional imports

- `unused-dependency` on `uvicorn` (console script via `[project.scripts]`,
  imported lazily inside `main()`) and `pymc` (lazy import inside the optional
  BEST lens, declared in an optional-dependency group).
- `missing-dependency` on `conftest` and `reference` — first-party test modules,
  not PyPI packages.
- **Fix:** read `[project.scripts]` entry points; count imports inside function
  bodies and `[project.optional-dependencies]`; treat first-party modules on the
  path (incl. `conftest.py`) as resolvable, not external.

## P3 — `commented-code` over-triggers on prose comments (5×)

- Flags explanatory English comments (e.g. `# from zero (proportion of draws on
  the wrong side of 0, doubled).`) as commented-out code.
- **Fix:** require real code structure (assignment / call / operator), not
  English that merely contains code-like fragments.

## P3 — Score weighting & first-run UX

- Fold confidence into the headline score (or exclude `uncertain` from it), and
  ship / auto-generate a starter `.mollifyrc.json` that ignores `tests/` for the
  export/typing rules. A first-time user currently sees 20/100, which undersells
  both the codebase and the tool.

---

## What already works well (don't regress these)

- **Complexity** (cyclomatic/cognitive) — accurately flagged the densest
  functions (`assess_trust` 30/22, `power.plan` 23/26, `build_scorecard` 20/7,
  `decide` 15/13). Well-prioritized.
- **`unused-parameter`** — found a genuinely dead parameter
  (`scorecard._narrate_rollup(..., guardrails)` never used). True positive.
- **`duplication`** — correctly spotted two near-identical t-tests (~159 tokens)
  and shared bootstrap/CI logic across `resampling`/`ratio`/`proportion`.
- **`private-import`** — caught a test reaching into the private `_hdi`. Fair.
- **`missing-dependency: sqlalchemy`** — correct: imported directly but only
  `sqlmodel` declared.
- Confidence tiers, stable finding IDs, CWE tags, churn×complexity hotspots, and
  SARIF / MCP outputs are all strong primitives — keep them.

**Net:** precision is the blocker, not the architecture. Landing the
import-resolution fix + the pytest/`__future__` whitelists + the `dangerous-eval`
tightening would likely move this from ~5% to ~70%+ actionable on this repo.
