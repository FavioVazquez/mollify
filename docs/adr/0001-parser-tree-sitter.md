# ADR-0001: Parser foundation — tree-sitter-python now, ruff_python_parser later

- **Status:** Accepted (2026-06-24)
- **Context:** `PLAN.md` §3.2 specifies building on Astral's `ruff_python_parser`
  / `ruff_python_ast` crates, consumed via a pinned git revision (pyrefly's
  pattern), because they are the de-facto Rust Python foundation.
- **Problem:** In the current build environment, **cargo cannot fetch git
  dependencies from GitHub** — `cargo fetch` of a `git = "https://github.com/astral-sh/ruff"`
  dependency fails with HTTP 403 (the egress proxy blocks github.com git access;
  only crates.io and a few hosts are allowlisted). The ruff crates are **not
  published to crates.io**, so there is no buildable path to them here.
- **Decision:** Build on **`tree-sitter-python`** (crates.io, compiles cleanly
  with the available `cc`), wrapped behind the `mollify-parse` crate's
  parser-agnostic types (`ParsedModule`, `Definition`, `Import`). This is the
  same foundation skylos and Bury use for Python reachability.
- **Consequences:**
  - ✅ The project builds and tests in this environment today.
  - ✅ The rest of the engine depends only on `mollify-parse`'s types, so the
    parser swap is localized to one crate.
  - ⚠️ tree-sitter is a lossy/untyped CST vs ruff's typed AST; some precision
    (full scope/binding resolution) is more work than it would be on ruff.
  - 🔭 **Migration path:** when git access or a vendored copy of the ruff crates
    is available, re-implement `mollify-parse` against `ruff_python_parser` +
    `ruff_python_ast` (+ build resolution like pyrefly, per RESEARCH.md §8.6),
    keeping the public types stable. Track under a future ADR.
- **Re-verify:** if the egress policy changes to allow github.com git, revisit.
