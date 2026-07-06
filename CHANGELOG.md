# Changelog

All notable changes to Mollify. This project follows the spirit of
[Keep a Changelog](https://keepachangelog.com/) and the JSON contract is
versioned by `schema_version` (currently `0.1`).

## Unreleased

Calibration and portability fixes from the first real-world corpus evaluation
(all engines run against pinned checkouts of requests, flask, rich,
MediaCrawler, and MoneyPrinterTurbo; fingerprints are unaffected — baselines
survive, though report *bytes* change where paths/confidences did).

### Added
- **Engine panic isolation.** Every report runs each engine under
  `catch_unwind`; a panicking engine degrades to a single `engine-panic`
  finding (severity `error`) instead of killing the whole report. Motivated
  by the dupes OOM taking `audit` down with it on three corpus repos.
- **Windows path hardening.** The test/dev/fixture path heuristics normalize
  `\` separators before matching, so they classify correctly on Windows
  paths instead of silently never matching.
- **Chaos + fuzz test suites.** A generated hostile-input corpus (deep
  nesting, NUL bytes, latin-1, BOM/CRLF, unterminated strings, symlink
  loops, unicode identifiers) that every engine must survive, plus
  deterministic xorshift fuzz tests over the hand-written tokenizer,
  string consumer, and comment parsers.

### Fixed
- **The dupes engine no longer OOMs on non-ASCII identifiers.** Its tokenizer
  walked bytes and re-interpreted UTF-8 lead bytes as chars; a Unicode
  identifier (`c.ß` — legal Python 3) made the identifier scanner consume
  zero bytes and push empty tokens until the OOM killer fired (~16 GB).
  Found live: attrs', black's, and django's unicode tests killed `dupes`
  and `audit`. The tokenizer is now UTF-8-aware and keeps Unicode
  identifiers intact.
- **Nothing inside an unreachable module or a fixture/data tree is
  `certain`/auto-fixable anymore.** A `.py` that nothing imports is often
  tool fixture data — black's formatter test cases and pydantic's mypy
  golden inputs were full of technically-correct `certain` unused-imports
  that `fix --apply` would have "fixed", corrupting both projects' test
  suites. Unreachable modules and recognized fixture trees
  (`data/`, `fixtures/`, `testdata/`, `golden/`, `snapshots/` path segments
  — sample code with a `__main__` guard reads as an entry point, so
  reachability alone is not enough) now cap `unused-import`/`unused-export`
  at `likely`; the file-level `unused-file` finding remains the actionable
  evidence.
- **Names in quoted `TypeAlias` values count as uses.**
  `_P: TypeAlias = 'partial[Any] | partialmethod[Any]'` (pydantic) no longer
  yields a certain + auto-fixable unused-import for `partial`/`partialmethod`
  — type checkers (and pydantic itself) evaluate that string.
- **`unused-import` no longer grades deliberate imports `certain` +
  auto-fixable** — `mollify fix --apply` could previously delete them (found
  live on flask). Now: redundant-alias re-exports (`import x as x`,
  `from m import y as y` — PEP 484) and names another module imports *from*
  the flagging module (compat/shim re-exports) are treated as used; imports
  inside `try`/`except` (availability probes) and `__init__.py` re-exports cap
  at `uncertain` and are never auto-fixed.
- flake8-style **`# noqa` comments are honored** for the rules they map to:
  blanket `# noqa` / `# noqa: F401` silences `unused-import` on that line,
  `# noqa: F841` silences `unused-variable`. Other codes are not interpreted.
- **`location.path` (and action descriptions) are now root-relative** in every
  report: output no longer varies with how `--path` was spelled, absolute
  machine-specific paths no longer leak into JSON/SARIF, and `.mollifyrc`
  `ignore` patterns match the same strings on every machine.

### Changed
- **Security candidates in test/docs/example trees are capped at `uncertain`**
  confidence and tagged in the reason. On the corpus, non-production code
  dominated security output (116 of requests' 130 findings were
  `request-without-timeout`, mostly in its own test suite); the candidates
  remain in the report but no longer survive `--min-confidence likely`.

## 0.1.4 - 2026-07-02

Fix release from a full-repository code review (`docs/code-review-2026-07-01.md`):
every crate was read end-to-end, and every fix below was verified against a
reproduction before landing. **Contains breaking changes** — fingerprints and
several CLI exit codes changed; regenerate any saved baselines after upgrading
(see *Breaking* below).

### Breaking
- **Fingerprints changed wholesale — regenerate baselines** (`--save-baseline`).
  Fingerprints now hash the **root-relative** path (they no longer vary with
  `--path` spelling or checkout location, so CI baselines finally transfer to
  laptops), drop line numbers in favor of symbol/content identity plus an
  occurrence index (edits above a finding no longer churn it), and use the
  full 64-bit hash (16 hex chars).
- CLI exit codes are stricter where CI trust demanded it: a failed
  `--save-baseline` write exits 1 (was: success message + 0); a missing or
  invalid `--baseline` with `--fail-on-regression` exits 1 (was: gate silently
  disabled); a nonexistent `--path` exits 2 (was: clean 100/100 report);
  `trace`/`inspect`/`list`/`metrics` reject formats they don't implement with
  exit 2 (was: silent human output); `inspect` with no matching file exits 1.
- `unresolved-import` is now `likely`, not `certain`: a relative import may
  resolve to a C extension or build-generated module the `.py` walk can't see.

### Fixed
- **`fix --apply` corrupted Jupyter notebooks**: finding lines are relative to
  the concatenated code cells, so notebook findings are no longer
  auto-fixable (and the fix planner refuses non-`.py` files outright).
- **`fix --apply` could delete live, dynamically-dispatched code**: a symbol
  invoked from *another* module via `getattr(lib, "_handler_" + name)()` was
  graded `certain`; a dynamic sink anywhere in the project now caps
  unused-export confidence, mirroring `unused-file`.
- `fix --apply` preserves CRLF line endings and no longer aborts the whole run
  (losing the applied count) when one file fails I/O.
- Dead-code false positives from resolver gaps: module constants used only in
  function signatures (parameter defaults/annotations, return annotations,
  lambda defaults); imports inside module-level `with`/`for`/`while`/`match`
  suites; symbols used only via lazy in-function imports; sibling modules of a
  root-level `__init__.py`; names shadowed by Python-3-scoped comprehension
  targets; `__all__ += […]` / `.extend(…)` extensions.
- Runtime imports in the `else` branch of an `if TYPE_CHECKING:` guard are no
  longer treated as type-only (and `if not TYPE_CHECKING:` now works);
  TYPE_CHECKING guard detection is exact instead of substring-based.
- Decorated `def`/`class` line numbers point at the definition, not the first
  decorator.
- `cold-code` can now actually fire for imported modules: the `def` line —
  executed at import time — no longer counts as evidence the body ran.
- The duplication engine reads notebook **code cells**, not raw `.ipynb` JSON
  (near-identical scaffolding produced bogus clone families).
- Dependency hygiene: dev-group tools (black, mypy, pre-commit…) are exempt
  from `unused-dependency` (deptry parity); `psycopg2`/`psycopg2-binary` and
  friends are alternative providers instead of a forced alias (no more paired
  unused+missing false positives); namespace tops (`google`, `azure`, …) are
  never guessed as `missing-dependency` without an installed env; URL/VCS
  requirement lines no longer produce mangled names (`#egg=` respected, pip
  comment rules honored).
- PEP 440: epochs compare correctly (`2!1.0` no longer parses as `2`), and
  `.postN`/`.devN` order per spec instead of comparing equal to the release.
  `specs_intersect` finds narrow gaps like (`>2.0`, `<2.0.1`).
- Determinism: import→dist mapping and requirements/pins collection no longer
  depend on filesystem `read_dir` order; hotspot/coverage/git fallback
  matching is anchored at path-separator boundaries (`app.py` no longer claims
  `myapp.py`'s churn, coverage, or diff hunks) with deterministic tie-breaks;
  non-ASCII filenames survive git's `core.quotePath`.
- `inspect <file>` matches path fragments at separator boundaries only
  (inspecting `b.py` no longer returns `lib.py`'s findings).
- Unconstrained declared deps (bare `flask`) are matched against advisories
  again instead of being silently skipped.
- MCP protocol: version negotiation no longer echoes arbitrary client
  versions; malformed JSON gets a `-32700` response; requests without a
  `method` get `-32600`; `mollify_fix` apply errors surface as tool errors.
- LSP: unknown requests get `-32601` instead of hanging the client;
  `didClose` clears stale diagnostics; diagnostic ranges are never reversed
  and cover the final line; a malformed `Content-Length` header no longer
  kills the server mid-session.
- OSV `querybatch` pagination: truncated advisory sets are no longer cached
  as authoritative.
- `--save-baseline` keeps stdout pure JSON (status note moved to stderr, the
  report is still emitted); `quality_score` is recomputed after
  `--gate`/`--min-confidence`/`--baseline` filtering so the envelope is
  internally consistent.
- Update-check cache writes atomically (temp file + rename).

### Changed
- `mollify-types` contract enums are `#[non_exhaustive]` (adding a report
  kind/category is no longer a breaking Rust change), and the load-bearing
  `Confidence`/`Severity` orderings are documented and locked by tests.
- Docs/CI hygiene: `AGENTS.md` no longer documents a nonexistent
  `mollify graph --format` flag; the GitLab CI example uses
  `artifacts:reports:sarif`; the advisory-database docs describe the actual
  live-by-default behavior; `bump-version.sh` works on BSD/macOS sed;
  Dependabot covers GitHub Actions and Cargo; the report hook can no longer
  fail the action on malformed JSON.

## 0.1.3 - 2026-07-01

Precision release: a real-world audit surfaced a cluster of false positives whose
root causes are fixed here (on a fixture reproducing the audited patterns, the
score moved from 20/100 to 95/100). Two new ADRs document the core graph-semantic
changes ([ADR-0002](docs/adr/0002-package-aware-relative-import-resolution.md),
[ADR-0003](docs/adr/0003-nested-import-model.md)).

### Fixed
Precision pass from a real-world audit (running `mollify` on an external Python
package) that surfaced these false positives:
- **Relative imports in a package `__init__.py` now resolve.** A package's
  `__init__.py` has the package itself as its dotted name, so the resolver was
  dropping one segment too many — `.aa` resolved to `aa` instead of `pkg.aa`.
  This cascaded into spurious `unresolved-import` → `unused-file` →
  `unused-export` across re-exporting packages (the dominant FP source). Package
  self-references no longer create a `circular-dependency`.
- **`session.exec(...)` no longer flagged `dangerous-eval` (CWE-95).** The
  security rule matched any trailing `.exec`/`.eval` segment; it now matches only
  the `eval`/`exec`/`compile` builtins, not ORM/driver methods.
- **pytest `test_*`/`Test*` are no longer `unused-export`.** They are treated as
  reachability roots within test paths, honoring
  `[tool.pytest.ini_options].testpaths`.
- **`from __future__ import …` is no longer `unused-import`** (it has a compiler
  effect and is never unused).
- **Lazy/in-function imports** now count toward dependency usage and
  reachability, so a dependency imported only inside `main()` (e.g. `uvicorn`)
  isn't falsely `unused-dependency`; module-scope `unused-import` is unaffected.
- **`[project.scripts]` entry points are reachability roots** — the target
  module isn't `unused-file` and the named function isn't `unused-export`.
- **First-party test helpers** imported by bare leaf name (`conftest`, sibling
  modules on a test path) are no longer `missing-dependency`.
- **`commented-code` no longer fires on prose** that opens with a keyword
  (e.g. `# from zero (...), doubled.`); `from …` now requires a real `import`.

### Changed
- **Quality score is weighted by confidence** — `uncertain` findings penalize
  the 0–100 score far less than `certain` ones, so a report dominated by
  low-confidence review items no longer reads as a failing grade. Still
  deterministic.
- **`mollify init` writes a richer, documented starter `.mollifyrc.json`**
  (five-area severities, `type-health` off by default, complexity knobs,
  inline `_comment` docs).

### Added
- **`--include <DIR>`** flag on all 8 analysis commands (`audit`, `dead-code`,
  `deps`, `arch`, `complexity`, `dupes`, `types`, `security`; not
  `coverage`/`supply-chain`, which aren't path-scoped). Repeatable; overrides
  the builtin discovery denylist (`.venv`, `.git`, `__pycache__`,
  `node_modules`, `build`, `dist`, etc.), `.mollifyrc.json`'s `exclude_dirs`,
  and `.gitignore` for the named directory, letting users opt a directory
  back into scanning on a per-invocation basis. Does not override the
  `pyvenv.cfg` virtualenv guard — an included directory that is itself a
  virtualenv stays excluded.

## 0.1.2 - 2026-06-26

### Fixed
- **PyPI sdist upload failed with a 400** (`License-File LICENSE does not
  exist in distribution file`). `[tool.maturin] manifest-path` points at the
  CLI crate, not the workspace root where `LICENSE` lives, so maturin wrote a
  `License-File: LICENSE` metadata pointer into the sdist without actually
  including the file — reproduced across maturin 1.7.8–1.14.1. Fixed with
  `[tool.maturin] include = ["LICENSE"]`. (0.1.1's wheels published fine and
  are unaffected; only its sdist is missing from PyPI.)
- **Discovery no longer descends into virtualenvs, VCS metadata, or build/cache
  directories by default.** Previously `discover_python_files` walked every
  directory in a project, so an un-gitignored `.venv`/`venv` (or `.git`,
  `__pycache__`, `node_modules`, `build`, `dist`, etc.) had its contents parsed
  and flagged like first-party source — every installed package became a
  potential source of findings. Discovery now always prunes a builtin denylist
  (mirroring `ruff`'s defaults) plus any directory directly containing a
  `pyvenv.cfg` (catches custom-named virtualenvs). New `.mollifyrc.json`
  `exclude_dirs` extends this list for project-specific cases.

## 0.1.1 - 2026-06-26

Packaging/metadata release — no analysis or CLI behavior changes.

### Fixed
- **PyPI project page:** README images used repo-relative paths that don't
  resolve on PyPI; rewritten to absolute `raw.githubusercontent.com` URLs so they
  render on both GitHub and PyPI.
- **Python version badge** rendered as "missing" (no per-version trove
  classifiers); replaced with a static `Python 3.8+` badge.

### Changed
- **PyPI publishing** now uses PyPA's `pypa/gh-action-pypi-publish` instead of the
  deprecated `maturin upload` (PyO3/maturin#2334); still token-less OIDC Trusted
  Publishing.
- Added `Programming Language :: Python :: 3.8`–`3.13` classifiers so PyPI
  advertises supported versions.

## 0.1.0 - 2026-06-26

First public release. Distributed via PyPI (`uvx`/`pip install mollify`) and
crates.io (`cargo install mollify-cli`) — every channel ships the same
self-contained binary with agent integrations embedded.

### Engines & rules
- **Dead code:** `unused-file`, `unused-export`, `unused-import` (whole-statement
  and partial-name), `unused-variable` (F841), `unused-parameter`,
  `commented-code`, plus runtime cold-path (`cold-code`).
- **Dependency hygiene:** `unused-dependency`, `missing-dependency`, and
  `transitive-dependency` (venv `*.dist-info`-aware import→distribution mapping).
- **Architecture:** `circular-dependency`, `layer-violation` (presets),
  declarative contracts (`forbidden-import`, `independence-violation`), and
  rule-pack policies (`forbid_import`/`forbid_call`).
- **Complexity & cohesion:** `high-complexity`, churn×complexity `hotspot`,
  `low-cohesion` (LCOM*), and a `mollify metrics` report (Maintainability Index,
  Halstead, raw LOC).
- **Duplication:** token clone families with configurable thresholds.
- **Type health:** `untyped-function`.
- **Security:** eval/exec, shell, `sql-injection`, weak hash/cipher,
  insecure-random, unsafe deserialization, TLS, secrets, missing-timeout — each
  with a CWE id.
- **Supply chain:** `vulnerable-dependency` — live OSV (`/v1/querybatch`) by
  default with an offline advisory-DB fallback.

### Surfaces
- 21 CLI commands (incl. `metrics`, `graph`, `inspect`, `list`, `trace`,
  `explain`, `watch`, `fix`, `supply-chain`).
- Output formats: human, JSON (kind-discriminated), SARIF, GitHub annotations,
  JUnit XML.
- Gating: `--gate new-only` with **line-level** introduced-vs-inherited
  attribution; regression baselines (`--save-baseline`/`--baseline`/
  `--fail-on-regression`); `--brief`; `--min-confidence`.
- **MCP server** (`mollify mcp`) — 16 tools.
- **Language Server** (`mollify lsp`) — diagnostics on open/save plus live
  file-local diagnostics on edit.
- Inline `# mollify: ignore[<rule>]` suppressions; `.mollifyrc.json` config
  (severity, ignore, complexity/duplication thresholds, architecture layers &
  presets, contracts, policies).
- Agent integrations for Claude Code, OpenAI Codex, Cursor, Gemini CLI, and
  Devin/Cascade.

### Invariants
- Deterministic: identical input → byte-identical output.
- Evidence, not decisions: every finding carries a fingerprint, confidence tier,
  and human reason; only `certain` + `auto_fixable` findings are ever auto-fixed.
