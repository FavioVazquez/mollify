# Using Mollify

Mollify analyzes a Python project and reports **deterministic evidence** about
dead code, dependency hygiene, circular dependencies, complexity, and
duplication. It never decides for you — every finding carries a confidence tier,
a reason, and a stable fingerprint.

## Install

```bash
uvx mollify audit                 # one-off via uv (no install)
uv tool install mollify           # or: pip install mollify
npm install --save-dev mollify    # Node projects (npx mollify audit)
cargo install mollify-cli         # from crates.io (binary: mollify)

# from source:
git clone https://github.com/FavioVazquez/mollify && cd mollify
cargo build --release             # binary at ./target/release/mollify
```

See the [README](../README.md#install) for the full matrix and the
`mollify init --agent <name>` agent-integration installer.

## Commands

| Command | What it reports |
|---|---|
| `mollify audit` | Everything, plus a 0–100 quality score. Start here. |
| `mollify dead-code` (`check`) | Unused files, top-level functions/classes/variables, **class members (methods/attributes), enum members**, imports, locals/parameters, **unreachable code**, and duplicate re-exports. |
| `mollify deps` | Declared-but-unused, imported-but-undeclared (missing/transitive), **misplaced dev dependencies**, and **unresolved/broken internal imports**. |
| `mollify arch` | Import cycles, layer-boundary violations, contracts, **cross-package private-import (interface) violations**, and custom policy violations. |
| `mollify complexity` (`health`) | Functions over the cyclomatic/cognitive thresholds + churn×complexity hotspots. |
| `mollify dupes` | Duplicated code blocks (clone families). |
| `mollify types` | Fully-untyped public functions (annotation health) + **private-type leaks** in public signatures. |
| `mollify security` | Bandit-style security candidates with CWE ids (eval, shell, deserialization, weak crypto, SQLi, TLS, secrets, **flask debug, jinja2 autoescape, broad except: pass**, …). |
| `mollify coverage --coverage-file <f>` | Reachable-but-never-executed functions (cold paths) from a coverage.py JSON report. |
| `mollify supply-chain [--offline]` | Pinned/locked versions **and declared ranges** matched against vulnerability advisories (ranges resolve via the installed venv, else flagged when they permit a vulnerable version). Live OSV by default; `--offline` uses the local DB. |
| `mollify fix [--apply]` | Remove safe (certain) unused symbols. Dry-run unless `--apply`. |
| `mollify explain [<rule>]` | Explain a rule (semantics/confidence/action); lists all rules with no argument. |
| `mollify trace <module>` | A module's import neighborhood: what it imports and what imports it. |
| `mollify watch [--interval-ms]` | Re-run `audit` whenever a Python file changes (poll-based; Ctrl-C to stop). |
| `mollify inspect <file>` | Evidence bundle for one file: its findings plus its import neighborhood. |
| `mollify list [entry-points\|files\|frameworks]` | Project topology. |
| `mollify metrics` | Maintainability Index, Halstead, raw LOC, per-file complexity. |
| `mollify graph [--mermaid]` | Export the module import graph (Graphviz DOT or Mermaid). |
| `mollify lsp` | Run the Language Server (stdio) for real-time editor diagnostics. |
| `mollify init` | Write a starter `.mollifyrc.json`, or scaffold agent integrations with `--agent <name>` / `--all`. |
| `mollify mcp` | Start the MCP server for coding agents (stdio). |

### Regression baselines (CI gate without git)

`--save-baseline <f>` snapshots the current finding fingerprints; later runs use
`--baseline <f>` to report only what's **new** since then, and
`--fail-on-regression` exits non-zero when any new finding appears. `--brief`
prints the report but always exits 0 (advisory). This works without git and
survives file moves (fingerprints are content-derived).

```bash
mollify audit --save-baseline .mollify/baseline.json     # once, on a clean main
mollify audit --baseline .mollify/baseline.json --fail-on-regression   # in CI
```

Common flags: `--path <dir>`, `--format human|json|sarif|github|junit`,
`--gate all|new-only`, `--base <ref>`, `--min-confidence certain|likely|uncertain`,
and the regression-baseline flags (`--save-baseline`/`--baseline`/`--fail-on-regression`/`--brief`).

## Editor integration (LSP)

`mollify lsp` runs a Language Server over stdio that publishes mollify diagnostics
on file open/save. Point your editor's generic LSP client at `mollify lsp` for
Python files (VS Code via a generic LSP bridge, Neovim `vim.lsp.start`, Zed, etc.).
It reuses the deterministic audit, so editor results match CI exactly.

## Examples

```bash
# Human-readable health check
mollify audit --path .

# Machine-readable, for scripts / agents
mollify dead-code --format json

# CI: only fail on issues this PR introduced, vs the main branch
mollify audit --gate new-only --base origin/main

# Code scanning (GitHub/GitLab)
mollify audit --format sarif > mollify.sarif

# Preview safe fixes, then apply
mollify fix
mollify fix --apply

# Understand a rule, or trace a module's dependencies
mollify explain circular-dependency
mollify trace app.services.billing

# Supply-chain: refresh the advisory DB (out-of-band), then scan pinned versions
python3 scripts/fetch-advisories.py .mollify/advisories.json
mollify supply-chain
```

## Supply-chain advisories

`mollify supply-chain` matches dependency versions against vulnerability
advisories. **Pinned/locked** versions (`requirements*.txt` `==` pins,
`poetry.lock`, `uv.lock`) are matched precisely. **Declared ranges**
(`requirements` specifiers, PEP 621 `[project].dependencies`, Poetry caret/tilde)
are resolved to the concrete **installed** version when a virtualenv is present;
otherwise the range is flagged (at `uncertain` confidence) when it *permits* a
vulnerable version.

**Live by default, offline fallback.** Advisories change constantly, so the
`supply-chain` command queries **OSV.dev live** for each pinned package (honoring
`HTTPS_PROXY`). If the network is unavailable, it falls back to the local
advisory DB. Flags:

- `--refresh` — after a live fetch, cache the advisories to the DB path for later offline runs.
- `--offline` — skip the network entirely and use the local DB only (fully deterministic).
- `--advisory-db <path>` — choose the DB (default `.mollify/advisories.json`).

The DB uses the `mollify-advisories/1` schema; regenerate/seed it with
`scripts/fetch-advisories.py` (OSV.dev export, falling back to pyup safety-db).
A small real-CVE sample lives at `examples/advisories.sample.json`.

**`mollify audit` stays offline and deterministic** — it folds in supply-chain
findings only from the local DB (`.mollify/advisories.json`) when present, never
hitting the network. Use `mollify supply-chain --refresh` (or the script) to keep
that DB current.

## Confidence tiers

- **certain** — provable (e.g. a private unused symbol with no dynamic dispatch
  in scope). Only these are auto-fixable.
- **likely** — strong static signal with a residual dynamic risk. Suggested.
- **uncertain** — public surface, or the module uses `getattr`/`eval`/`importlib`.
  Reported for review only.

## Exit codes

`0` when there are no `error`-severity findings, `1` otherwise. Findings are
`warn` by default; raise them to `error` in `.mollifyrc.json` to gate CI. See
[configuration.md](configuration.md).

## Suppressing a finding

Add the finding's suppression comment on the relevant line, e.g.
`# mollify: ignore[unused-export]`, or set the rule/category to `off` in config.
