# ADR-0001: Parser foundation — ruff_python_parser / ruff_python_ast

- **Status:** Accepted (2026-06-24); **revised 2026-06-26** to adopt the ruff
  parser (the original tree-sitter decision is superseded — see History).
- **Context:** `PLAN.md` §3.2 specifies building on Astral's `ruff_python_parser`
  / `ruff_python_ast` crates — the de-facto, full-fidelity Rust Python parser
  foundation (the same one that powers `ruff`).
- **Decision:** Build `mollify-parse` on **`ruff_python_parser` + `ruff_python_ast`**
  (with `ruff_source_file` for line indexing), pinned to an exact **crates.io**
  release (`=0.0.3`). The crate exposes parser-agnostic types (`ParsedModule`,
  `Definition`, `Import`, `FunctionComplexity`, …), so the concrete parser stays
  an implementation detail confined to one crate.
- **Consequences:**
  - ✅ Full-fidelity, error-resilient typed AST with `Load`/`Store` contexts —
    enabling real **scope/binding resolution** (LEGB) for precise dead-code, not
    coarse token counting.
  - ✅ Published on crates.io, so **every** distribution channel — crates.io
    publish, PyPI (maturin), and source — builds the identical binary. No
    git dependency, fully reproducible (pinned `=0.0.3`).
  - ✅ The rest of the engine depends only on `mollify-parse`'s types, so the
    backend swap was localized to one crate (all downstream tests passed
    unchanged).
- **History:** The first cut (2026-06-24) used `tree-sitter-python` because the
  build environment then appeared to block GitHub git dependencies (cargo HTTP
  403) and the ruff crates were thought to be git-only. Both premises proved
  **stale**: git deps work, and the ruff parser crates are now published to
  crates.io. `mollify-parse` was migrated to ruff on 2026-06-26 (public types
  preserved); tree-sitter was removed.
