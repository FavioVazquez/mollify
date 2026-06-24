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
- **Phase 0 — Skeleton + parser POC:** 🟡
  - ✅ workspace (`Cargo.toml`, toolchain, gitignore)
  - ✅ `mollify-types` — kind-discriminated envelope, Confidence/Severity/Category, Finding, Summary, deterministic sort. 4 tests green.
  - ✅ `mollify-parse` — tree-sitter wrapper: defs (fn/class/var), imports (incl. relative/star/conditional), `__all__`, used-names, dynamic-sink detection. 6 tests green.
  - ⬜ `mollify-graph` (next)
  - ⬜ `mollify-cli` (next)
- **Phase 1 — MVP dead-code + deps:** ⬜
- **Phase 2 — dupes + complexity + arch:** ⬜
- **Phase 3 — AI/MCP + plugins:** ⬜
- **Phase 4 — runtime/type intelligence:** ⬜
- **Agent integrations** (`.devin/` skills+rules+hooks, `.windsurf/` workflows): ⬜ (after CLI surface is real)

## Verification protocol (every commit)
1. `cargo build` clean. 2. `cargo test` green. 3. `cargo clippy` (best-effort). 4. Update this file. 5. Commit with a descriptive message (author: Favio Vázquez).

## Invariants we must not break (from RESEARCH.md §2.11)
Determinism · candidate-producer/verifier separation · versioned `kind`-discriminated
output · five co-equal analysis areas · evidence-preserving findings.
