# Mollify — Build Plan: Rust-native Codebase Intelligence for Python

> **Grounded in fallow's real source** (`fallow-rs/fallow` v2.102.0, full tree read, not just docs). Surface-area and invariants below reflect that verification — see `RESEARCH.md` for the per-claim corrections (notably: fallow ships ~118 plugins not 122; ~29 CLI subcommands with `check` canonical + `dead-code` as alias; 11 output formats; 25 MCP tools; six ADRs; and graph construction is **not yet incremental** even in fallow).

## 1. Vision & Positioning

**Mollify is the codebase truth layer for Python coding agents.** It is a Rust-native, sub-second, deterministic codebase-intelligence engine that gives both humans and AI agents structured, inspectable repo truth — dead code, duplication, circular dependencies, complexity hotspots, architecture boundaries, and package hygiene — instead of forcing them to reconstruct structure from `grep`. The core thesis, ported directly from fallow: **no AI invents findings; every result is deterministic, reachability-backed evidence with a stable fingerprint, a confidence level, and a reason.** This is the wedge against both the LLM-judge tools (Sourcery, Qodo — non-deterministic, token-hungry) and the per-file/name-table Python incumbents (pyflakes/ruff are per-file; vulture/deadcode are name-table, not reachability). Two crisp differentiators define us: **(a) Python has no fallow** — fallow is TypeScript/JavaScript-only and will not cross over; **(b) a genuinely Rust core** — the closest Python competitor, Skylos, is Python + tree-sitter + optional LLM, so a real Rust engine (the Ruff playbook, ~100x over Flake8) is a measurable, hard-to-copy moat.

**Name rationale.** *fallow* = land deliberately rested so it recovers — gentle, non-destructive stewardship. *mollify* = "to soften, soothe, appease" (Late Latin *mollificare*, from *mollis* "soft"). Both reject the violent vocabulary of "kill/slash/prune." The narrative: Mollify *softens* the friction between agents and a codebase and *soothes* the rough edges AI agents leave behind. The `-fy` verb form is CLI-friendly (`mollify .`, `mollify check`, `mollify fix`), and there is a natural snake pun — "soothe your Python," "a gentler way to tame your Python."

**Name availability (verified 2026-06-24).**
- PyPI `mollify` — **AVAILABLE** (404). PyPI `mollify-cli` — **AVAILABLE**.
- crates.io `mollify` — **AVAILABLE**. crates.io `mollify-cli` — **AVAILABLE**.
- npm `mollify` — **TAKEN** (old minify middleware, v6.0.0). Use scope `@mollify/*` for any npm-distributed skill/installer.
- GitHub: no Python code-intelligence project owns the name; existing repos are unrelated. Recommendation: grab org **`mollify-rs`** (mirrors `fallow-rs`, signals the Rust core).

**Decision:** proceed with the name. Reserve `mollify` + `mollify-cli` on PyPI and crates.io now (placeholder sdist + cargo placeholder), the `@mollify/*` npm scope, and the `mollify-rs` GitHub org.

---

## 2. Scope: Capability Matrix

Each row maps a fallow capability → Mollify's Python implementation → the Python-specific "even more" we add. Versioning: **v1** (MVP), **v2** (depth), **later**.

| Fallow capability | Mollify Python approach | Python "even more" | Phase |
|---|---|---|---|
| Dead code via module/symbol reachability | Mark-reachable BFS/DFS from entry-point roots over the import + symbol graph; flag unreached files/exports/members | Confidence-tiered verdicts (certain/likely/uncertain); dead members (methods, properties, `__all__` entries, enum members); whole-file unreachability | v1 |
| Unused / missing / transitive / dev-vs-prod deps | deptry-equivalent: reconcile first-party imports vs declared distributions; import-name→distribution mapping table | PEP 735 dependency-groups + extras + `uv` awareness; **unused-AND-vulnerable** composite verdict (join with CVE) | v1 |
| Unresolved / unlisted imports, circular deps | Resolve via module resolver; cycle detection during graph build | Namespace-package (PEP 420) cycle handling; conditional/`TYPE_CHECKING` import classification | v1 |
| Duplication (suffix array + LCP, 4 modes) | Port the SA-IS suffix-array + LCP engine; Python tokenizer with strict/mild/weak/semantic normalization | **SA-IS+LCP whole-corpus sub-quadratic engine + fingerprinted clone families + refactor suggestions + SARIF/MCP** — note Type-2 itself is NOT unprecedented (lizard `-Eduplicate` already does token-normalized Type-2 for Python); the moat is the engine + families + integration, not "first to Type-2" | v2 |
| Complexity hotspots | Cyclomatic (McCabe) + cognitive (SonarSource model) per function/file | **Churn × complexity hotspot ranking** (git change-frequency × complexity/MI) — empty quadrant in Python; Maintainability Index | v2 |
| Architecture boundaries (layered/hexagonal/feature-sliced/bulletproof presets) | Named presets compiled to layer/forbidden/independence/cycle contracts over the import graph | Symbol-level public-interface enforcement (which symbols may cross), like tach; named opinionated presets (no Python equivalent exists) | v2 |
| Dependency hygiene unified with the rest | Single pass folds boundary + dep-hygiene + cycles into one report | Monorepo first-party-vs-external disambiguation via unified workspace model | v2 |
| Framework plugins (~118 built-in; data + dynamic resolution) | `Plugin` trait: static entry-point globs + convention-used symbols + decorator registries + dynamic AST config resolution | Django/FastAPI/Flask/Celery/pytest/SQLAlchemy/Pydantic/click/typer + `[project.scripts]`/`entry_points` (richer, more standardized than JS) | v1 (core set) / v2 (breadth) |
| Caching + git-diff incrementalism | Persistent cache (bitcode), git changed-files, base worktree, introduced-vs-inherited attribution, NewOnly PR gate | Same model, ported directly | v1 (cache) / v2 (worktree attribution) |
| Parallelism | rayon, eager module-level incrementalism (pyrefly model) | — | v1 |
| CLI / JSON / SARIF / MCP / LSP / Agent Skills | Full surface; typed JSON contract crate | `mollify-skills` repo; `auto_fixable` actions array | v1 (CLI/JSON/SARIF) / v2 (MCP/LSP/skills) |
| Runtime coverage merge (fallow's paid layer) | Merge coverage.py / SlipCover against the reachability graph | **30-40x cheaper in Python** via PEP 669 `sys.monitoring` (~5% overhead); three-state verdict (static-dead / cold / hot) | later |
| Type-coverage / type-quality | (no fallow analog — fallow excludes type findings) | **Net-new:** annotation coverage, `Any`-leakage, untyped-def %, per-module type-health score | later |
| Security + secrets | (fallow scopes out) | bandit-style high-signal AST checks + entropy secret detection, folded into the same pass | later |
| Notebooks | (no fallow analog) | Native `.ipynb` cross-cell name resolution, unused-cell/variable, execution-order hazards | later |
| Async hazards | (no analog) | Native flake8-async-style rules (blocking-in-async, unawaited coroutine, fire-and-forget task) | later |

---

## 3. Architecture

### 3.0 Non-negotiable invariants (ported from fallow's ADRs/docs)

These are load-bearing — verified in fallow's `CLAUDE.md`/`docs/`, and Mollify must keep them:
- **Determinism.** Identical input → byte-identical output across runs/platforms/CI. No AI in the analyzer; any randomness seeded. Use deterministic-iteration maps (Rust `FxHashMap`, fallow ADR-003) and **path-sorted stable FileIds** (ADR-004). Rust gives us this for free where Python tools struggle.
- **Candidate-producer vs. verifier separation.** Mollify emits *evidence* (candidates, traces, metrics, confidence) and never decides the irreversible verdict. `fix` previews by default; auto-apply is explicit and gated to `Certain` findings. Security/dead-code surface candidates; the agent/human owns judgement.
- **Versioned output contract.** Every command emits a JSON envelope with a discriminating top-level `kind`; ship a first-class JSON Schema. Clients depend on the JSON shape, not Rust structs — the `types` crate's serde output *is* the public API.
- **Five co-equal analysis areas.** Unused code · circular deps · duplication · complexity hotspots · boundary violations — sharing one discovery/parse pass. Lead with dead-code for the MVP, but architect all five from day one. Never market Mollify as "just a dead-code tool."
- **Evidence-preserving findings.** Every issue carries its trace (import chain, reachability proof, reference counts) so it can be audited.

### 3.1 Workspace layout (Cargo workspace, `crates/*`, shared version)

Mirrors fallow's proven 12-crate decomposition, adapted for Python:

```
mollify/
  Cargo.toml                 # workspace, members = ["crates/*"], single shared version
  crates/
    config/      # parse .mollifyrc(.json/.jsonc/.toml), pyproject [tool.mollify],
                 # framework presets, rule packs; manifest discovery (pyproject/setup.cfg/
                 # setup.py/requirements*/poetry/pdm/uv); source-root & namespace discovery
    types/       # serde contract: findings, confidence, actions[]/auto_fixable,
                 # suppression metadata, fingerprints. The serialization contract crate.
    parse/       # ruff_python_parser + ruff_python_ast wrapper; trivia/comments;
                 # parsed-tree cache; inline-suppression scanning
    graph/       # import + symbol graph; module resolver; ADR-style stable path-sorted
                 # FileIds; flat Vec<Edge> + range indices; re-export/__all__ propagation;
                 # reachability.rs (mark-reachable BFS); cycle detection
    core/        # orchestration: analyze/ (dead-code predicates, unused files/exports/
                 # members/deps), plugins/, duplicates/, complexity/, arch/, hygiene/
    cli/         # per-command modules; report/ formatter dispatch; license/; coverage/
    lsp/         # tower-lsp-server: diagnostics, code actions, code lens, hover
    mcp/         # stdio MCP server wrapping the CLI
    pyext/       # pyo3/maturin bindings — wheel packaging + Python-side env/search-path
                 # discovery (importlib/sysconfig) and import→distribution metadata
    license/     # offline Ed25519 JWT verification, grace ladder (paid runtime layer)
    coverage/    # coverage.py/SlipCover ingest, sys.monitoring normalization (paid)
    benchmarks/  # criterion microbench + comparative wall-clock harness
```

### 3.2 Parser / AST / semantic foundation — **decision: ruff crates, pinned git rev**

Build on **`ruff_python_parser` + `ruff_python_ast` + `ruff_text_size`** (all **MIT** — license check clean), pinning **all** ruff crates to a **single shared git rev** to avoid AST-type mismatches (pyrefly's current rev: `db5aa0a5f1b92cb91d910bf0866a967554dd94f5`). Treat **`ruff_python_semantic`** as a **reference, not a drop-in**: pyrefly — the engine we adopt — consumes only parser+AST+text_size and **hand-rolls its own binding/scope/resolution**; ty's semantic model is a separate Salsa-coupled `ty_python_semantic`. So reuse the parser+AST and build resolution/import-graph ourselves, mirroring pyrefly.

**Rationale.** These are the de-facto Rust Python foundation: both ty (Astral) and pyrefly (Meta) use them. The parser is hand-written recursive-descent, error-resilient, tracks full source ranges, and is battle-tested. RustPython's parser is deprecated (its own README redirects here). tree-sitter-python is a generic untyped CST — a complement for editor latency, not a foundation.

**Consumption strategy.** Astral does **not** publish these to crates.io and offers no API-stability guarantee. **Pin a git revision** (pyrefly's proven pattern) — low friction, tracks upstream. Be prepared to **vendor a fork** if churn becomes painful. **Do not** depend on the unofficial crates.io ruff forks. Add **`libcst` (Rust, MIT)** only when we ship format-preserving autofixes; add **tree-sitter-python** only as an optional fast-reparse layer for LSP keystroke latency; reserve **pyo3** for the wheel/env-discovery sidecar.

**Engine model.** Start with **pyrefly's eager + rayon, module-level-incremental** architecture (compute exports → lower each module to bindings in isolation → resolve, pulling in other modules' solutions). It is simpler than Salsa, has predictable memory, and excellent batch throughput (~1.8M LOC/sec on pyrefly). Adopt **ty's Salsa query model** (`salsa` + a `ruff_db`-style VFS) only later, when the LSP needs keystroke-latency incrementality.

### 3.3 Module/symbol graph & reachability engine (`graph`)

- **Stable IDs:** path-sorted `FileId`s for cross-run determinism (fallow ADR-004).
- **Flat edge storage:** contiguous `Vec<Edge>` with range indices, not pointer adjacency lists (cache-friendly, fallow ADR-002).
- **Re-export / `__all__` propagation:** iterative resolution with cycle detection (fallow ADR-005); `__all__`, star-imports (expand against source `m.__all__`), and re-export chains resolved to real edges.
- **Reachability:** mark-reachable BFS from entry-point roots; symbols not reached = unused. Roots come from plugin `entry_patterns` + packaging entry points + framework decorator registries. Reachability lives in `graph/reachability.rs` (not in predicates — fallow's docs mis-attribute this).
- **Cycles** detected during graph construction and reported as a first-class finding.

### 3.4 Duplication engine (`core/duplicates`)

Port fallow's suffix-array + LCP design: `tokenize_file()` → `normalize_and_hash()` → `CloneDetector` builds suffix array + LCP arrays in **O(n log n)** (no quadratic pairwise) → `group_into_families()` → mirrored-directory detection → suppression + `min_occurrences` filtering → deterministic sort. Four normalization modes: **Strict** (Type-1 exact), **Mild** (default, light structural), **Weak** (string-blind), **Semantic** (identifier + literal blind = Type-2 renamed-variable). The tokenizer is **reimplemented for Python** (indentation/significant-whitespace handling, Python token blinding). Thresholds: `min_tokens`, `min_lines`, `skip_local`. Stable `dup:<hex>` fingerprints; optional CODEOWNERS/directory grouping.

### 3.5 Complexity / churn engine (`core/complexity`)

- **Cyclomatic** (McCabe) + **cognitive** (SonarSource nesting-weighted) per function/method/file. Match radon/ruff/complexipy outputs rather than reinvent — these metrics are commoditized.
- **Maintainability Index** (radon formula).
- **Churn × complexity hotspots** — the differentiator: read git history (via `git2`/shelling to git) for per-file change frequency, multiply by complexity/MI/duplication to produce a ranked refactor-priority score. This combined hotspot ranking is unfilled in the Python ecosystem (wily reports trends only; complexipy ratchet is diff-direction only).

### 3.6 Architecture-boundary engine (`core/arch`)

Named **presets** compiled into primitive contracts over the import graph:
- `layered` → ordered layers (higher imports lower, never reverse, incl. indirect).
- `feature-sliced` → independence (slices must not import each other).
- `hexagonal` → forbidden contracts (domain may not import infrastructure) + symbol-level port boundaries.
- `bulletproof` → layers + forbidden + no-cycles.

Symbol-level public-interface enforcement (which symbols may cross a boundary) goes beyond import-linter's edge-only model and matches tach's strongest primitive. Containers (repeated layer pattern per feature package) supported. Stale-allowlist alerting like import-linter's `unmatched_ignore_imports_alerting`.

### 3.7 Dependency-hygiene engine (`core/hygiene`)

deptry-equivalent rule set: **missing** (imported, undeclared), **unused** (declared, never imported), **transitive** (used but only available transitively), **misplaced-dev** (dev dep used in prod code), **stdlib-listed**. Parses pyproject (PEP 621), Poetry, PDM, uv, requirements*, setup.cfg/py; understands dependency groups/extras/dev-vs-prod. Import-name→distribution mapping via installed `*.dist-info` metadata **plus a maintained alias table** (the `cv2`→`opencv-python`, `yaml`→`PyYAML`, `sklearn`→`scikit-learn` long tail) — a durable moat.

### 3.8 Framework plugin system (`core/plugins`)

A `Plugin` **trait** of **static defaults + dynamic AST resolution** (the accurate fallow model — not "pure static data"). Each plugin declares: `entry_patterns()` / `entry_pattern_rules()` / `entry_point_role()`; `config_patterns()` / `resolve_config()` (parse config AST → dynamic facts); `used_exports()` / `used_export_rules()` (convention-used symbols); a **decorator registry** (framework decorators that mark a symbol reached even with zero in-repo callers — `@app.route`, `@app.get`, `@task`, `@pytest.fixture`, `@receiver`, `@app.command`); `path_aliases()`. Emits a `PluginResult { entry_patterns, used_exports, referenced_dependencies, provided_dependencies, path_aliases }`. Plugins ship as pure data with no executable code (any future executable checks sit behind explicit trust opt-in). Python's standardized packaging entry points (`[project.scripts]`, `[project.entry-points]`, setup.cfg) are a first-class, richer entry-point source than JS — a strength to exploit.

### 3.9 Caching + git-diff incrementalism (`cli/audit`)

Persistent cache under `.mollify/cache/`, encoded with **bitcode**, extraction cache capped by `MOLLIFY_CACHE_MAX_SIZE` (default 256 MB, LRU eviction). Audit incrementalism: `git changed-files` with base ref by precedence `--changed-since` > `MOLLIFY_AUDIT_BASE` > auto-detect (`@{upstream}` merge-base → remote default → local main/master). For the `NewOnly` gate, spin an isolated **git worktree** at the base, re-run the same passes, capture a hashed `AuditKeySnapshot`. Cache key = **xxHash3-64** of (cache version, CLI version, base SHA, config hash, changed-files list, production settings, workspace config, baseline paths). Attribution partitions findings into **introduced vs inherited**; the NewOnly verdict gates only newly introduced issues. Base-snapshot cache capped at 16 MiB. `--no-cache` disables.

### 3.10 Parallelism

**rayon** throughout: parallel cache-aware parsing, per-module binding/lowering, per-file complexity, parallel suffix-array tokenization. Eager module-level incrementalism (re-process changed module + dependents). **Realistic scope note:** even fallow's graph construction is single-threaded and not yet incremental (its `watch` rebuilds the full graph; only extraction + audit base-snapshots are cached). So Mollify v1 targets **full-project speed** (parse/extract in parallel, fast single-pass graph), with audit base-snapshot caching in v2 and true incremental graph (the Salsa option) deferred to the LSP/watch phase — exactly fallow's own trajectory.

---

## 4. Handling Python's Dynamism

Dead-code detection in Python is undecidable in general; any tool claiming boolean certainty is wrong on some inputs. Mollify uses a **confidence-level model** attached to every verdict, with a reason string.

### 4.1 Confidence levels

- **Certain** — syntactically unreachable code (after `return`/`raise`/`break`), unused parameters, module-private symbol with scope-tracked single binding and no dynamic sink in scope. → **safe to auto-fix.**
- **Likely** — module-private symbol, scope-tracked, no nearby dynamic sink; or unreached export in an application (not a library). → **suggest fix.**
- **Uncertain** — public name, symbol near `getattr`/`eval`/`exec`, framework-adjacent, or reachable only through a dynamic dispatch we cannot resolve. → **report only, never auto-fix.**

This is materially better than vulture's flat 60% and scope-blind name matching.

### 4.2 Dynamic-construct handling

- `getattr`/`setattr`/`__getattr__`: literal name → resolve precisely; dynamic expression → mark same-named attributes possibly-reached (Uncertain). A class defining `__getattr__` suppresses unused-attribute findings on that class (high-confidence suppression). This wins the case vulture fails.
- `importlib.import_module`/`__import__`: literal target → real edge; computed target → seed from config/conventions, never the AST.
- `eval`/`exec` of non-literal code → downgrade all symbols in that scope to Uncertain (reachability sink).
- Decorators → decorator-registry model marks registered symbols reached.
- Metaclasses / `__init_subclass__` / descriptors → classes with registering metaclasses treated as roots.
- `__all__` / `entry_points` → public-API roots **for libraries**; for applications they do not protect symbols (deletion safety differs by project type — Mollify infers/accepts project type).
- Monkeypatching (`module.attr = x`) → target name marked reachable.
- PEP 420 namespace packages → membership inferred from directory structure.
- Conditional imports (`try/except ImportError`, `if TYPE_CHECKING:`, version guards) → both branches reachable; `TYPE_CHECKING` imports are type-only roots.
- Star imports → expand against source `__all__`/public names or all downstream resolution is unsound.

### 4.3 Entry-point / allowlist strategy

Roots = direct calls + framework registration (plugins) + packaging entry points (`[project.scripts]`/`[project.entry-points]`, console_scripts) + `__main__`/`conftest.py`/`test_*.py` + public exports + bounded dynamic dispatch. User config in `.mollifyrc`/pyproject adds entry-point selectors (by `name`, `decorators`, `base_classes`, `parent` — matching Skylos's proven model) and `ignore_imports`/allowlists with **stale-allowlist alerting**. Built-in framework plugins are the primary false-positive killer (the dominant reason engineers abandon Python dead-code tools); per-user whitelist maintenance is the fallback, not the default.

---

## 5. CLI Command Surface

Mirrors fallow. Binary: `mollify`.

| Command | Behavior |
|---|---|
| `mollify` / `mollify audit` | Full unified single-pass report (quality score, hotspots, dup families, architecture, dep hygiene, cleanup) |
| `mollify health` | Quality score 0–100 + hotspots |
| `mollify dead-code` | Reachability-based unused files/exports/members/deps |
| `mollify dupes` | Duplication families |
| `mollify deps` | Dependency hygiene (missing/unused/transitive/misplaced) |
| `mollify arch` | Architecture-boundary contract check |
| `mollify security` | Reachability-filtered candidates (later phase) |
| `mollify trace <symbol>` | Caller/callee chains |
| `mollify watch` | Continuous re-analysis |
| `mollify fix [--dry-run]` | Apply auto-fixable findings (LibCST-backed, format-preserving) |
| `mollify init` | Scaffold config, detect frameworks |
| `mollify license` | Activate/refresh paid runtime |

> **Naming note (from fallow's real CLI):** fallow's canonical command is `check` with `dead-code` as an alias, and `audit` with `review` as an alias; it exposes ~29 subcommands total (incl. `flags`, `explain`, `schema`, `list`, `workspaces`, `migrate`, `telemetry`, `decision-surface`). Mollify keeps the clearer names as canonical (`dead-code`, `audit`) and adds tooling subcommands (`list`, `explain`, `*-schema`, `migrate` to import vulture/deptry/jscpd config) as we grow. Scoping flags to port: `--changed-since`/`--base`, `--diff-file`/`--diff-stdin`, **`--churn-file`** (non-git VCS history for churn×complexity on hg/perforce), `--gate new-only|all`, `--group-by owner|directory`.

**Config file `.mollifyrc`** (precedence: `.mollifyrc.json` > `.mollifyrc.jsonc` > `.mollifyrc.toml` > `pyproject.toml [tool.mollify]`). Declares: source roots, entry-point selectors, framework preset list, architecture preset, rule severities (`error` CI-fail / `warn` exit 0 / `off`), duplication thresholds, ignore/allowlists, baseline paths, cache dir.

**Output formats** (in `cli/report/`, mirroring fallow's 11): **human** (default), **JSON** (typed contract from `types`, with a discriminating top-level `kind`), **SARIF** (CI/code-scanning), compact, markdown, CodeClimate (gitlab-code-quality), plus the CI envelope formats **pr-comment-github / pr-comment-gitlab / review-github / review-gitlab** and **badge**. Severity model (`error`/`warn`/`off`) and the five co-equal analysis areas mirror fallow. **Suppression:** inline `# mollify-ignore-next-line <kind>`, `# mollify-ignore-file [kinds] -- <reason>`, docstring/comment markers (`@public`/`@internal`/`@expected-unused`), scoped policy tokens `<pack>/<rule-id>`, and declarative rule packs (`banned-call`/`banned-import`).

**MCP server** (stdio, `mcp` crate): fallow exposes **25 tools**, and Mollify mirrors the shape — analysis (`analyze`, `check_changed`, `audit`, `dead_code`, `find_dupes`, `check_health`), tracing (`trace_export`/`trace_file`/`trace_dependency`/`trace_clone`), the bundled `inspect_target` (file-scoped trace + dupes + complexity + candidates + impact closure), `fix_preview`/`fix_apply`, `project_info`, `decision_surface`, `explain`, `list_boundaries`, `security_candidates`, and the read-only `code_execute` sandbox; coverage-gated tools (`hot_paths`, `blast_radius`, `cleanup_candidates`) arrive with the runtime layer. Every JSON finding carries an `actions` array with an `auto_fixable` flag so the agent decides whether to call a fix tool. Typed contract version-pinned to the CLI.

**Agent skills:** a separate version-matched **`mollify-skills`** repo (distributed via `@mollify/*` npm + bundled), teaching agents which commands/flags to use and how to read output — supporting Claude Code, Cursor, Codex, Gemini CLI. This is the parity-plus move vs Skylos (no published skills repo, no version-pinned typed import).

**Editor/LSP** (`lsp` crate, tower-lsp-server + tokio): real-time diagnostics, hover, code actions, Code Lens with reference counts. VS Code, Zed, Neovim.

---

## 6. Distribution

- **cargo:** `cargo install mollify-cli` — the Rust-native path.
- **PyPI wheels (primary channel for a Python tool):** ship via **maturin + pyo3 (`pyext` crate)**, abi3 stable-ABI wheels for broad CPython compatibility. `pip install mollify` / `uvx mollify`. This is the channel that matters most — Python devs expect `pip`/`uv`.
- **npm:** scope `@mollify/cli` and `@mollify/skills` (bare `mollify` is taken), for agent-skill distribution and to match fallow's agent-install ergonomics.
- **GitHub Action:** `mollify-rs/mollify-action` running the audit with SARIF upload to code scanning + PR annotations; plus a GitLab CI template.

---

## 7. Differentiators Beyond Fallow

1. **Runtime coverage merge (cheaper & more credible than fallow's paid layer).** Merge production/test coverage (coverage.py `.coverage`/JSON, SlipCover) against the static reachability graph → three-state verdict: **static-dead** (Certain), **reachable-but-never-executed-in-prod** (cold path, strong delete candidate), **hot**. PEP 669 `sys.monitoring` (3.12+) drops overhead to ~5% (vs `settrace` ~2000%, coverage.py historically ~180%), so always-on production coverage is viable — JS cannot match this on cost. The killer, monetizable feature.
2. **Type-coverage / type-quality (no fallow analog — fallow excludes type findings).** Annotation coverage %, `Any`-leakage (params/returns/attrs typed or inferred `Any`), untyped-def %, `Any`-contamination propagation, per-module/PR **type-health score**. Either shell out to ty/pyrefly or embed scoring natively in Rust.
3. **Framework awareness as a first-class plugin system** — the false-positive killer that makes the tool trustworthy enough to act on.
4. **Notebooks (`.ipynb`):** native cross-cell name resolution, unused-cell/variable detection, execution-order hazards — underserved (nbQA only wraps existing tools).
5. **Supply-chain composite verdicts:** join deptry-style hygiene with pip-audit/OSV CVE data → "this dependency is **unused AND has a critical CVE** → delete it," a verdict neither tool produces alone. Plus bandit-style AST security + entropy secret detection folded into the same pass.

---

## 8. Phased Roadmap

### Phase 0 — Skeleton + Parser POC
- **Goals:** stand up the workspace; prove ruff-crate consumption; parse + walk a real Python repo.
- **Deliverables:** Cargo workspace with `config`/`types`/`parse`/`graph` stubs; ruff_python_parser pinned to a git rev; parse-and-visit POC over a mid-size repo with rayon; parsed-tree cache; criterion baseline.
- **Key risks:** ruff API churn on the pinned rev (mitigation: vendoring fallback ready); semantic-model reuse may be tighter-coupled to Salsa than hoped (mitigation: start by reusing only scope/binding primitives, hand-roll resolution).

### Phase 1 — MVP: Dead-code + Deps
- **Goals:** trustworthy, fast, reachability-first dead code + dependency hygiene with the core framework plugins — the trust foundation.
- **Deliverables:** import + symbol graph (stable IDs, flat edges, `__all__`/re-export propagation); mark-reachable engine; confidence-level model; entry-point/allowlist + selector config; core framework plugins (Django/FastAPI/Flask/Celery/pytest/setuptools entry points); dep-hygiene engine + import→distribution alias table; CLI `dead-code`/`deps`/`audit`/`init`; human + JSON + SARIF output; persistent cache; `.mollifyrc`; PyPI wheels + cargo + GitHub Action.
- **Key risks:** false positives from dynamism eroding trust (mitigation: conservative "assume-used" defaults + confidence tiers + framework plugins); import→distribution mapping long tail (mitigation: installed-metadata first, alias table for the rest); competing with Skylos's accuracy claims (mitigation: independent benchmark suite, lead with hard latency numbers).

### Phase 2 — Dup + Complexity + Arch
- **Goals:** complete the unified static audit; differentiate on semantic dupes and churn×complexity.
- **Deliverables:** suffix-array + LCP duplication engine with Python tokenizer and strict/mild/weak/**semantic** modes + fingerprinted families; cyclomatic + cognitive complexity + MI; **churn × complexity hotspot ranking**; architecture-boundary engine with named presets + symbol-level public interfaces; git-worktree audit incrementalism with introduced-vs-inherited attribution + NewOnly PR gate; `dupes`/`arch` commands; quality score 0–100; suppression + baselines; broader plugin set.
- **Key risks:** semantic-clone false positives (mitigation: tunable thresholds, default to mild); standalone dupes slower than jscpd (mitigation: position value as integration into the unified audit, not standalone speed); git-history cost on large repos (mitigation: cache churn data, bound history depth).

### Phase 3 — AI/MCP + Framework Plugins
- **Goals:** become the agent-native truth layer; widen framework coverage.
- **Deliverables:** MCP server (`inspect_target`, `security_candidates`) with `auto_fixable` actions; LSP (diagnostics, code actions, Code Lens, hover); version-matched `mollify-skills` repo (Claude Code/Cursor/Codex/Gemini CLI); LibCST-backed format-preserving `fix`; `trace`/`watch`; expanded plugin catalog (SQLAlchemy, Pydantic, click/typer, plugin registries).
- **Key risks:** LibCST autofix over-deletion (mitigation: only Certain-confidence findings auto-fixable, `--dry-run` default in agent flows); LSP latency demands Salsa sooner than planned (mitigation: tree-sitter fast-reparse layer as a stopgap); keeping skills version-pinned to the CLI contract (mitigation: CI gate on schema/skill drift).

### Phase 4 — Runtime / Type Intelligence (paid + brand)
- **Goals:** ship the monetizable cold-path deletion evidence and the novel type-health surface.
- **Deliverables:** `coverage` crate ingesting coverage.py/SlipCover + `sys.monitoring` normalization; three-state static/cold/hot verdicts; runtime-weighted scoring; offline Ed25519 JWT license + grace ladder; type-coverage / `Any`-leakage scoring (shell to ty/pyrefly or native); notebook analysis; async hazards; bandit-style security + secrets; supply-chain CVE join.
- **Key risks:** runtime layer requires customer instrumentation discipline (mitigation: SlipCover's ~5% overhead makes always-on credible; clear docs); type-checker dependency if shelling out (mitigation: pin ty/pyrefly versions, degrade gracefully); monetization boundary blurring OSS trust (mitigation: keep the entire static layer free/MIT, gate only runtime).

---

## 9. Risks & Open Questions

**Risks**
- **ruff-crate instability** is the single biggest technical risk: unpublished, no API-stability guarantee. Pin-a-rev now, vendor-fork if churn hurts. Track ty/pyrefly's own bumps as a signal.
- **Dynamism-driven false positives** are the product-trust risk; the confidence model + framework plugins are the entire defense. Ship conservative.
- **Skylos competition:** it is mature-ish, Apache-2.0, framework-aware, and broadening into a PR scanner. Our wedge is *focus* (reachability-first, not a kitchen-sink scanner) + a *real Rust core* + the *full agent contract*. Do not rest on Skylos's "only" framing — benchmark independently.
- **Benchmark credibility:** vendor-authored benchmarks (Skylos's, and ours) are suspect. Invest in a reproducible, third-party-runnable suite early.
- **Standalone dupes will lose to jscpd on raw speed** — frame value as unified-audit integration, not a dupes race.

**Open questions**
- Eager (pyrefly) vs Salsa (ty) — when exactly does the LSP force the Salsa migration, and can the two coexist behind one analysis API?
- Library-vs-application project-type inference: how reliably can we auto-detect it, given deletion safety for `__all__`/exports depends on it?
- Type intelligence: embed native type-quality scoring in Rust, or shell out to ty/pyrefly? Native is more work but removes a runtime dependency and fits the brand.
- Monetization line: is runtime coverage enough of a paid wedge, or does type-health belong in paid too? Keep static free regardless.
- How aggressively to maintain the import→distribution alias table — community-sourced, or owned? It is a durable moat either way.
- npm scope vs a renamed bare package: is `@mollify/cli` sufficient for agent-install ergonomics, or do we need a distinct unscoped npm name?

---

## 10. Implementation Orchestration (how we build it)

Mollify is built phase-by-phase with **many parallel agents under validation gates**, not 2–3 sequential ones. Three mechanisms divide the labor:

| Mechanism | Owns | Why |
|---|---|---|
| **`/goal`** | The per-phase convergence gate, e.g. `/goal "cargo test passes AND clippy is clean AND every new crate has tests"`. A fast evaluator checks the condition after **every turn** and stops only when truly met (bound it with `or stop after N turns`). | Guarantees a phase is *done*, not "looks done." |
| **`/loop`** (dynamic) | The iterate-until-green driver: `/loop "build, run tests, fix failures, repeat"` self-paces 1–60 min, faster while active. | Keeps the build-test-fix cycle turning without re-prompting; pairs with `/goal` as the stop condition. |
| **Workflow (ultracode)** | The parallel implementation fan-out — dozens of agents, structured outputs, worktree isolation, resume/journaling. | This is where "as many agents as needed" actually happens. |

**Hard limits that shape the design:** 16 concurrent agents max (more queue), 1000 per run; no mid-run user input (so phase sign-offs are separate workflows); **worktree isolation is opt-in** — any agent that *writes code* must run with `isolation: "worktree"` or parallel agents clobber each other; resume works in-session (edit script, re-run with `resumeFromRunId`, completed agents return cached).

**The safe implementation shape** — pipeline-per-crate with a verify gate baked in, not "spawn N coders and pray":

```
pipeline(crates,
  spec  → implement  (agent, isolation:"worktree")        // writes its crate in its own worktree
        → review      (FRESH-context reviewer vs PLAN.md + spec; schema verdict; finds gaps only)
        → fix-if-failed (re-runs only when review fails)
)
then: integrator agent merges worktrees → cargo test + clippy → reports
```

Layered with: **adversarial review** (reviewer sees only the diff + spec, told to refute), **hard test/clippy gates** (Rust concurrency/unsafe bugs that tests miss → `/code-review`), and **`/goal`** as the outer convergence gate after the workflow returns.

**Per-phase recipe:** (1) `/goal` sets the phase done-condition; (2) Workflow fans out one isolated worktree-agent per crate/module from the Phase's deliverables → fresh-context reviewer → fix loop → integrator; (3) `/loop` drives iterate-until-green if integration flakes; (4) gates (adversarial reviewers + `cargo test`/`clippy` + `/code-review`) before anything merges to `main`; (5) **start with one crate to calibrate token cost**, then scale the same script across all crates. Note: the real Workflow API is `agent(prompt, { isolation, schema, label, phase })` — model is inherited unless explicitly overridden.

---

## 11. Agent Integrations (full plan in INTEGRATIONS.md)

Like fallow ships `fallow-skills` + an MCP path, Mollify ships into **every major 2026 coding agent** — but broader. The model is **one MCP server, many front-ends**: build the `mollify mcp` server + the kind-discriminated JSON contract + one canonical `SKILL.md` once, then emit thin per-agent shims (a rule/memory file, an MCP registration block, and a command/workflow where supported) via `mollify init --agents`. Full copy-pasteable artifacts for each platform live in **`INTEGRATIONS.md`**.

Coverage: **Claude Code** (plugin/marketplace + Skill + PostToolUse/Stop gate hook + `.mcp.json` + slash commands), **OpenAI Codex** (`AGENTS.md` + `~/.codex/config.toml` MCP + `.agents/skills/` + hooks/notify), **Cursor** (`.cursor/rules/*.mdc` + `.cursor/mcp.json` + commands + native Skills 2.4+), **Gemini CLI** (`GEMINI.md` + `.gemini/settings.json` + TOML commands + skills), and a matrix for **Copilot / Cline / Aider / Continue**.

**Devin Desktop / Cascade is a featured, first-class target** (the org runs Cascade), built on the modern **`.devin/`** convention which bundles **skills + rules + hooks**, with **`.windsurf/`** for **workflows** (slash commands):
- **`.devin/skills/mollify/SKILL.md`** — the priority, future-facing artifact (also shipped to portable `.agents/skills/`). Vendor guidance prefers skills over rules; lazy-loaded, folder-bundled.
- **`.devin/rules/mollify.md`** — glob-triggered ("audit before PRs on `**/*.py`"), pointing at the skill; 12k/6k char limits, `trigger` modes.
- **Hooks (two systems):** Cascade IDE `.windsurf/hooks.json` (12 lowercase events e.g. `post_write_code`/`pre_run_command`, `command`/`show_output`, no matcher, `exit 2` blocks) **and** Devin CLI/Local `.devin/hooks.v1.json` (Claude-Code-compatible — the *same file* serves Claude Code). Ship both.
- **`.windsurf/workflows/{mollify-audit,mollify-cleanup,mollify-bootstrap}.md`** — `/slash`, manual-only, 12k char (mirror to the now-preferred `.devin/workflows/`).
- **MCP:** Cascade `~/.codeium/windsurf/mcp_config.json` (`${env:}`/`${file:}` interpolation, 100-tool cap) + committed `.devin/config.json` for Devin CLI/Local.

**ACP forward-path (Cascade available through July 2026 → Devin Local):** Mollify is an **MCP server**, and **ACP (agent↔editor) is orthogonal to MCP (agent↔tools)** — so the transition doesn't touch Mollify. Skills/rules/workflows/hooks + an MCP server carry forward with no rework (Devin Desktop keeps reading existing Windsurf config and retains MCP); Devin Local's sub-agents make a dedicated Mollify "audit" sub-agent (a `subagent: true` skill) *more* valuable. Authoritatively sourced from `docs.devin.ai/llms-full.txt` — see INTEGRATIONS.md §4/§6.

---

## 12. Positioning honesty (2026 currency pass — see RESEARCH.md §8)

A full re-verification against June-2026 releases retired three over-claims; the plan's wedges are reframed accordingly so we never market "first/only" where it's false:
- **Dead code:** skylos 4.25.0 *already* does whole-project framework-aware reachability (with a single confidence threshold). Our wedge is **determinism + tiered Certain/Likely/Uncertain verdicts with reasons + granular dead-members (enum members, `__all__`, properties, methods) + a focused product** — not "the only reachability tool." (Also: PyDeadCode is Rust too, so "genuinely Rust" alone isn't unique — pair it with determinism + ruff-AST + tiered confidence.)
- **Duplication:** lizard `-Eduplicate` *already* does token-normalized Type-2 for Python. Our wedge is the **SA-IS+LCP whole-corpus engine + clone families + integration + SARIF/MCP**, not Type-2 itself.
- **Architecture:** tach *already* does symbol-level public-interface enforcement (`interfaces[].expose`). Our net-new piece is **named presets (layered/hexagonal/feature-sliced/bulletproof) compiled to import-linter/tach contracts**; symbol-level enforcement is "tach parity, extended."

**Where the white space is clean and unchallenged (lead here):** FOSS Python **churn × complexity hotspot ranking**, **CRAP-style coverage-weighted risk**, the **"unused AND vulnerable" composite verdict**, and **a single deterministic Rust-cored audit** unifying all signals with tiered confidence, SARIF/MCP, and safe LibCST autofix. The "Python has no fallow" + Rust-foundation thesis fully holds; the marketing must be honest about which walls are already partly built.
