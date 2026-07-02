# Recipe 02 — Anatomy of a finding

**Goal:** understand *why* Mollify reported something and how much to trust it —
so you can act with confidence instead of guessing.

Mollify's one rule: **no AI invents findings.** Every result is a deterministic
piece of evidence. Three things tell you what to do with it: the **rule**, the
**confidence tier**, and the **reason**.

## Ask the tool what a rule means

`mollify explain <rule>` describes the rule's semantics, its confidence, and how
to act:

```bash
mollify explain unused-export
```

```text
unused-export
  A top-level function/class never referenced outside its own module and not listed in `__all__`. Confidence: likely (dynamic access via getattr downgrades it). Reachability roots are exempt: framework-registered symbols, pytest `test_*`/`Test*` in test paths (honoring `[tool.pytest.ini_options].testpaths`), and functions named by a `[project.scripts]` entry point. Action: remove it or make it private.
```

Run `mollify explain` with no argument to print the entire rule catalog — handy
the first time you meet an unfamiliar rule id in a report.

## Confidence tiers — the honesty knob

| Tier | Meaning | Auto-fixable? |
|------|---------|---------------|
| `certain` | Provable. A private symbol with no dynamic dispatch in scope. | ✅ yes |
| `likely` | Strong static signal, small residual dynamic risk. | — suggested |
| `uncertain` | Public surface, or the module uses `getattr`/`eval`/`importlib`. | — review only |

You can see the tiers diverge in the sample. Both of these are dead exports, but:

```text
./billing/app.py:12 [warn/certain] unused-export — function `_legacy_helper` …
./billing/app.py:7  [warn/likely]  unused-export — function `main` …
```

`_legacy_helper` is **certain** — it's private (`_`-prefixed) and provably
unreferenced, so it's safe to delete automatically. `main` is only **likely**:
it's public, so something outside the project (an entry point, a test, a
`getattr`) *could* reach it. Mollify won't auto-remove it — that's your call.

Filter to just the safe, provable findings:

```bash
mollify dead-code --min-confidence certain
```

## Framework awareness kills the #1 false positive

A naive "unused function" check screams about every Flask route and pytest
fixture. Mollify understands decorators from Flask/FastAPI/Django/Celery/pytest/
click/Pydantic and treats decorated entry points as *reachable* — so it doesn't
flag your `@app.get("/")` handler as dead. That's why its `certain` tier is
actually trustworthy.

## Trust, but verify — `inspect`

Want the full evidence bundle for a single file (its findings *plus* its import
neighborhood, so you can see why something is or isn't reachable)?

```bash
mollify inspect billing/app.py
```

**Next:** [Recipe 03 — Clean up dead code, safely »](03-clean-dead-code.md)
