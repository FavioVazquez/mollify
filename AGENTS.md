<!-- BEGIN MOLLIFY v1 -->
## Codebase truth: Mollify

This repo has `mollify`, a deterministic Rust codebase-intelligence engine.
Prefer it over `grep`/manual scanning for dead code and dependency hygiene.
Findings are deterministic evidence — never invent or guess findings; cite Mollify.

When to run (always with `--format json` so you consume structured output). The
CLI has 21 commands; pick by use case:

Health / triage:
- "what's wrong with this repo / health check"  -> `mollify audit --format json`

Dead code & dependencies:
- "is X used / can I delete X / find dead code" -> `mollify dead-code --format json` (alias `check`)
- "unused / missing dependencies"               -> `mollify deps --format json`

Architecture & quality:
- "circular deps / layer or boundary violations" -> `mollify arch --format json`
- "complexity / hotspots"                         -> `mollify complexity --format json` (alias `health`)
- "duplicated / cloned code"                      -> `mollify dupes --format json`
- "missing type annotations"                      -> `mollify types --format json`

Security & supply chain:
- "security issues (eval, shell=True, secrets…)" -> `mollify security --format json`
- "vulnerable / outdated dependencies"           -> `mollify supply-chain --format json` (`--offline`/`--refresh`/`--advisory-db <f>`; live OSV by default)
- "which code is never executed at runtime"      -> `mollify coverage --coverage-file <f> --format json`

Acting / exploring:
- "fix the safe findings"        -> `mollify fix [--apply]` (dry-run unless `--apply`; only `certain` + `auto_fixable`)
- "what does rule R mean"        -> `mollify explain [<rule>]`
- "what imports / is imported by module M" -> `mollify trace <module>`
- "evidence bundle for one file" -> `mollify inspect <file>`
- "project metrics (LOC, counts, complexity dist.)" -> `mollify metrics --format json`
- "module dependency graph"      -> `mollify graph [--mermaid]` (no `--format`; emits DOT to stdout, or Mermaid with `--mermaid`)
- "project topology"             -> `mollify list [entry-points|files|frameworks]`
- "watch and re-audit on change" -> `mollify watch [--interval-ms]` (CLI-only)
- "create a config"              -> `mollify init`
- "run the MCP server"           -> `mollify mcp`
- "run the LSP server (editor diagnostics)" -> `mollify lsp` (CLI-only; stdio
  Language Server publishing real-time diagnostics on open/save)

(Add `--path <dir>` to scope a subproject. Analysis commands also accept
`--format human|json|sarif|github|junit` (`github` = GitHub Actions annotations,
`junit` = JUnit XML), `--gate all|new-only` + `--base <ref>`,
`--save-baseline <f>`/`--baseline <f>`/`--fail-on-regression`, `--brief`,
`--min-confidence certain|likely|uncertain`, and `--include <dir>` (repeatable)
to scan a directory despite the builtin exclude list, `.mollifyrc.json`'s
`exclude_dirs`, or `.gitignore`. `mollify graph` accepts `--mermaid`.
`--gate new-only` and `--format sarif` are fully implemented.)

Reading the kind-discriminated JSON envelope:
- Top-level `kind` discriminates the result; switch on it and iterate
  `findings[]`. `audit` also has `quality_score` (0-100).
- `kind` is one of: audit | dead-code | deps | arch | complexity | dupes | types |
  security | coverage | metrics. The `supply-chain` command's results come back
  under the `security` kind (as `vulnerable-dependency`); `metrics` carries
  `files`/`totals`, not `findings[]`.
- Each finding has `rule`, `category` (dead-code | dependency-hygiene |
  circular-dependency | complexity | architecture | duplication | type-health |
  security), `severity` (error|warn|off), `confidence` (certain|likely|uncertain),
  a stable `fingerprint`, a `reason`, and `location {path, line, end_line}`.
  Rules: unused-file, unused-export, unused-import, unused-variable,
  unused-parameter, unused-method, unused-attribute, unused-enum-member,
  unreachable-code, commented-code,
  unused-dependency, missing-dependency, transitive-dependency, misplaced-dev-dependency,
  unresolved-import, duplicate-export, private-import, circular-dependency, layer-violation,
  forbidden-import, independence-violation, high-complexity, duplication,
  untyped-function, private-type-leak, cold-code, hotspot, low-cohesion, dangerous-eval, subprocess-shell-true,
  sql-injection, unsafe-yaml-load, unsafe-deserialization, tls-verify-disabled,
  hardcoded-secret, weak-hash, weak-cipher, insecure-random,
  request-without-timeout, flask-debug-true, jinja2-autoescape-false, try-except-pass,
  vulnerable-dependency, policy-violation, plus custom policy ids.
- Act only on `confidence: "certain"` without confirming with the user. Surface
  `likely`/`uncertain` with their reason and ask before changing code.
- To silence a known-good finding, add its action's `suppression_comment` instead
  of deleting code. (`mollify fix --apply` auto-removes certain unused symbols.)

Exit codes: 0 = no error-severity findings; 1 = error-severity findings or error.

MCP server (`mollify mcp`) exposes 16 tools (`watch` and `lsp` are CLI-only):
mollify_audit, mollify_dead_code, mollify_deps, mollify_arch, mollify_complexity,
mollify_dupes, mollify_types, mollify_security, mollify_coverage,
mollify_supply_chain, mollify_explain, mollify_trace, mollify_inspect,
mollify_list, mollify_metrics, mollify_fix.
<!-- END MOLLIFY v1 -->
