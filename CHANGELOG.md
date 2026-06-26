# Changelog

All notable changes to Mollify. This project follows the spirit of
[Keep a Changelog](https://keepachangelog.com/) and the JSON contract is
versioned by `schema_version` (currently `0.1`).

## Unreleased

## 0.1.0 - 2026-06-26

First public release. Distributed via PyPI (`uvx`/`pip install mollify`) and
crates.io (`cargo install mollify-cli`) — every channel ships the same
self-contained binary with agent integrations embedded.

### Engines & rules
- **Dead code:** `unused-file`, `unused-export`, `unused-import` (whole-statement
  and partial-name), `unused-variable` (F841), `unused-parameter`,
  `commented-code`, plus runtime cold-path (`cold-code`).
- **Dependency hygiene:** `unused-dependency`, `missing-dependency`, and
  `transitive-dependency` (venv `*.dist-info`-aware import→distribution mapping).
- **Architecture:** `circular-dependency`, `layer-violation` (presets),
  declarative contracts (`forbidden-import`, `independence-violation`), and
  rule-pack policies (`forbid_import`/`forbid_call`).
- **Complexity & cohesion:** `high-complexity`, churn×complexity `hotspot`,
  `low-cohesion` (LCOM*), and a `mollify metrics` report (Maintainability Index,
  Halstead, raw LOC).
- **Duplication:** token clone families with configurable thresholds.
- **Type health:** `untyped-function`.
- **Security:** eval/exec, shell, `sql-injection`, weak hash/cipher,
  insecure-random, unsafe deserialization, TLS, secrets, missing-timeout — each
  with a CWE id.
- **Supply chain:** `vulnerable-dependency` — live OSV (`/v1/querybatch`) by
  default with an offline advisory-DB fallback.

### Surfaces
- 21 CLI commands (incl. `metrics`, `graph`, `inspect`, `list`, `trace`,
  `explain`, `watch`, `fix`, `supply-chain`).
- Output formats: human, JSON (kind-discriminated), SARIF, GitHub annotations,
  JUnit XML.
- Gating: `--gate new-only` with **line-level** introduced-vs-inherited
  attribution; regression baselines (`--save-baseline`/`--baseline`/
  `--fail-on-regression`); `--brief`; `--min-confidence`.
- **MCP server** (`mollify mcp`) — 16 tools.
- **Language Server** (`mollify lsp`) — diagnostics on open/save plus live
  file-local diagnostics on edit.
- Inline `# mollify: ignore[<rule>]` suppressions; `.mollifyrc.json` config
  (severity, ignore, complexity/duplication thresholds, architecture layers &
  presets, contracts, policies).
- Agent integrations for Claude Code, OpenAI Codex, Cursor, Gemini CLI, and
  Devin/Cascade.

### Invariants
- Deterministic: identical input → byte-identical output.
- Evidence, not decisions: every finding carries a fingerprint, confidence tier,
  and human reason; only `certain` + `auto_fixable` findings are ever auto-fixed.
