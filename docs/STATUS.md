# Mollify — Build Status / Log

This is the running build log. **Update it every working session** so context loss
never erases progress. It records: per-phase status, what compiles/tests, and
every deviation from `PLAN.md` (with rationale).

Legend: ✅ done & tested · 🟡 in progress · ⬜ not started · 🔵 scaffolded (compiles, stubbed)

## Environment constraints discovered (these shape the build)
- cargo 1.94.1, rustc 1.94.1, `cc`/`gcc` present. crates.io fetch works.
- **Git dependencies from GitHub are BLOCKED** (cargo → HTTP 403 via egress proxy).
  → The plan's `ruff_python_parser` (git-rev) path is not buildable here.
  See **ADR-0001**: we build on `tree-sitter-python` (crates.io) behind a parser
  abstraction; ruff crates remain the future migration.

## Deviations from PLAN.md (documented as required)
| # | Plan said | What we did | Why | Where |
|---|-----------|-------------|-----|-------|
| D1 | Parser = ruff_python_parser via pinned git rev (§3.2) | tree-sitter-python via crates.io, behind `mollify-parse` abstraction | GitHub git deps blocked by egress (403) | ADR-0001 |
| D2 | Crate names `config/types/parse/...` (§3.1) | `mollify-*` prefix (e.g. `mollify-types`) | avoid crates.io name clashes; clearer | this file |

## Phase status
- **Phase 0 — Skeleton + parser POC:** ✅
  - ✅ workspace, toolchain, gitignore
  - ✅ `mollify-types` (4 tests), `mollify-parse` (6 tests), `mollify-graph` (4 tests)
- **Phase 1 — MVP dead-code + deps:** ✅ (core complete, tested, dogfooded)
  - ✅ `mollify-graph` — discovery (ignore walker), path-sorted FileIds, dotted-name
    resolution (incl. src-layout + relative imports), import edges, BFS reachability
    from entry points, symbol-usage queries (internal count + cross-module + `from x import`).
  - ✅ `mollify-core` — dead-code engine (unused-file, unused-export) with confidence
    tiers (certain/likely/uncertain) + `__all__`/dunder suppression + dynamic-sink
    downgrade; deps engine (unused/missing dependency) parsing pyproject (PEP 621 +
    Poetry + PEP 735) with stdlib set + import→dist alias table; deterministic
    fingerprints (`<rule>:<xxh3>`); quality score; kind-discriminated reports. 9 tests.
  - ✅ `mollify-cli` — `mollify` binary: `audit`/`dead-code`(alias `check`)/`deps`/`init`,
    `--format human|json`, `--path`, CI exit code on errors. Dogfooded on a sample
    project (correct results: private→certain+autofix, public→likely, orphan file,
    missing numpy, unused rich/leftover-pkg; requests+stdlib+cross-module not flagged).
  - **Total: 23 tests green; `cargo build`, `cargo test`, `cargo clippy` clean.**
  - ✅ Phase-1 polish landed: **SARIF 2.1.0** output (`--format sarif`, `mollify-core/sarif.rs`);
    **`--gate new-only`** + `--base <ref>` (git change detection in `mollify-core/git.rs`,
    file-level introduced/inherited attribution — line-level base-worktree is the documented
    upgrade); framework entry-point plugins (done in Phase 2). Agent hooks now use `--gate new-only`.
  - ✅ **`.mollifyrc.json`** now read (`mollify-core/config.rs`): per-rule/category severity
    overrides (so teams can make rules `error` → CI/hooks block), `ignore` path substrings,
    complexity thresholds. Applied across every engine.
  - ✅ **`mollify fix`** (`mollify-core/fix.rs`): removes only `certain` + `auto_fixable`
    unused symbols, bottom-up; dry-run by default, `--apply` to write. Verified.
  - ⏳ Still open (nice-to-have): line-level gate (base-worktree), named arch presets,
    churn×complexity ranking, LSP, runtime/type intelligence (Phase 4).
- **Phase 2 — dupes + complexity + arch:** ✅ (all three engines done, tested, in `audit`)
  - ✅ **Framework plugins** (`mollify-core/plugins.rs`) — decorator registry (routes, tasks,
    fixtures, signal receivers, CLI commands, validators…) marks registered symbols reached;
    parser now captures decorators per def. The dominant false-positive killer.
  - ✅ **Architecture** (`arch.rs`) — circular-dependency detection via Tarjan SCC over the
    import graph (`graph.find_cycles()`), `circular-dependency` findings (Certain). Named
    boundary presets still pending.
  - ✅ **Complexity** (`complexity.rs`) — cyclomatic + cognitive per function (computed in the
    parser over the tree), `high-complexity` findings above thresholds. Churn×complexity
    hotspot ranking still pending (needs git log --numstat).
  - ✅ **Duplication** (`dupes.rs`) — Rabin-Karp token-clone detector (Python tokenizer,
    literal-blinded), maximal-window extension + clone families. SA-IS+LCP is the documented
    upgrade. (jscpd-class detector.)
  - ✅ CLI: `arch`, `complexity` (alias `health`), `dupes`; all five engines fold into `audit`.
  - **39 tests green.**
- **Phase 3 — AI/MCP + plugins:** 🟡 (MCP server done; plugins pending)
  - ✅ `mollify-mcp` — a minimal, dependency-light **MCP stdio server** (newline-delimited
    JSON-RPC 2.0): `initialize`/`ping`/`tools/list`/`tools/call`, tools `mollify_audit`/
    `mollify_dead_code`/`mollify_deps`, kind-discriminated text results, stderr-only logging.
    5 unit tests + verified end-to-end over real stdio (initialize → tools/list → tools/call
    audit returns kind=audit score=77). Wired as `mollify mcp`.
  - **This makes every platform's MCP registration functional** (one server, many front-ends).
  - ⬜ framework entry-point plugins, LSP, agent-skills repo packaging.
- **Phase 4 — runtime/type intelligence:** 🟡 (type-health shipped; runtime/notebooks/security pending)
  - ✅ **Type-health** (`typehealth.rs`, `mollify types`) — annotation-coverage engine: parser
    captures per-function param/return annotation counts (excluding self/cls); flags
    fully-untyped public functions (`untyped-function`, category `type-health`). A
    Python-specific differentiator with no fallow analog. Folded into `audit`. 1 test.
  - ✅ **Security** (`security.rs`, `mollify security`) — bandit-style candidate producer:
    dangerous-eval, subprocess-shell-true, unsafe-yaml-load, unsafe-deserialization,
    tls-verify-disabled, hardcoded-secret. Category `security`. Folded into `audit`. +tests.
  - ⬜ runtime-coverage merge (coverage.py/sys.monitoring), notebooks (.ipynb),
    churn×complexity ranking, supply-chain CVE join, LSP, named arch presets.
- **Agent integrations** (`.devin/` skills+rules+hooks, `.windsurf/` workflows): ✅ shipped, honoring the real CLI
  - `.devin/skills/mollify/SKILL.md` (+ `references/cli-reference.md`, `references/json-contract.md`)
  - `.devin/rules/mollify.md` (glob `**/*.py`)
  - `.devin/hooks.v1.json` (Devin/Claude-compatible: PostToolUse + Stop) and
    `.windsurf/hooks.json` (Cascade: post_write_code) → `scripts/mollify-report.sh` (verified)
  - `.windsurf/workflows/mollify-audit.md`, `mollify-cleanup.md`
  - **Note:** hooks are *advisory* (run audit + surface findings), not blocking,
    because the `--gate new-only` blocking gate is not built yet. Upgrade them to
    blocking once the gate + `attribution` land. README.md added.
  - User confirmed `.devin` = hooks/skills/rules, `.windsurf` = workflows.
  - **All four other platforms shipped** (generated + verified via a dynamic Workflow —
    parallel generate → adversarial verify gate → fix loop; all passed first-pass):
    - **Claude Code:** `.mcp.json`, `.claude/skills/mollify/SKILL.md` (+ references), `.claude/commands/mollify-{audit,cleanup}.md`, `.claude/settings.json` (PostToolUse+Stop hooks → mollify-report.sh).
    - **Codex:** `AGENTS.md` (delimited block), `.codex/config.toml` (`[mcp_servers.mollify]`), `.agents/skills/mollify/SKILL.md` (+ references) — the portable open-standard skill.
    - **Cursor:** `.cursor/rules/mollify.mdc` (glob comma-string), `.cursor/mcp.json`, `.cursor/commands/mollify-audit.md`.
    - **Gemini CLI:** `GEMINI.md`, `.gemini/settings.json`, `.gemini/commands/mollify/audit.toml`.
    - All JSON/TOML validated; all reference only real commands; MCP all → `mollify mcp`.

## Docs & infra (shipped)
- `README.md`, `CONTRIBUTING.md`, `LICENSE` (MIT).
- `docs/usage.md`, `docs/architecture.md`, `docs/configuration.md`, `docs/ci-integration.md`,
  `docs/adr/0001-parser-tree-sitter.md`, and this `docs/STATUS.md`.
- `.github/workflows/ci.yml` — fmt + clippy(-D warnings) + test, plus a dogfood SARIF upload.
- Code is `cargo fmt`-clean and passes `clippy -D warnings`.

## Verification protocol (every commit)
1. `cargo build` clean. 2. `cargo test` green. 3. `cargo clippy` (best-effort). 4. Update this file. 5. Commit with a descriptive message (author: Favio Vázquez).

## Invariants we must not break (from RESEARCH.md §2.11)
Determinism · candidate-producer/verifier separation · versioned `kind`-discriminated
output · five co-equal analysis areas · evidence-preserving findings.
