# Mollify — Build Status / Log

This is the running build log. **Update it every working session** so context loss
never erases progress. It records: per-phase status, what compiles/tests, and
every deviation from `PLAN.md` (with rationale).

Legend: ✅ done & tested · 🟡 in progress · ⬜ not started · 🔵 scaffolded (compiles, stubbed)

## Environment constraints discovered (these shape the build)
- cargo 1.94.1, rustc 1.94.1, `cc`/`gcc` present. crates.io fetch works.
- **Git dependencies from GitHub are BLOCKED** (cargo → HTTP 403 via egress proxy).
  → The plan's `ruff_python_parser` (git-rev) path is not buildable here.
  See **ADR-0001**: we build on `tree-sitter-python` (crates.io) behind a parser
  abstraction; ruff crates remain the future migration.

## Deviations from PLAN.md (documented as required)
| # | Plan said | What we did | Why | Where |
|---|-----------|-------------|-----|-------|
| D1 | Parser = ruff_python_parser via pinned git rev (§3.2) | tree-sitter-python via crates.io, behind `mollify-parse` abstraction | GitHub git deps blocked by egress (403) | ADR-0001 |
| D2 | Crate names `config/types/parse/...` (§3.1) | `mollify-*` prefix (e.g. `mollify-types`) | avoid crates.io name clashes; clearer | this file |

## Phase status
- **Phase 0 — Skeleton + parser POC:** ✅
  - ✅ workspace, toolchain, gitignore
  - ✅ `mollify-types` (4 tests), `mollify-parse` (6 tests), `mollify-graph` (4 tests)
- **Phase 1 — MVP dead-code + deps:** ✅ (core complete, tested, dogfooded)
  - ✅ `mollify-graph` — discovery (ignore walker), path-sorted FileIds, dotted-name
    resolution (incl. src-layout + relative imports), import edges, BFS reachability
    from entry points, symbol-usage queries (internal count + cross-module + `from x import`).
  - ✅ `mollify-core` — dead-code engine (unused-file, unused-export) with confidence
    tiers (certain/likely/uncertain) + `__all__`/dunder suppression + dynamic-sink
    downgrade; deps engine (unused/missing dependency) parsing pyproject (PEP 621 +
    Poetry + PEP 735) with stdlib set + import→dist alias table; deterministic
    fingerprints (`<rule>:<xxh3>`); quality score; kind-discriminated reports. 9 tests.
  - ✅ `mollify-cli` — `mollify` binary: `audit`/`dead-code`(alias `check`)/`deps`/`init`,
    `--format human|json`, `--path`, CI exit code on errors. Dogfooded on a sample
    project (correct results: private→certain+autofix, public→likely, orphan file,
    missing numpy, unused rich/leftover-pkg; requests+stdlib+cross-module not flagged).
  - **Total: 23 tests green; `cargo build`, `cargo test`, `cargo clippy` clean.**
  - ⏳ Phase-1 polish still open: `--gate new-only` (git diff + base worktree + attribution),
    SARIF output, framework entry-point plugins (Django/FastAPI/pytest decorators),
    config file (`.mollifyrc`) actually read, `fix` command.
- **Phase 2 — dupes + complexity + arch:** ⬜ (scaffold next)
- **Phase 3 — AI/MCP + plugins:** ⬜
- **Phase 4 — runtime/type intelligence:** ⬜
- **Agent integrations** (`.devin/` skills+rules+hooks, `.windsurf/` workflows): ⬜
  (user confirmed: `.devin` = hooks/skills/rules, `.windsurf` = workflows)

## Verification protocol (every commit)
1. `cargo build` clean. 2. `cargo test` green. 3. `cargo clippy` (best-effort). 4. Update this file. 5. Commit with a descriptive message (author: Favio Vázquez).

## Invariants we must not break (from RESEARCH.md §2.11)
Determinism · candidate-producer/verifier separation · versioned `kind`-discriminated
output · five co-equal analysis areas · evidence-preserving findings.
