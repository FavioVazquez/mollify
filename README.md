# Mollify

**Deterministic codebase intelligence for Python.** A Rust-native engine that
gives humans and AI agents structured, inspectable repo truth — dead code,
dependency hygiene (and, in progress, duplication, complexity hotspots,
circular dependencies, and architecture boundaries) — as *evidence, not
guesses*. It's [fallow](https://github.com/fallow-rs/fallow)'s model, ported to
Python and extended.

> Status: **early, working MVP.** Phase 0–1 are implemented, tested, and
> dogfooded. See [`docs/STATUS.md`](docs/STATUS.md) for exactly what's done vs
> pending, and [`PLAN.md`](PLAN.md) / [`RESEARCH.md`](RESEARCH.md) for the full
> design and the competitive landscape.

## What works today
- **Dead code** — reachability-based unused files and top-level
  functions/classes/variables, with `certain` / `likely` / `uncertain`
  confidence tiers (`__all__`, dunder, and dynamic-import aware).
- **Dependency hygiene** — unused and missing distributions from
  `pyproject.toml` (PEP 621 + Poetry + PEP 735), with stdlib + import→distribution
  alias handling.
- **Deterministic JSON contract** — a `kind`-discriminated envelope with stable
  fingerprints, designed for CI and coding agents.

## Install & run (from source)
```bash
cargo build --release
./target/release/mollify audit --path /path/to/your/python/project
./target/release/mollify dead-code --format json
./target/release/mollify deps
```

## Agent integration
Mollify ships first-class integration for coding agents (see
[`INTEGRATIONS.md`](INTEGRATIONS.md)). Notably **Devin Desktop / Cascade**:
- `.devin/skills/mollify/SKILL.md` — the skill the agent invokes.
- `.devin/rules/mollify.md` — glob-triggered guidance for Python files.
- `.devin/hooks.v1.json` (Devin/Claude-compatible) and `.windsurf/hooks.json`
  (Cascade) — advisory audit hooks.
- `.windsurf/workflows/mollify-audit.md`, `mollify-cleanup.md` — `/slash` workflows.

## Layout
`crates/mollify-types` (JSON contract) · `mollify-parse` (Python parsing) ·
`mollify-graph` (module/symbol graph + reachability) · `mollify-core` (engines) ·
`mollify-cli` (the `mollify` binary).

MIT licensed.
