# Mollify — repo guide for agents

Mollify is a Rust-native, deterministic **codebase-intelligence engine for
Python** — fallow's model (dead code, duplication, circular deps, complexity
hotspots, architecture boundaries, dependency hygiene) ported to Python and
extended. It emits **evidence, not decisions**.

## Where things are
Local working docs (gitignored — not published):
- `PLAN.md` — the build plan (capability matrix, architecture, phased roadmap, orchestration).
- `RESEARCH.md` — the landscape + fallow internals + Python-tool currency pass (§8).
- `INTEGRATIONS.md` — agent integration notes (Claude Code, Codex, Cursor, Gemini, **Devin/Cascade**).
- `docs/STATUS.md` — **the running build log. Read it first; update it every session.**

Published:
- `docs/adr/` — architecture decision records (significant decisions live here).
- `docs/` — usage, configuration, architecture, CI integration.
- `crates/` — the Cargo workspace.

## Crates (workspace)
- `mollify-types` — the serde **contract** (kind-discriminated envelope). The public API.
- `mollify-parse` — Python parsing (tree-sitter today, see ADR-0001).
- `mollify-graph` — module/symbol graph + reachability + Tarjan cycles.
- `mollify-core` — the engines: dead-code, deps, arch, complexity, hotspots, dupes, security, type-health, coverage, supply-chain (+ plugins, config, git gate, sarif, fix, fingerprint).
- `mollify-cli` — the `mollify` binary (clap).
- `mollify-mcp` — the MCP stdio server (`mollify mcp`).
- `mollify-lsp` — the Language Server (`mollify lsp`).

## Non-negotiable invariants (RESEARCH.md §2.11)
1. **Determinism** — identical input → byte-identical output. Sort before emit; use `FxHashMap`.
2. **Candidate/verifier separation** — propose actions; only `Certain` + `auto_fixable` may auto-apply.
3. **Versioned `kind`-discriminated output** — clients depend on the JSON shape, not Rust internals.
4. **Five co-equal areas** — never call it "just a dead-code tool".
5. **Evidence-preserving** — every finding carries fingerprint + confidence + reason.

## Build / verify
```
cargo build
cargo test
cargo clippy --all-targets
```
Every commit must build + pass tests. Update `docs/STATUS.md`. Commit as
`Favio Vázquez <favio.vazquezp@gmail.com>` (no other attribution).
