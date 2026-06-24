<div align="center">

# Mollify

**Deterministic codebase intelligence for Python.**

*Dead code · duplication · circular dependencies · complexity & hotspots · architecture · dependency hygiene · type health · security — as evidence, not guesses.*

[Usage](docs/usage.md) · [Configuration](docs/configuration.md) · [Architecture](docs/architecture.md) · [CI integration](docs/ci-integration.md) · [Agent integrations](INTEGRATIONS.md) · [Build status](docs/STATUS.md)

</div>

---

Mollify is a Rust-native engine that gives humans **and AI agents** a structured,
inspectable map of a Python codebase. It's [fallow](https://github.com/fallow-rs/fallow)'s
model — one fast binary that unifies the whole "what's unused / risky / duplicated /
tangled" question — ported to Python and extended with Python-specific signals
(type health, notebooks, framework awareness) that fallow doesn't have.

Its one rule: **no AI invents findings.** Every result is a piece of deterministic
evidence with a stable fingerprint, a confidence tier, and a human-readable reason.
Mollify *produces candidates*; you (or your agent) decide what to do with them.

> **Project status:** early but real. Phases 0–4 of the [plan](PLAN.md) are
> substantially implemented, tested (68+ tests), and dogfooded; CI is green.
> See [`docs/STATUS.md`](docs/STATUS.md) for exactly what's done vs pending and
> [`docs/adr/`](docs/adr) for design decisions. Honest about its edges — see
> *Known limitations* below.

## Why Mollify

- **One tool, eight signals.** Most Python shops bolt together vulture + ruff +
  deptry + tach + radon + jscpd + bandit. Mollify runs the equivalent set in a
  single deterministic pass with one config and one output contract.
- **Built for coding agents.** A first-class MCP server plus shipped integrations
  for **Devin/Cascade, Claude Code, Codex, Cursor, and Gemini CLI** — so the
  agent reads repo *truth* instead of reconstructing it from `grep`.
- **Honest about uncertainty.** Python dead-code detection is undecidable in
  general, so every verdict is tiered `certain / likely / uncertain` and only
  `certain` findings are ever auto-fixed. Framework decorators (routes, tasks,
  fixtures, CLI commands, validators) are understood, killing the #1 false positive.
- **Deterministic & CI-ready.** Identical input → byte-identical output. SARIF,
  JSON, exit codes, and a PR-scoped `--gate new-only`.

## What it detects

| Area | Command | Rules |
|---|---|---|
| **Dead code** | `mollify dead-code` | `unused-file`, `unused-export`, `unused-import` |
| **Dependency hygiene** | `mollify deps` | `unused-dependency`, `missing-dependency` |
| **Architecture** | `mollify arch` | `circular-dependency`, `layer-violation`, custom policies |
| **Complexity & hotspots** | `mollify complexity` | `high-complexity`, `hotspot` (churn × complexity) |
| **Duplication** | `mollify dupes` | `duplication` (clone families) |
| **Type health** | `mollify types` | `untyped-function` |
| **Security** | `mollify security` | `dangerous-eval`, `subprocess-shell-true`, `unsafe-yaml-load`, `unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret` |
| **Cold paths** | `mollify coverage --coverage-file` | `cold-code` (reachable but never executed) |
| **Supply chain** | `mollify supply-chain` | `vulnerable-dependency` (pinned versions vs a local CVE/advisory DB) |
| **Everything + score** | `mollify audit` | all of the above + a 0–100 quality score |

Also: **Jupyter notebooks (`.ipynb`)** are discovered and analyzed cell-by-cell;
**framework awareness** (Flask/FastAPI/Django/Celery/pytest/click/Pydantic/…);
**architecture presets** (`layered`/`hexagonal`/`feature-sliced`/`bulletproof`) and
**declarative rule packs** (ban imports/calls per path); `mollify fix` to safely
remove `certain` unused symbols; `mollify explain <rule>` for rule semantics; and
`mollify trace <module>` for a module's import neighborhood; `mollify inspect
<file>` for a per-file evidence bundle; `mollify list` for project topology; and
**regression baselines** (`--save-baseline` / `--baseline --fail-on-regression`)
to gate CI on *new* issues without git.

## Install

```bash
git clone https://github.com/FavioVazquez/mollify
cd mollify
cargo build --release          # binary at ./target/release/mollify
```

(PyPI wheels via maturin and `cargo install` are planned — see the roadmap.)

## Quick start

```bash
mollify audit --path /your/python/project
```

```text
Mollify audit — /your/project
Quality score: 84/100
12 finding(s) across 47 file(s) — 0 error, 12 warn
  src/app.py:6   [warn/certain]  unused-export — function `_legacy` has no reachable references  (unused-export:931a82e6)
  src/api.py:88  [warn/likely]   high-complexity — function `handle` is complex (cyclomatic 14, cognitive 19)  (high-complexity:1aa9…)
  src/db.py:1    [warn/certain]  circular-dependency — import cycle: db → models → db  (circular-dependency:7c…)
  pyproject.toml:1 [warn/likely] unused-dependency — declared dependency `rich` is never imported  (unused-dependency:93…)
```

Machine-readable + CI:

```bash
mollify audit --format json                       # kind-discriminated contract
mollify audit --format sarif > mollify.sarif      # GitHub/GitLab code scanning
mollify audit --gate new-only --base origin/main  # only fail on regressions
mollify fix                                        # preview safe removals (--apply to write)
```

Supply-chain (offline, deterministic — refresh the advisory DB out-of-band):

```bash
python3 scripts/fetch-advisories.py .mollify/advisories.json   # pulls from OSV / safety-db
mollify supply-chain                                            # match pinned versions vs the DB
# (audit auto-includes supply-chain when .mollify/advisories.json exists)
```

## Confidence tiers

| Tier | Meaning | Auto-fixable |
|---|---|---|
| `certain` | Provable (e.g. a private unused symbol, no dynamic dispatch in scope) | ✅ |
| `likely` | Strong static signal, small residual dynamic risk | — |
| `uncertain` | Public surface, or near `getattr`/`eval`/`importlib` | — |

## The JSON contract

Every command emits a `kind`-discriminated envelope (`schema_version` pinned by
agent skills). Clients switch on `kind` and iterate `findings[]`:

```json
{
  "kind": "audit", "schema_version": "0.1", "quality_score": 84,
  "summary": { "total": 12, "errors": 0, "warnings": 12, "files_analyzed": 47 },
  "findings": [{
    "fingerprint": "unused-export:931a82e6", "rule": "unused-export",
    "category": "dead-code", "severity": "warn", "confidence": "certain",
    "reason": "function `_legacy` has no reachable references in the project",
    "location": { "path": "src/app.py", "line": 6, "end_line": 7 },
    "actions": [{ "type": "remove-symbol", "auto_fixable": true,
                  "suppression_comment": "# mollify: ignore[unused-export]" }]
  }]
}
```

## Configuration — `.mollifyrc.json`

```json
{
  "severity": { "dead-code": "error", "duplication": "warn", "unused-dependency": "off" },
  "ignore": ["tests/", "migrations/"],
  "max_cyclomatic": 10,
  "max_cognitive": 15,
  "architecture": { "layers": ["api", "service", "domain", "infra"] },
  "policies": [
    { "id": "no-requests-in-domain", "forbid_import": "requests", "in_paths": ["domain/"], "severity": "error" }
  ]
}
```

Raise rules/categories to `error` to make CI (and agent hooks) **block**. Full
reference: [docs/configuration.md](docs/configuration.md).

## Agent integrations

One MCP server (`mollify mcp`), many front-ends. Shipped, ready-to-commit artifacts:

| Agent | Artifacts |
|---|---|
| **Devin Desktop / Cascade** | `.devin/skills/mollify/`, `.devin/rules/mollify.md`, `.devin/hooks.v1.json` + `.windsurf/hooks.json`, `.windsurf/workflows/mollify-*.md` |
| **Claude Code** | `.mcp.json`, `.claude/skills/mollify/`, `.claude/commands/`, `.claude/settings.json` hooks |
| **Codex** | `AGENTS.md`, `.codex/config.toml`, `.agents/skills/mollify/` (portable) |
| **Cursor** | `.cursor/rules/mollify.mdc`, `.cursor/mcp.json`, `.cursor/commands/` |
| **Gemini CLI** | `GEMINI.md`, `.gemini/settings.json`, `.gemini/commands/mollify/` |

Details and copy-pasteable examples: [INTEGRATIONS.md](INTEGRATIONS.md).

## Architecture

A Cargo workspace; data flows parse → graph → engines → report:

`mollify-types` (JSON contract) · `mollify-parse` (Python parsing, tree-sitter) ·
`mollify-graph` (module/symbol graph + reachability + cycles) · `mollify-core`
(the engines) · `mollify-cli` (`mollify`) · `mollify-mcp` (MCP server).

See [docs/architecture.md](docs/architecture.md).

## How it compares

| | vulture | ruff | deptry | tach | radon | jscpd | bandit | **Mollify** |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| Whole-project dead code | ~ | – | – | – | – | – | – | ✅ (reachability + tiers) |
| Dependency hygiene | – | – | ✅ | – | – | – | – | ✅ |
| Circular deps | – | – | – | ✅ | – | – | – | ✅ |
| Complexity | – | ~ | – | – | ✅ | – | – | ✅ |
| Churn × complexity | – | – | – | – | – | – | – | ✅ |
| Duplication | – | – | – | – | – | ✅ | – | ✅ |
| Type health | – | – | – | – | – | – | – | ✅ |
| Security candidates | – | ~ | – | – | – | – | ✅ | ✅ |
| One deterministic pass + agent/MCP contract | – | – | – | – | – | – | – | ✅ |

`~` = partial. Mollify's wedge is the **unified deterministic pass** with one
contract — see [RESEARCH.md](RESEARCH.md) for the honest, sourced landscape
(including where individual tools already do a piece well).

## Known limitations (we're honest about these)

- Built on **tree-sitter**, not the ruff AST ([ADR-0001](docs/adr/0001-parser-tree-sitter.md)) —
  GitHub git-deps are blocked in the build env; ruff is the planned migration.
- Symbol usage is name-table-assisted, not full scope/binding resolution.
- Duplication is Rabin-Karp token matching (SA-IS+LCP is the planned upgrade).
- `--gate` attribution is file-level (line-level base-worktree is planned).
- Supply-chain matching needs **pinned/locked** versions (requirements `==`,
  poetry/uv lockfiles); unpinned ranges can't be matched precisely. The advisory
  DB is refreshed out-of-band (`scripts/fetch-advisories.py`) to keep audits
  deterministic and offline — Mollify itself never hits the network.
- Not yet built: an LSP server. Tracked in [docs/STATUS.md](docs/STATUS.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The bar: every change compiles, is tested,
and is documented; the tree stays `fmt` + `clippy -D warnings` clean.

## License

[MIT](LICENSE) © 2026 Favio Vázquez
