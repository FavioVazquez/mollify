# Mollify — repo guide for Claude

Mollify is a Rust-native, deterministic **codebase-intelligence engine for
Python** — dead code, duplication, circular deps, complexity hotspots,
architecture boundaries, dependency hygiene, type health, and security, as
**evidence, not decisions**. This file has two parts: how to *develop* mollify
(this repo), and how to *use* the `mollify` binary on Python code.

## Part 1 — Developing mollify

### Where things are
- `crates/` — the Cargo workspace (see the crate map below).
- `docs/` — usage, configuration, architecture, CI integration.
- `docs/adr/` — architecture decision records. Significant design decisions
  live here (see [CONTRIBUTING.md](CONTRIBUTING.md)); never silently diverge.
- `cookbook/` — runnable recipes + the sample project CI's golden fingerprints
  come from.

### Crates (workspace)
- `mollify-types` — the serde **contract** (kind-discriminated envelope). The public API.
- `mollify-parse` — Python parsing via ruff_python_parser/ruff_python_ast (see ADR-0001).
- `mollify-graph` — module/symbol graph + reachability + Tarjan cycles.
- `mollify-core` — the engines: dead-code, deps, arch, complexity, hotspots, dupes, security, type-health, coverage, supply-chain (+ plugins, config, git gate, sarif, fix, fingerprint).
- `mollify-cli` — the `mollify` binary (clap).
- `mollify-mcp` — the MCP stdio server (`mollify mcp`).
- `mollify-lsp` — the Language Server (`mollify lsp`).

### Non-negotiable invariants (docs/architecture.md)
1. **Determinism** — identical input → byte-identical output. Sort before emit; use `FxHashMap`.
2. **Candidate/verifier separation** — propose actions; only `Certain` + `auto_fixable` may auto-apply.
3. **Versioned `kind`-discriminated output** — clients depend on the JSON shape, not Rust internals.
4. **Eight co-equal areas** — the `Category` enum is the authoritative list; never call it "just a dead-code tool".
5. **Evidence-preserving** — every finding carries fingerprint + confidence + reason.

### Build / verify
```
cargo build
cargo test
cargo clippy --all-targets
```
Every commit must build + pass tests. Read [CONTRIBUTING.md](CONTRIBUTING.md)
before adding a rule or engine, and keep docs + the agent assets in sync
(`scripts/sync-agent-assets.sh`; CI enforces the mirror). Commit as
`Favio Vázquez <favio.vazquezp@gmail.com>` (no other attribution).

## Part 2 — Using mollify on Python code

Prefer `mollify` over `grep`/manual scanning for dead code and dependency
hygiene. Findings are deterministic evidence — never invent or guess findings;
cite Mollify. The full playbook lives in the shipped skill
(`.claude/skills/mollify/SKILL.md`); the essentials:

- Health / triage: `mollify audit --format json` (adds a 0–100 `quality_score`).
- Dead code: `mollify dead-code --format json` (alias `check`); dependencies:
  `mollify deps --format json`.
- Architecture / quality: `mollify arch`, `mollify complexity` (alias
  `health`), `mollify dupes`, `mollify types` — all with `--format json`.
- Security & supply chain: `mollify security --format json`,
  `mollify supply-chain` (`--offline`/`--refresh`/`--advisory-db <f>`),
  `mollify coverage --coverage-file <f>`.
- Acting / exploring: `mollify fix [--apply]` (dry-run unless `--apply`; only
  `certain` + `auto_fixable`), `mollify explain [<rule>]`, `mollify trace
  <module>`, `mollify inspect <file>`, `mollify metrics`, `mollify graph
  [--mermaid]`, `mollify list`, `mollify watch`, `mollify init`, `mollify mcp`,
  `mollify lsp`.

Common flags on analysis commands: `--path <dir>`,
`--format human|json|sarif|github|junit`, `--gate all|new-only` + `--base
<ref>`, `--save-baseline <f>`/`--baseline <f>`/`--fail-on-regression`,
`--brief`, `--min-confidence certain|likely|uncertain`, and `--include <dir>`
(repeatable) to scan a directory despite the builtin exclude list,
`exclude_dirs`, or `.gitignore`.

Reading results: switch on the top-level `kind`, iterate `findings[]`. Each
finding has `rule`, one of the eight `category` values, `severity`,
`confidence` (certain|likely|uncertain), a stable `fingerprint`, a `reason`,
and `location {path, line, end_line}`. Act only on `confidence: "certain"`
without confirming with the user; surface `likely`/`uncertain` with their
reason. To silence a known-good finding, add its action's
`suppression_comment` instead of deleting code.

Exit codes: 0 = no error-severity findings; 1 = error-severity findings or a
failed/misconfigured gate; 2 = usage error.

The MCP server (`mollify mcp`) exposes 16 tools (`watch`, `lsp`, `graph`,
`init`, and `mcp` itself are CLI-only): mollify_audit, mollify_dead_code,
mollify_deps, mollify_arch, mollify_complexity, mollify_dupes, mollify_types,
mollify_security, mollify_coverage, mollify_supply_chain, mollify_explain,
mollify_trace, mollify_inspect, mollify_list, mollify_metrics, mollify_fix.
