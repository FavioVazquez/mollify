# Birefringence audit → Mollify precision plan

> **Status: implemented.** All P1–P3 items below have landed with tests
> (`cargo test`: 143 passing; `cargo clippy --all-targets`: clean). On a fixture
> reproducing Birefringence's patterns (package `__init__` re-exports, lazy
> import, `session.exec`, `from __future__`, pytest tests, prose comments,
> `[project.scripts]`), `mollify audit` moved from the audited **20/100** to
> **95/100**, with every remaining finding a genuine true positive (a truly
> unimported module, an undeclared `pytest`, untyped functions) or a defensible
> `uncertain` re-export note (matching ruff F401 semantics). See the
> per-item "Fix" sections for what changed; tests live beside each engine.


This plan turns the findings in `docs/birefringenceaudit.md` (real-world run of
`mollify audit` v0.1.2 on the *Birefringence* package — scored 20/100 with 384
findings, ~95% false positives) into concrete, code-grounded changes. Every root
cause below was traced to a specific location in the current tree; line numbers
are from the audit branch.

**Theme:** precision is the blocker, not the architecture. The single
import-resolution bug (P1.1) cascades into ~200 of the 384 findings. Landing
P1 + P2 should move this repo from ~5% to ~70%+ actionable, matching the audit's
own estimate.

Order = impact. P1 are correctness bugs that poison the score; P2 are systematic
false positives; P3 are polish/UX.

---

## P1.1 — Relative imports inside `__init__.py` never resolve  *(root cause of ~200 findings)*

**Confirmed root cause.** `dotted_name()` strips the `/__init__` suffix
(`crates/mollify-graph/src/lib.rs:279`), so `birefringence/__init__.py` gets the
dotted name `birefringence` — i.e. the *package itself*, not a module inside it.
But `resolve_relative()` (`lib.rs:637`) unconditionally drops `dots` trailing
segments, assuming the importer is a *module*:

```
resolve_relative("birefringence", 1, "aa")
  parts = ["birefringence"]; keep = 1 - 1 = 0; base = ""   // empty!
  → returns "aa"            // should be "birefringence.aa"
```

For a real module `a.b.c` (file `a/b/c.py`) dropping one segment to get the
package `a.b` is correct; for a package's `__init__.py` it drops one segment too
many. So every `.aa`, `.decision`, `from . import x` in an `__init__.py` resolves
to a top-level name that doesn't exist → `unresolved-import`, which then cascades:
the submodules look "never imported" (`unused-file`), and their exports get "no
reachable references" (`unused-export`).

**Fix.** Teach relative resolution whether the importer is a package
(`__init__.py`) or a module. For a package, the current package *is* its own
dotted name, so `dots=1` keeps all segments; `dots=2` drops one; etc. For a
module, keep today's behavior (`dots=1` drops the module's own segment).

Implementation options (prefer A):
- **A.** Add an `is_package: bool` to `ModuleInfo` (set from
  `path.file_name() == "__init__.py"` at build time, alongside `is_entry`), and
  pass it into `resolve_relative`. Effective drop count becomes
  `dots - 1` for a package, `dots` for a module. Update both call sites
  (`resolve_edges` `lib.rs:354` and `unresolved_imports` `lib.rs:420`).
- **B.** Normalize at the boundary: give `__init__.py` modules a sentinel dotted
  form so the existing arithmetic works. Rejected — it would corrupt
  `by_dotted` lookups and `unused_files`/`dotted` display.

**Tests** (`mollify-graph`):
- `resolve_relative` with an `is_package` flag: `(pkg="birefringence", pkg=true,
  dots=1, "aa") == "birefringence.aa"`; `dots=2` from a subpackage `__init__`
  goes up one real level.
- Integration: a package whose `__init__.py` re-exports submodules via `.x` /
  `from . import x` → 0 `unresolved-import`, 0 `unused-file`, exports reachable.
- Regression: keep `relative_import_resolution` (module case) green
  (`lib.rs:681`).

**Risk:** low and well-contained, but it is the highest-leverage change — verify
the cascade clears end-to-end on a fixture that mirrors Birefringence's layout.

---

## P1.2 — `session.exec(...)` mis-flagged as `dangerous-eval` (CWE-95)  *(security FP)*

**Confirmed root cause.** `security_call()` matches on `last` — the final
dotted segment — not the full callee (`crates/mollify-parse/src/lib.rs:1727`):

```rust
let last = f.rsplit('.').next().unwrap_or(f);
if (last == "eval" || last == "exec") && !first_positional_is_string(c) { … }
```

So SQLModel's `session.exec(select(...))` has `last == "exec"` → flagged. A false
*security* finding is the costliest kind (alarm fatigue).

**Fix.** Match only the builtins — the bare names with no receiver. Use the full
path `f`, not `last`:

```rust
if matches!(f, "eval" | "exec" | "compile") && !first_positional_is_string(c) { … }
```

(Optionally also accept `builtins.eval` / `builtins.exec` / `builtins.compile`.)
`os.system`, `subprocess(..., shell=True)`, etc. already have their own dedicated
rules, so nothing is lost. Note `compile` is currently not flagged at all; adding
it is a small true-positive bonus, but keep it out if we want a strictly minimal
diff.

**Tests** (`mollify-parse`): `session.exec(select(x))`,
`conn.exec(q)`, `obj.eval(expr)` → **no** `dangerous-eval`; bare `eval(user)` /
`exec(code)` → still flagged. Extend the existing assertions at `lib.rs:1987`.

**Risk:** very low. Pure tightening.

---

## P1.3 — pytest `test_*` functions flagged `unused-export`

**Confirmed root cause.** `is_entry()` marks `test_*` files as reachability roots
so the *file* isn't "unused" (`crates/mollify-graph/src/lib.rs:289`), but
`unused_symbols()` still runs `graph.symbol_used()` on every top-level def
(`crates/mollify-core/src/deadcode.rs:310`). A `def test_foo()` has no in-repo
caller and no framework decorator (`is_framework_entry` only matches *decorated*
defs — `crates/mollify-core/src/plugins.rs:84`), so every test is reported "no
reachable references." Advertised pytest-awareness never engages for plain test
functions.

**Fix.** Treat pytest entities in test paths as reachability roots in
`unused_symbols`: skip a def when its module is a test module **and** its name
matches `test_*` / class `Test*` (and, for completeness, methods named
`test_*` inside `Test*` classes). Reuse the test-path predicate that already
exists in deps (`is_test_module`, `deps.rs:564`) — promote it to a shared helper
(e.g. `crate::members::is_test_module` or a small `paths` module) so deadcode and
deps share one definition. Honor `[tool.pytest.ini_options].testpaths` from
`pyproject.toml` to widen "test path" beyond the `tests/` convention; fall back
to the convention when unset.

**Tests** (`mollify-core`): `tests/test_x.py::test_foo` and class
`TestThing::test_bar` → no `unused-export`; a genuinely dead non-test helper in
the same file → still flagged. A `testpaths = ["suite"]` pyproject → defs under
`suite/` are treated as tests.

**Risk:** low. Scope the exemption to test paths so production dead code is still
caught.

---

## P2.1 — `from __future__ import annotations` flagged `unused-import` (45×)

**Confirmed root cause.** `unused_imports()` checks each binding against
`local_uses` (`crates/mollify-core/src/deadcode.rs:160-170`). For
`from __future__ import annotations` the binding is `annotations`, never appears
in `local_uses`, so it's flagged — even though `__future__` imports have a
runtime/compiler effect and are *never* unused. 45 of 46 `unused-import` findings
were this one import.

**Fix.** Whitelist all `__future__` imports in `unused_imports`: skip any import
whose `module == "__future__"`. (Equivalently, set a flag on the `Import` at
parse time, but a module-name check in the one consumer is the smaller diff.)

**Tests** (`mollify-core`): a module with only
`from __future__ import annotations` → no `unused-import`; mixing a genuinely
unused stdlib import alongside it → the stdlib one still flagged.

**Risk:** trivial.

---

## P2.2 — Dependency analysis misses entry points, lazy & optional imports

Three distinct bugs reported under one heading; fix each.

### (a) Lazy / in-function imports not seen → `unused-dependency` on `uvicorn`, `pymc`
**Confirmed root cause.** `scan_top_level()` recurses into `If` and `Try` blocks
but **not** into `FunctionDef`/`ClassDef` bodies
(`crates/mollify-parse/src/lib.rs:473-571`). So an import lazily placed inside
`main()` (uvicorn) or inside an optional lens (pymc) is never added to
`m.imports`; `used_distributions()` (`deps.rs:413`) therefore never counts it and
the declared dep looks unused.

**Fix.** Collect nested imports, but keep them **separate** from top-level
imports so we don't regress `unused-import` (a function-local import used only in
its own function must not be judged against module-wide `local_uses`). Add a
distinct list — e.g. `ParsedModule.nested_imports: Vec<Import>` — populated by a
lightweight walk into function/class bodies (mark them e.g. `line`-only; no
`bindings` needed). Then:
- `used_distributions` / `module_imported_dists` iterate `imports` **+**
  `nested_imports` (dependency usage should see lazy imports).
- `unused_imports` keeps iterating **only** `imports` (unchanged).
- `unresolved_imports` / `resolve_edges`: include `nested_imports` too, since a
  lazy `from .x import y` is still a real internal edge — improves reachability.

### (b) `[project.scripts]` entry points ignored → `uvicorn` console script
**Confirmed gap.** Nothing reads `[project.scripts]` /
`[project.gui-scripts]` / Poetry `[tool.poetry.scripts]`. An entry point
`app = "birefringence.cli:main"` makes `birefringence.cli` a reachability root
and its imports "used."

**Fix.** Parse the scripts tables in `deps.rs` (and feed the referenced module to
the graph as a root for dead-code). At minimum: treat the `module` half of each
`pkg.mod:func` target as an entry module so its (now lazily-collected) imports
count toward `used_distributions`. Wiring the module as a graph reachability root
also helps `unused-file`/`unused-export` — do both.

### (c) First-party test modules flagged `missing-dependency` → `conftest`, `reference`
**Confirmed root cause.** `internal_top_levels()` collects only the **first**
dotted segment of each module (`deps.rs:399-409`). `tests/conftest.py` →
dotted `tests.conftest` → top `tests`. But pytest puts the test dir on
`sys.path`, so tests do `import conftest` / `from reference import x` by **bare
leaf name**; `conftest`/`reference` aren't in `internal_tops` → treated as
external → `missing-dependency`.

**Fix.** Also register each module's **leaf** name (file stem / last dotted
segment) as first-party — at least for modules under test paths, where bare-name
imports are idiomatic. Add leaf names to the `internal` set used by
`used_distributions`/`module_imported_dists`, and likewise let the graph's
first-party detection (`unresolved_imports`, `lib.rs:408-414`) know about leaf
names so these don't surface as `unresolved-import` either. Always treat
`conftest` as first-party.

**Tests** (`mollify-core`):
- (a) declared dep imported only inside a function body → **not** `unused`.
- (b) dep used only by a `[project.scripts]` entry module → **not** `unused`;
  entry module + its exports reachable.
- (c) `import conftest` / `from reference import x` from a test → **no**
  `missing-dependency`.

**Risk:** medium — most code touched. (a) needs care to avoid regressing
`unused-import`; the separate-list design isolates that. Add the nested walk
behind clear tests.

---

## P3.1 — `commented-code` over-triggers on prose (5×)

**Confirmed root cause.** `looks_like_code()` returns `true` immediately when a
comment starts with a statement keyword (`crates/mollify-core/src/commented.rs:33`),
**before** the prose guards on line 42. So
`# from zero (proportion of draws on the wrong side of 0, doubled).` matches the
`"from "` starter and is flagged, despite being English ending in a period.

**Fix.** Apply the prose guards to the keyword-starter path too, and tighten the
starters that double as English words (`from`, `for`, `with`, `import`, `del`,
`if`, `while`):
- reject if the body ends with `.` (sentence) or has too many words
  (`split_whitespace().count() > 12`), as the codeish path already does;
- for `from `/`import `, require it to actually look like an import
  (`from … import …` contains `" import "`; bare `import x` is a single short
  token list with no spaces-as-prose);
- keep unambiguous code starters (`def `, `class `, `return`, `try:`, `except`,
  assignments) as-is.

**Tests** (`mollify-core`): the Birefringence sentence and
`# for each row we compute the mean.` → not flagged; `# import os`,
`# from a import b`, `# return x + 1` → still flagged. Extend the existing
`flags_code_not_prose_or_directives` test (`commented.rs:98`).

**Risk:** low.

---

## P3.2 — Score weighting & first-run UX

Two parts.

### (a) Fold confidence into the headline score
**Confirmed gap.** `quality_score()` subtracts a flat penalty per finding by
severity only, ignoring confidence (`crates/mollify-core/src/lib.rs:493-508`).
With ~95% of findings being `Uncertain`/`Likely` false positives, a clean repo
reads 20/100.

**Fix.** Weight the per-finding penalty by confidence (e.g. `Certain` ×1.0,
`Likely` ×0.5, `Uncertain` ×0.15), or exclude `Uncertain` from the headline and
report it separately. Note: once P1/P2 land the FP volume collapses, so this is
secondary — but it makes the score robust to residual noise. Keep it
deterministic and document the weights next to the function.

### (b) Ship / auto-generate a starter `.mollifyrc.json`
**Confirmed gap.** `config.rs` supports an `ignore` list and per-rule/category
severity overrides but there's no scaffolding command and no default that quiets
test-only rules (`config.rs:1`, defaults at `:69`). A first-time user sees the
raw 20/100.

**Fix.** Add a `mollify init` subcommand (clap, in `mollify-cli`) that writes a
commented starter `.mollifyrc.json` — e.g. set `unused-export`/typing rules to
`off`/`ignore` for `tests/`, document the confidence tiers. Do **not** silently
change defaults (determinism + explicitness); make it an opt-in generated file.

**Tests:** config round-trips the generated file; `init` is idempotent / won't
clobber an existing rc without `--force`.

**Risk:** low; (b) is additive.

---

## What already works — protect with regression tests (don't regress)

Per the audit's "works well" section: complexity ranking, `unused-parameter`,
`duplication`, `private-import`, `missing-dependency: sqlalchemy` (true
positive — keep it firing), confidence tiers, stable IDs, CWE tags,
churn×complexity hotspots, SARIF/MCP outputs. The P1.2 and P2.2(c) fixes are
tightenings — add explicit tests that the *true* positives in those same areas
still fire (bare `eval`, a genuinely missing third-party dist).

---

## Suggested sequencing

1. **P1.1** (import resolver) — unblocks the score; do first and re-measure.
2. **P1.2** + **P2.1** + **P3.1** — tiny, high-trust tightenings; batch together.
3. **P1.3** (pytest roots) + shared `is_test_module`/testpaths plumbing.
4. **P2.2** (nested imports, entry points, first-party leaves) — largest; land
   behind its own tests.
5. **P3.2** (score weighting + `mollify init`) — UX polish, last.

Each step: `cargo build && cargo test && cargo clippy --all-targets`, update
`docs/STATUS.md`. After P1/P2, re-run `mollify audit` on Birefringence to confirm
the score and finding count move as predicted (target ~70%+ actionable).

Worth an ADR in `docs/adr/` for the package-aware relative-import resolution
(P1.1) and the nested-import collection model (P2.2a), since both change core
graph semantics.
