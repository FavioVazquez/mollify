# Changelog

All notable changes to Mollify. This project follows the spirit of
[Keep a Changelog](https://keepachangelog.com/) and the JSON contract is
versioned by `schema_version` (currently `0.1`).

## Unreleased

### Fixed
Precision pass from a real-world audit (running `mollify` on the *Birefringence*
package surfaced these false positives; see `docs/birefringenceaudit.md` and
`docs/birefringence-audit-plan.md`):
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
