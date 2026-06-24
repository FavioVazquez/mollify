# Using Mollify

Mollify analyzes a Python project and reports **deterministic evidence** about
dead code, dependency hygiene, circular dependencies, complexity, and
duplication. It never decides for you — every finding carries a confidence tier,
a reason, and a stable fingerprint.

## Install

```bash
# From source (the only path today)
git clone https://github.com/FavioVazquez/mollify
cd mollify
cargo build --release
# binary at ./target/release/mollify  (put it on your PATH)
```

## Commands

| Command | What it reports |
|---|---|
| `mollify audit` | Everything, plus a 0–100 quality score. Start here. |
| `mollify dead-code` (`check`) | Unused files and unused top-level functions/classes/variables. |
| `mollify deps` | Declared-but-unused and imported-but-undeclared distributions. |
| `mollify arch` | Import cycles, layer-boundary violations, and custom policy violations. |
| `mollify complexity` (`health`) | Functions over the cyclomatic/cognitive thresholds + churn×complexity hotspots. |
| `mollify dupes` | Duplicated code blocks (clone families). |
| `mollify types` | Fully-untyped public functions (annotation health). |
| `mollify security` | Bandit-style security candidates. |
| `mollify coverage --coverage-file <f>` | Reachable-but-never-executed functions (cold paths) from a coverage.py JSON report. |
| `mollify fix [--apply]` | Remove safe (certain) unused symbols. Dry-run unless `--apply`. |
| `mollify explain [<rule>]` | Explain a rule (semantics/confidence/action); lists all rules with no argument. |
| `mollify trace <module>` | A module's import neighborhood: what it imports and what imports it. |
| `mollify init` | Write a starter `.mollifyrc.json`. |
| `mollify mcp` | Start the MCP server for coding agents (stdio). |

Common flags: `--path <dir>`, `--format human|json|sarif`,
`--gate all|new-only`, `--base <ref>`.

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
```

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
