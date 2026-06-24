# Mollify — Research: Codebase Intelligence Tooling for Python (2026)

> **Source-verified.** The fallow sections below were checked against a full download of the real source tree (`fallow-rs/fallow` v2.102.0 — 791 Rust files, ~510k LOC, the 12 crates named in §2.1) via four parallel source-reading passes, not just the README. Corrections from that pass are folded inline and flagged. Net result: the architecture claims hold; the surface-area counts (commands, MCP tools, plugins, formats) were under-counted in the first draft and are corrected here.

## 1. Executive Summary

**The fallow model.** [fallow](https://github.com/fallow-rs/fallow) (`fallow-rs/fallow`, MIT, ~3.9k stars, version line 2.102.0, Rust edition 2024) is a Rust-native, deterministic codebase-intelligence engine for JavaScript and TypeScript. Its central thesis is uncompromising: **no AI invents findings — only deterministic, inspectable evidence.** It builds a module/symbol dependency graph from a fast *syntactic-only* parse (the [Oxc](https://github.com/oxc-project/oxc) suite, no type information), runs mark-reachable traversal from plugin-declared entry points, and surfaces dead code, duplication, circular dependencies, complexity hotspots, architecture-boundary violations, and dependency hygiene in a single unified pass. It ships as a CLI plus an LSP server, an MCP server, and a version-matched Agent Skill, with typed JSON contracts, `auto_fixable` action arrays, SARIF/Markdown/CodeClimate output, and git-worktree audit incrementalism that attributes findings as introduced-vs-inherited for PR gating. The static layer is free (MIT); a paid **Fallow Runtime** layer adds production coverage evidence (V8/Istanbul) for cold-path deletion, gated by an offline Ed25519 JWT license. The single most important fact about fallow: **it is exclusively TypeScript/JavaScript** ([verified](https://github.com/fallow-rs/fallow); the workspace contains a `napi` crate for Node bindings and a `v8-coverage` crate, with no Python parser). Python is its largest adjacent market, and fallow does not serve it.

**The Python ecosystem state.** Python has every *piece* fallow assembles, but they are fragmented across single-purpose tools, and the trustworthy/fast ones rarely overlap. Dead-code detection splits into three technique tiers: per-file AST scope analysis ([pyflakes](https://github.com/PyCQA/pyflakes), [ruff](https://docs.astral.sh/ruff/) `F401`/`F811`/`F841`/`ARG`), whole-project *name-table* matching ([vulture](https://github.com/jendrikseipp/vulture), [deadcode](https://github.com/albertas/deadcode)), and true entry-point reachability over a call/package graph ([skylos](https://github.com/duriantaco/skylos), and the pre-alpha Rust [Bury](https://github.com/neural-garage/tools)). Duplication is the weakest quadrant: [pylint R0801](https://pylint.readthedocs.io/en/latest/user_guide/messages/refactor/duplicate-code.html) is line-hash based, while [jscpd](https://github.com/kucherenko/jscpd), [PMD CPD](https://pmd.github.io/pmd/pmd_userdocs_cpd.html), Simian, and [lizard](https://github.com/terryyin/lizard) do exact-token (Type-1) detection — with no Python-wired renamed-variable (Type-2) semantic mode. Complexity is well served ([radon](https://radon.readthedocs.io/en/latest/intro.html), [complexipy](https://github.com/rohaquinlop/complexipy), ruff `C901`, lizard), but churn×complexity hotspot ranking has exactly one stagnant tool ([wily](https://github.com/tonybaloney/wily)). Architecture boundaries ([tach](https://github.com/tach-org/tach), [import-linter](https://import-linter.readthedocs.io/)/[grimp](https://github.com/python-grimp/grimp)) and dependency hygiene ([deptry](https://github.com/osprey-oss/deptry), [fawltydeps](https://github.com/tweag/FawltyDeps), [pip-audit](https://github.com/pypa/pip-audit)) are mature but siloed from each other and from dead code.

**The white space.** No Python tool ties module graph + dead code + duplication + complexity + dependency hygiene + architecture into one deterministic pass with stable IDs, ships a framework-plugin system as pure data, offers git-worktree audit incrementalism with new-vs-existing attribution, and exposes an MCP server + LSP + SARIF for agent/CI consumption. The closest competitor, Skylos, has the right architecture (entry-point reachability + framework awareness) but has diluted into a general multi-language PR/security scanner where dead code is the default mode but one feature among many. The only genuinely fallow-shaped Rust Python tool, Bury, is pre-alpha (0 stars, ~15 commits, no autofix, stale since December 2025).

**Python's dynamism and its compensating advantages.** Python's dynamic features (`getattr`, `importlib`, `__all__`, decorators, metaclasses, the Django app registry, pytest fixtures, entry points) make pure-syntactic reachability harder than in JS — which means *framework plugins matter more* and a conservative confidence-tiered "assume-used" policy is essential. But Python also offers compensating advantages fallow cannot match: standardized packaging entry points (`pyproject.toml` `[project.scripts]`/`[project.entry-points]`) are a richer, more canonical entry-point source than JS; and runtime coverage is dramatically cheaper via [PEP 669 `sys.monitoring`](https://peps.python.org/pep-0669/) (~5% overhead vs `sys.settrace`'s ~2000%), making cold-path deletion evidence — fallow's *paid* differentiator — far more credible in Python.

**The strategy.** Mollify's opening is "fallow for Python, with a real Rust core." Build on Astral's `ruff_python_parser` + `ruff_python_ast` (the de-facto Rust foundation used by both [ty](https://github.com/astral-sh/ty) and [pyrefly](https://github.com/facebook/pyrefly)), port fallow's suffix-array clone engine and stable-ID flat-edge graph, lead with confidence-tiered framework-aware reachability, and differentiate on a genuinely Rust-fast engine (Skylos is Python + Tree-sitter + optional LLM) plus runtime-coverage cold-path evidence and type-quality scoring that have no JS analog. The name `mollify` is clean on PyPI and crates.io and extends fallow's gentle-cultivation family ("soothe your Python").

---

## 2. What fallow Does and How It's Built

fallow is the reference architecture Mollify ports to Python. Its design choices are documented as ADRs and verifiable in the source ([repo](https://github.com/fallow-rs/fallow), MIT).

### 2.1 Workspace / crate decomposition

fallow is a Cargo workspace with `members = ["crates/*"]` (a glob), all sharing version 2.102.0 ([Cargo.toml](https://raw.githubusercontent.com/fallow-rs/fallow/main/Cargo.toml)). The [crates tree](https://github.com/fallow-rs/fallow/tree/main/crates) contains exactly twelve members (*verified*):

| Crate | Responsibility |
|---|---|
| `config` | Parses `.fallowrc.json`/`.fallowrc.jsonc`/`fallow.toml`/`.fallow.toml` (in that precedence), framework presets, rule packs, `package.json`, workspace discovery. |
| `types` | Shared serde data structures — the serialization contract crate. |
| `extract` | AST extraction engine: `visitor.rs`, `complexity.rs` (cyclomatic/cognitive), framework dialect parsers (`sfc.rs`/`astro.rs`/`mdx.rs`/`css.rs`), `cache.rs`, `suppress.rs`. |
| `graph` | Module dependency graph; resolves imports with `oxc_resolver`; `project.rs` holds project state. Reachability BFS lives in `graph/reachability.rs`. |
| `core` | Analysis orchestration: `analyze/` (dead code), `plugins/`, `duplicates/`. |
| `cli` | Per-command modules (`audit.rs`, `check.rs`, `dupes.rs`, `watch.rs`, `fix/`, `init.rs`), `license/`, `coverage/`, `report/`. |
| `lsp` | `tower-lsp-server`-based diagnostics, code actions, code lens, hover. |
| `mcp` | MCP server wrapping the CLI over stdio. |
| `napi` | Node.js native bindings (`@fallow-cli/fallow-node`). |
| `license` | Offline Ed25519 JWT verification; 7/30/hard-fail grace ladder. |
| `v8-coverage` | Parses V8 ScriptCoverage, normalizes to Istanbul (paid runtime layer). |
| `benchmarks` | Criterion microbenchmarks + comparative wall-clock harness. |

> **Note for Mollify:** `napi` and `v8-coverage` confirm fallow's JS/TS orientation. A Python analog drops these and substitutes a Python parser crate and a `coverage`/`sys.monitoring` ingestion crate.

### 2.2 Parser — syntactic only, no types

fallow uses the **Oxc suite v0.126** natively in Rust (not via NAPI): `oxc_allocator`, `oxc_ast`, `oxc_ast_visit`, `oxc_parser`, `oxc_resolver` (v11), `oxc_semantic`, `oxc_span`, `oxc_str`, `oxc_syntax`; CSS via [lightningcss](https://github.com/parcel-bundler/lightningcss) (v1.0.0-alpha.71, pinned). Parsing is **syntactic only — no type information** (*verified*: the README states "syntactic analysis — no type information"; `Cargo.toml` pins `oxc_parser`/`oxc_semantic` at `0.126` and `oxc_resolver` at `11`). This is the deliberate speed/determinism trade-off: it explicitly excludes type-narrowing and conditional-type reachability, in exchange for sub-second runs and no `tsc`/Node dependency.

### 2.3 Pipeline and graph design

**Config → File Discovery (`ignore`/`globset`) → Incremental Parallel Parsing (rayon + Oxc, cache-aware) → Script Analysis → Module Resolution (`oxc_resolver`) → Graph Construction → Re-export Chain Resolution → Dead-Code Detection → Reporting.**

Graph/architecture ADRs (*verified* in `CLAUDE.md` — there are **six**, not three):
- **ADR-001 no TypeScript compiler** — syntactic analysis via Oxc + `oxc_semantic`; no type resolution, no `tsc`.
- **ADR-002 flat edge storage** — a contiguous `Vec<Edge>` with range indices instead of pointer-based adjacency lists (cache-friendly traversal).
- **ADR-003 FxHashMap/FxHashSet required** — no `std` `HashMap` (deterministic iteration + speed).
- **ADR-004 path-sorted FileIds** — stable cross-run node identity independent of insertion order (determinism).
- **ADR-005 re-export propagation** — barrel/re-export chains resolved iteratively with Tarjan-SCC cycle detection (`run_re_export_fixpoint()` with a safety cap).
- **ADR-006 hidden-directory allowlist** — controls which dotted dirs are traversed.

### 2.4 Dead-code reachability

Mark-reachable traversal from entry points over the dependency graph; exports not reached are unused (*partly verified*: the mechanism is real, but the `mark_reachable` BFS lives in `crates/graph/src/graph/reachability.rs`, not `predicates.rs` as originally attributed). Issue types: unused files, unused exports, unused deps, unused class/enum members, unresolved imports, unlisted deps, duplicate exports, circular deps. Entry points come from plugin `entry_patterns`.

### 2.5 Duplication — suffix array + LCP

The `core/duplicates/` module implements **suffix array + LCP based clone detection** (*verified in source*). The suffix array is built with **SA-IS** (linear-time induced sorting, `detect/suffix_array.rs`) and the LCP array with **Kasai's algorithm** (`detect/lcp.rs`), so the engine is genuinely sub-quadratic (no pairwise comparison); ranking maps u64 token hashes to dense u32 ranks to keep the alphabet small. Pipeline: `tokenize_corpus_for_duplicates()` (`tokenize_file()` / `tokenize_file_cross_language()`) → `normalize_and_hash_resolved()` → `CloneDetector::detect_with_totals()` builds SA + LCP and extracts maximal repeated intervals → `families::group_into_families()` clusters by identical file-set → `detect_mirrored_directories()` → suppression + `min_occurrences` filtering → deterministic sort. **Cross-file safety** is enforced with `TokenKind::Boundary` sentinels so clones never span file boundaries (`detect/boundary.rs`). **Fingerprints** are `dup:<8hex>` from `xxh3_64` of a representative fragment, widening to 64-bit on collision (`deepdive.rs`). The mode enum is **`DetectionMode`** (in `config/duplicates_config.rs`), four variants: **Strict** (Type-1 exact), **Mild** (default — for AST tokenization it largely equals Strict, i.e. preserves values), **Weak** (`ignore_string_values`), **Semantic** (`ignore_identifiers` + strings + numbers / Type-2 renamed-variable). Thresholds: `min_tokens` (default 50), `min_lines` (default 5), `skip_local`, plus `min_occurrences` (default 2) and a percentage `threshold`.

### 2.6 Plugin system (~118 built-in plugins; README says 122)

**Count corrected:** the README markets "122 plugins" but the source instantiates **118** via a `push_plugins!` macro across five registration functions in `core/src/plugins/builtin.rs` (`add_framework_plugins` 24, `add_content_and_platform_plugins` 19, `add_build_and_test_plugins` 24, `add_quality_and_language_plugins` 20, `add_tooling_and_infra_plugins` 31). Plugins are `Box<dyn Plugin>` trait objects. The `Plugin` **trait** is static `&'static` defaults *plus* dynamic AST-driven resolution (*verified*). Each plugin declares: `entry_patterns()`/`entry_pattern_rules()`/`entry_point_role()`; `config_patterns()`/`resolve_config()` (parses config files via Oxc into dynamic facts)/`package_json_config_key()`; `used_exports()`/`used_export_rules()`; `path_aliases()`/`auto_imports()`. Dynamic resolution emits `PluginResult { entry_patterns, used_exports, referenced_dependencies, path_aliases, provided_dependencies, setup_files, fixture_patterns, used_class_members, always_used_files, replace_* flags, … }`. End-users can author custom framework plugins declaratively in JSONC/JSON/TOML with no code (schema queryable via `fallow plugin-schema`).

### 2.7 Command set and output contracts

Commands (*corrected from source* — the clap enum has ~29 subcommands, more than first listed): core analysis — `check` (canonical; alias **`dead-code`**), `dupes`, `health`, `audit` (alias **`review`**), `decision-surface`, `inspect`, `trace`, `impact` (enable/disable/status…), `security` (sub: `survivors`/`blind-spots`), `coverage` (paid; setup/analyze/upload…), `watch`, `fix`, `init`. Introspection/tooling — `flags`, `explain`, `schema` (MCP manifest), `list`, `workspaces`, `config`, `config-schema`/`plugin-schema`/`rule-pack-schema`, `ci`/`ci-template`, `migrate` (knip/jscpd import), `telemetry`, `hooks`/`setup-hooks`, `license`. Scoping flags worth noting for the port: `--changed-since`/`--base`, `--diff-file`/`--diff-stdin`, **`--churn-file`** (non-git VCS history — hg/perforce/arc — via a `fallow-churn/v1` JSON), `--gate NewOnly|All`, `--group-by owner|directory`, `--runtime-coverage` (spawns the paid `fallow-cov` sidecar). Output formats (`cli/report/`): **11** total — human, json, sarif, compact, markdown, codeclimate (aka gitlab-code-quality), **pr-comment-github, pr-comment-gitlab, review-github, review-gitlab, badge**; the `types` crate owns the serde contract and every envelope carries a discriminating top-level `kind`. Severity: `error` (CI fail, default) / `warn` / `off`. Suppression: `// fallow-ignore-next-line <kind>`, `// fallow-ignore-file [kinds] -- <reason>`, JSDoc `@public/@internal/@beta/@alpha/@expected-unused`, scoped policy tokens `<pack>/<rule-id>`, and declarative rule packs (`banned-call`/`banned-import`/`banned-effect`).

### 2.8 Caching and git-diff incrementalism

Persistent cache `.fallow/cache/` (audit base snapshots under `audit-base-v3/`); extraction AST cache `.fallow/cache.bin` with a 256 MB default (`DEFAULT_CACHE_MAX_SIZE`, LRU eviction); encoded with **bitcode v0.6** (*verified*: `Cargo.lock` pins `bitcode = "0.6.9"`). **Important scope nuance** (*from `ROADMAP.md`*): caching today is at the **extraction phase + audit base-snapshot** level only — **graph construction is not yet incremental**, and `watch` rebuilds the full graph on each change. The flat-edge/stable-FileId design (ADR-002/004) was chosen to *enable* future incremental graph work, but it is listed as ongoing, not shipped. Audit incrementalism (*verified* against [`audit.rs`](https://raw.githubusercontent.com/fallow-rs/fallow/main/crates/cli/src/audit.rs)): `git::get_changed_files()` + `auto_detect_base_ref()`; when gate = `NewOnly` and the current tree isn't reusable, it spins an isolated git worktree at the base via `BaseWorktree::create()`, runs the same passes, captures an `AuditKeySnapshot`. Cache key = **xxHash3-64** of (cache version, CLI version, base SHA, config hash, changed-files list, production settings, workspace config, baseline paths). Attribution: `AuditAttribution` + `count_introduced()` partition findings into introduced vs inherited; `compute_introduced_verdict()` gates only new issues. The base-snapshot cache is capped at 16 MiB (`MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE`).

### 2.9 AI/runtime split (free vs paid)

- **LSP** (tower-lsp-server v0.23 + tokio): real-time diagnostics, hover, code actions, Code Lens with reference counts (VS Code, Zed, Neovim).
- **MCP** (stdio): **25 tools** (*corrected — far more than the 3 first listed*), incl. `analyze`, `check_changed`, `audit`, `find_dupes`, `check_health`, `inspect_target` (bundles file-scoped trace + dupes + complexity + security candidates + impact closure), `security_candidates`, `trace_export`/`trace_file`/`trace_dependency`/`trace_clone`, `fix_preview`/`fix_apply`, `project_info`, `decision_surface`, `fallow_explain`, `list_boundaries`, `feature_flags`, `impact`/`impact_all`, `code_execute` (bounded read-only sandbox), plus coverage-gated `check_runtime_coverage`/`get_hot_paths`/`get_blast_radius`/`get_importance`/`get_cleanup_candidates`.
- **LSP**: `tower-lsp-server` 0.23 + tokio; diagnostics (`unused`/`security`/`quality`/`structural`), code actions (`quick_fix`/`suppress`), `code_lens` with reference counts, hover.
- **Agent Skill**: version-matched guidance shipped in npm.
- **Free/OSS (MIT):** all static analysis, CLI/LSP/MCP, all output formats, CI templates, ~118 plugins, config/suppression/baselines (*verified*).
- **Paid Fallow Runtime:** runtime coverage (V8 + Istanbul), `health --runtime-coverage`, hot-path/cold-code evidence, cloud aggregation, runtime-weighted scoring, gated by offline Ed25519 (EdDSA, algorithm-pinned) JWT with a **7-day warning → 30-day watermark → hard-fail** grace ladder. The paid `Feature` enum is exactly: `RuntimeCoverage`, `PortfolioDashboard`, `McpCloudTools`, `CrossRepoAggregation` (*verified* in `license/src/lib.rs`). Nuance: static *coverage-gap* analysis is free; only *runtime* coverage and some closed-source normalization are paid.

### 2.10 Performance

Dead-code vs knip v6 (M5, 10c/32GB): Preact 244f 74ms vs 2.01s (27×); Fastify 286f 64ms vs 205ms (3.2×); TanStack/query 901f 560ms vs 1.04s (1.9×); Svelte 3,337f 611ms vs 632ms (~equal); TypeScript 38,146f 2.22s vs 736ms (knip 3× faster on the very largest repo). Knip v5/v6 fail on next.js/vite/vue-core where fallow succeeds. Standalone dupes vs jscpd: jscpd is 1.4×–14.7× faster — fallow's dupe value is *integration into the unified audit*, not winning standalone. Note (*from `ROADMAP.md`*): parsing is parallel (rayon over files; Oxc itself is single-threaded), but **graph construction is single-threaded** today.

### 2.11 Load-bearing design principles (from `CLAUDE.md`/`CONTEXT.md`/`docs/`)

These are the invariants a faithful port must keep, not implementation trivia:
1. **Determinism is non-negotiable.** "No AI inside the analyzer." Identical input → byte-identical output across runs/platforms/CI. ADR-003 (FxHashMap, no `std` HashMap) and ADR-004 (path-sorted FileIds) exist to guarantee this; any randomness must be seeded.
2. **Candidate-producer vs. verifier separation.** fallow emits *evidence* (candidates, traces, metrics) and never decides the verdict (exploitability, fixability, deletion). `fix_preview` never auto-applies (`fix_apply` is explicit); `security` "does not call a model." Downstream agents/humans own judgement.
3. **Versioned output contract.** Every command emits a JSON envelope with a discriminating top-level `kind`; clients depend on the *JSON shape* (shipped JSON Schema), not on Rust struct stability. The programmatic API returns `serde_json::Value` matching the CLI contract.
4. **Five co-equal analysis areas.** Unused code · circular deps · duplication · complexity hotspots (incl. CRAP) · boundary violations. fallow's own guidance forbids reducing it to "a dead-code tool" — the discovery/parse phase is shared across all five.
5. **Evidence-preserving findings.** Each issue carries its full trace (import chain, reachability proof, reference counts) so a downstream tool can audit it.

---

## 3. The Python Ecosystem, Capability by Capability

### 3.1 Dead code / unused symbols

The Python toolchain splits into three technique tiers, and this split is the entire positioning story.

**Tier 1 — Per-file AST scope analysis.** One file at a time; zero cross-module knowledge. [pyflakes](https://github.com/PyCQA/pyflakes) (the canonical engine, "never emit false positives" by only flagging intra-file facts); [ruff](https://docs.astral.sh/ruff/) reimplements these as `F401`/`F811`/`F841`/`ARG`/`RUF100`. Ruff has **no whole-project/reachability rule** (*verified*: the canonical request [issue #872](https://github.com/astral-sh/ruff/issues/872) is still open; Astral puts reachability in the `ty` domain, not the linter; Ruff scores 62.67% on Skylos's dead-code suite, catching only the intra-file subset). Fixers: [autoflake](https://github.com/PyCQA/autoflake), [pycln](https://github.com/hadialqattan/pycln), [unimport](https://github.com/hakancelikdev/unimport); [flake8-eradicate](https://github.com/wemake-services/flake8-eradicate) flags commented-out code.

**Tier 2 — Whole-project name-table matching.** Parse all files, compute `defined_names − used_names`. Whole-project in *scope* but **not reachability**. [vulture](https://github.com/jendrikseipp/vulture) (the standard; confidence 60–100%; MIT; v2.16 Mar 2026) and [deadcode](https://github.com/albertas/deadcode) (AGPLv3, autofix, codes DC01–DC13). The defining defect (*verified*): a symbol referenced only by other dead code is counted as *used* (vulture's `core.py` builds `defined_*` collections and one `used_names` set, then takes the difference; deadcode documents the same name-keyed behavior). Dead cyclic clusters survive; public-but-externally-used symbols cause false positives.

**Tier 3 — Entry-point reachability over a call/package graph.** [skylos](https://github.com/duriantaco/skylos) (Apache-2.0, Python + tree-sitter, v4.25.0 Jun 2026): infers entrypoints, follows reachability, is framework-aware (FastAPI/Django/Flask/pytest/SQLAlchemy), configurable entrypoint selectors (`name`/`decorators`/`base_classes`/`parent`) in `pyproject.toml`, autofix, JSON, `--diff` PR gating, MCP `verify_change`. *Verified* but with caveats: the "only mature mainstream" framing is marketing-grade (vulture clearly doesn't do this, but uniqueness is unprovable), benchmarks are vendor-authored, and Skylos has **expanded into a general PR/security scanner** (*verified*: security/secrets/CVE/AI-code-mistake checks behind `-a`; dead code remains the *default* mode but is one feature among many). [Bury](https://github.com/neural-garage/tools) (Rust + tree-sitter, MIT/Apache) is a genuine fallow-shaped reachability detector for Python — but **pre-alpha** (*verified*: 0 stars, 15 commits, no releases, no crate, stale since Dec 2025, no autofix). The Rust type-checkers ty/pyrefly build whole-project graphs but **do not ship dead-code/reachability reporting** today (*verified*: ty's blog frames it as future "will power" work; pyrefly's only report command covers type-annotation coverage, not reachability).

| Tool | Lang | License | What it does | Key limitation |
|---|---|---|---|---|
| [pyflakes](https://github.com/PyCQA/pyflakes) | Python | MIT | Per-file unused imports/locals/redefinitions | Per-file only; no cross-module knowledge |
| [ruff](https://docs.astral.sh/ruff/) (F401/F811/F841) | Rust | MIT | Fast per-file pyflakes reimplementation + autofix | No whole-project/reachability rule (62.67% on Skylos suite) |
| [vulture](https://github.com/jendrikseipp/vulture) | Python | MIT | Whole-project name-table dead code | Name-matching, not reachability; no graph/entry points; no JSON/autofix |
| [deadcode](https://github.com/albertas/deadcode) | Python | AGPLv3 | Vulture-style name-table + autofix | Same reachability blind spot; AGPL blocks adoption; no JSON |
| [skylos](https://github.com/duriantaco/skylos) | Python (tree-sitter) | Apache-2.0 | Reachability + call graph, framework-aware, MCP | Scope diluted into security/PR scanning; vendor benchmarks |
| [Bury](https://github.com/neural-garage/tools) | Rust (tree-sitter) | MIT/Apache | Rust reachability (Python+TS) | Pre-alpha; no releases/crate/autofix; stale |
| [ty](https://github.com/astral-sh/ty) / [pyrefly](https://github.com/facebook/pyrefly) | Rust | MIT(/Apache) | Whole-project type checkers | Not dead-code tools; no reachability reporting today |

### 3.2 Duplication

The clearest gap. [pylint R0801](https://pylint.readthedocs.io/en/latest/user_guide/messages/refactor/duplicate-code.html) is line-hash based (cannot see renamed variables; documented false positives; `min-similarity-lines=0` doesn't disable it). [jscpd](https://github.com/kucherenko/jscpd) (Rust v5, 24–37× faster than the old TS engine, 223 formats, SARIF, MCP) is the fast cross-language CPD but **Type-1/near-exact only**. [PMD CPD](https://pmd.github.io/pmd/pmd_userdocs_cpd.html) supports Type-2 via `--ignore-identifiers` — but that anonymization is documented Java/C++ only and **not wired up for Python** (*correction to original framing: PMD CPD is Type-2-capable, just not for Python*). Simian (commercial) has renamed-variable detection for Java/C only. [lizard](https://github.com/terryyin/lizard)'s `-Eduplicate` is basic Type-1. **No actively-maintained mainstream tool does suffix-array + Type-2 semantic clones for Python** (*partly verified*: the precise conjunction holds, but renamed-variable detection ships in mainstream CLIs for *other* languages, and the cited fallow source is JS/TS, not Python). AST-based Python semantic detectors exist (DeepCSIM, duplicate-logic-detector-action) but are not suffix-array engines.

| Tool | Lang | License | What it does | Key limitation |
|---|---|---|---|---|
| [pylint R0801](https://pylint.readthedocs.io/en/latest/user_guide/messages/refactor/duplicate-code.html) | Python | GPL-2.0 | Line-similarity duplicate blocks | Line-hash, blind to Type-2; slow; many FPs |
| [jscpd](https://github.com/kucherenko/jscpd) | Rust | MIT | Rabin-Karp token CPD, Python among 223 formats | Type-1 only; no semantic mode |
| [PMD CPD](https://pmd.github.io/pmd/pmd_userdocs_cpd.html) | Java | BSD-style | Token/suffix-tree CPD; ignore-identifiers (Java/C++) | Type-2 anonymization not available for Python; JVM startup |
| Simian | Java | Commercial | Token similarity | Closed; renamed mode Java/C only |
| [lizard](https://github.com/terryyin/lizard) (-Eduplicate) | Python | MIT | Token copy-paste | Basic Type-1; secondary to CCN |

### 3.3 Complexity and churn

Well served on metrics. [radon](https://radon.readthedocs.io/en/latest/intro.html) (cyclomatic, Halstead, Maintainability Index; pure-Python, no churn); [xenon](https://github.com/rubik/xenon) (CI gate over radon); [ruff `C901`](https://docs.astral.sh/ruff/rules/complex-structure/) (cyclomatic, Rust-fast); [complexipy](https://github.com/rohaquinlop/complexipy) (Rust, cognitive complexity, `--diff`/`--ratchet`, SARIF; v5.6.1); [lizard](https://github.com/terryyin/lizard) (multi-language CCN); [SonarQube/sonar-python](https://github.com/SonarSource/sonar-python) (cognitive S3776). The churn×complexity niche has **one tool**, [wily](https://github.com/tonybaloney/wily), which walks git history running radon per revision — but reports per-revision *trends*, not a combined "high-complexity × high-churn = refactor priority" hotspot score, and is low-activity.

| Tool | Lang | License | What it does | Key limitation |
|---|---|---|---|---|
| [radon](https://radon.readthedocs.io/en/latest/intro.html) | Python | MIT | Cyclomatic + Halstead + MI | No churn, no cognitive; pure-Python (slow) |
| [ruff C901](https://docs.astral.sh/ruff/rules/complex-structure/) | Rust | MIT | Cyclomatic gate | Cyclomatic only; no MI/cognitive/churn |
| [complexipy](https://github.com/rohaquinlop/complexipy) | Rust | MIT | Cognitive complexity, diff/ratchet, SARIF | Cognitive only; ratchet is direction not churn×complexity |
| [wily](https://github.com/tonybaloney/wily) | Python | Apache-2.0 | Git-history complexity trends | Trend, not hotspot priority score; low activity |
| [SonarQube/sonar-python](https://github.com/SonarSource/sonar-python) | Java | LGPL/commercial | Cognitive + cyclomatic + duplication density | Server-heavy; history is DB not git-churn |

### 3.4 Architecture and boundaries

Two distinct graphs must not be conflated: the **internal module graph** (first-party modules; `import` edges) and the **distribution dependency graph** (PyPI distributions; the hard part is mapping `import cv2 → opencv-python`). [tach](https://github.com/tach-org/tach) (Rust, MIT) is the closest fallow-preset analog: per-module `depends_on`, **symbol-level public interfaces** (stronger than edge-only tools), ordered `layers`, cycle detection, first-class monorepo `source_roots`. [import-linter](https://import-linter.readthedocs.io/) (BSD, engine [grimp](https://github.com/python-grimp/grimp), Rust-accelerated ~6×): forbidden/independence/layers (with containers)/custom contracts; dynamic imports via `ignore_imports` allowlist. Neither ships named, opinionated presets (bulletproof/layered/hexagonal/feature-sliced) — a clear differentiator.

| Tool | Lang | License | What it does | Key limitation |
|---|---|---|---|---|
| [tach](https://github.com/tach-org/tach) | Rust | MIT | Module boundaries, symbol-level interfaces, layers, cycles | Static (blind to dynamic imports); external-dep checking less mature |
| [import-linter](https://import-linter.readthedocs.io/) | Python (grimp=Rust) | BSD-2 | forbidden/independence/layers/custom contracts | Edge-only (no symbol visibility); manual ignore_imports |
| [grimp](https://github.com/python-grimp/grimp) | Rust+Python | BSD-2 | Queryable import graph library | Library only; static-import blindness |
| [pydeps](https://github.com/thebjorn/pydeps) | Python | BSD-2 | Import-graph visualization | Viz not enforcement; needs Graphviz |

### 3.5 Dependency hygiene

[deptry](https://github.com/osprey-oss/deptry) (Rust core via `ruff_python_ast`, MIT) covers the full matrix: DEP001 missing / DEP002 unused / DEP003 transitive / DEP004 misplaced-dev / DEP005 stdlib-listed; supports Poetry/PDM/uv/PEP 621/requirements. [fawltydeps](https://github.com/tweag/FawltyDeps) (Python, BSD) adds Jupyter coverage but is weaker on transitive/dev splits. [pip-audit](https://github.com/pypa/pip-audit) (Apache-2.0, PyPA/Trail of Bits) does CVEs via OSV + PyPA Advisory DB, emits CycloneDX SBOM, `--fix`. [pipdeptree](https://github.com/tox-dev/pipdeptree), [pip-tools](https://github.com/jazzband/pip-tools) (increasingly displaced by uv) round it out.

| Tool | Lang | License | What it does | Key limitation |
|---|---|---|---|---|
| [deptry](https://github.com/osprey-oss/deptry) | Rust+Python | MIT | unused/missing/transitive/dev-split deps | Needs metadata/alias for import→dist; no security/layering |
| [fawltydeps](https://github.com/tweag/FawltyDeps) | Python | BSD-3 | undeclared/unused deps + notebooks | Weaker on transitive/dev; dynamic imports missed |
| [pip-audit](https://github.com/pypa/pip-audit) | Python | Apache-2.0 | CVE scan (OSV + PyPA), SBOM, --fix | Security only; misses bundled non-Python libs |

### 3.6 AI / agent positioning

[fallow](https://github.com/fallow-rs/fallow) sets the bar: typed `import type { CheckOutput }` contracts, `auto_fixable` actions, MCP (`inspect_target`/`code_execute`), LSP, and a separate version-matched [fallow-skills](https://github.com/fallow-rs/fallow-skills) repo for 30+ agents — but TS/JS only. [Skylos](https://github.com/duriantaco/skylos) is the closest Python competitor (MCP `verify_change`, agent commands, AI-code-mistake detection) but its core is Python + tree-sitter + optional LLM, with no version-pinned typed-types import or separate skills repo. [Sourcery](https://www.sourcery.ai/) and [Qodo](https://dev.to/rahulxsingh/qodo-vs-sourcery-ai-code-review-approaches-compared-2026-a6b) are LLM-judge reviewers (non-deterministic). [SonarQube Agentic Analysis + MCP](https://www.sonarsource.com/products/sonarqube/mcp-server/) and [Semgrep MCP](https://github.com/VetCoders/mcp-server-semgrep) are rule-based but enterprise-heavy / security-first.

| Tool | Lang | License | What it does | Key limitation |
|---|---|---|---|---|
| [fallow](https://github.com/fallow-rs/fallow) | Rust | MIT + paid runtime | Deterministic TS/JS truth layer; typed contracts, MCP, LSP, skills | TS/JS only; runtime layer paid |
| [Skylos](https://github.com/duriantaco/skylos) | Python+tree-sitter(+LLM) | Apache-2.0 | Multi-language PR scanner, MCP, AI-mistake detection | Not Rust core; LLM step adds non-determinism; no skills repo |
| [Sourcery](https://www.sourcery.ai/) | Python | Proprietary | AI PR review + IDE refactor | LLM-driven; not a deterministic truth layer |
| [SonarQube Agentic](https://www.sonarsource.com/products/sonarqube/mcp-server/) | Java/multi | Commercial | Rule-based analysis in the agent loop via MCP | Enterprise-priced, heavyweight, not local-first |
| [Semgrep](https://github.com/VetCoders/mcp-server-semgrep) | OCaml/Python | LGPL + commercial | SAST + MCP | Security-first; weak on dead code/structure |

---

## 4. The Hard Parts of Python Static Analysis

Dead-code detection in Python is **provably undecidable** in the general case ([analysis](https://dev.to/sendotltd/dead-code-in-python-is-undecidable-so-i-built-a-detector-that-admits-it-1k8j)); any tool claiming certainty is wrong on some inputs. The correct design is **confidence-tiered reachability**, not boolean dead/alive. Vulture's flat scheme (100% unreachable-after-return, 90% imports, 60% for most symbols — and it "ignores scopes") is the floor to beat. The research-grade [PyCG](https://github.com/vitsalis/PyCG) call graph hits 99.2% precision but only ~69.9% recall and doesn't scale.

Dynamic constructs and recommended handling with confidence levels:

| Construct | Soundest handling | Confidence |
|---|---|---|
| `getattr`/`setattr`/`__getattr__` | String-literal name → resolve precisely; dynamic expr → mark all same-named attrs *possibly-reached*; a class defining `__getattr__` → suppress unused-attribute findings on it | LOW (dynamic) / HIGH (suppression) |
| `importlib.import_module`/`__import__` | String-literal → real module edge; computed (INSTALLED_APPS, Celery, pytest plugins, entry points) → seed from config/conventions, not AST | MEDIUM/seeded |
| `eval`/`exec` | Module with `exec`/`eval` of non-literal code → downgrade confidence for scope symbols (reachability sink) | LOW |
| decorators | Framework registration (route/task/fixture/CLI/signal) → reached with zero in-repo callers; needs a built-in decorator registry | HIGH (with plugin) |
| metaclasses / `__init_subclass__` / descriptors | Classes with registering metaclasses → treat as roots | MEDIUM |
| `__all__` | Names are public-API roots (library); for applications they aren't — deletion safety is project-type-dependent | HIGH |
| monkeypatching (`module.attr = x`) | Mark target name reachable (reachability write) | MEDIUM |
| PEP 420 namespace packages | Infer membership from directory structure, not `__init__.py` | structural |
| conditional imports (`try/except ImportError`, `TYPE_CHECKING`) | Both branches reachable; `TYPE_CHECKING` = type-only roots | HIGH |
| `from m import *` | Expand against `m.__all__` (or all public names) or downstream resolution is unsound | required |

**Recommended scheme:** HIGH (syntactically unreachable code, unused params, locals with no `exec` in scope) → safe to auto-fix; MEDIUM (module-private, scope-tracked, no dynamic sinks) → suggest; LOW (public name, near getattr/eval, framework-adjacent) → report only. Every "unused" verdict carries a confidence + a reason. **Framework entry-point awareness is the dominant false-positive killer** — the reason engineers abandon Python dead-code tools (FastAPI routes, SQLAlchemy hooks, Pydantic validators, Django registries/migrations/admin/signals, Celery tasks, pytest fixtures, click/typer CLIs, setuptools `entry_points`). Universal weakness across *all* boundary/dep tools: dynamic-import handling punts to ignore-lists; smarter `TYPE_CHECKING`/entry-point/plugin-registry resolution is an opening, as is a maintained import-name→distribution-name table (the `cv2`→`opencv-python` problem).

---

## 5. Rust Foundation Options for a Python Analyzer

The Rust ecosystem has converged on a single de-facto foundation: **Astral's `ruff_python_parser` + `ruff_python_ast`**. Both 2025–2026 Rust type checkers — Astral's [ty](https://github.com/astral-sh/ty) and Meta's [pyrefly](https://github.com/facebook/pyrefly) — use this exact parser. The decision is not *which parser* but *how to consume it* and *which architecture to copy*.

| Crate | License | Role | Note |
|---|---|---|---|
| [`ruff_python_parser`](https://github.com/astral-sh/ruff/tree/main/crates/ruff_python_parser) | MIT | Hand-written recursive-descent (Pratt) parser; error-resilient, full source ranges | **Not on crates.io; no API-stability guarantee.** Lossy AST. |
| [`ruff_python_ast`](https://github.com/astral-sh/ruff/tree/main/crates/ruff_python_ast) | MIT | Typed AST nodes + visitors | Reused by pyrefly; same unpublished status |
| [`ruff_python_semantic`](https://github.com/astral-sh/ruff/tree/main/crates/ruff_python_semantic) | MIT | Scopes, bindings, symbol resolution, module resolver | Module resolver coupled to Salsa in ty path |
| [`ruff_db`](https://github.com/astral-sh/ruff/tree/main/crates/ruff_db) | MIT | Salsa-backed VFS + query DB | Reference impl for LSP-grade incrementality |
| [`salsa`](https://github.com/salsa-rs/salsa) | MIT/Apache | On-demand incremental computation | Powers ty; significant complexity |
| [tree-sitter-python](https://github.com/tree-sitter/tree-sitter-python) | MIT | Incremental GLR CST | Complement for editor re-parse latency; no semantic model |
| [libcst (Rust)](https://crates.io/crates/libcst) | MIT AND (MIT AND PSF-2.0) | Lossless CST | Only if shipping format-preserving autofixes |
| [pyo3](https://github.com/PyO3/pyo3) | MIT/Apache | Rust↔CPython bindings | Only for Python-side fallback / wheel packaging |
| [rustpython-parser](https://github.com/RustPython/Parser) | MIT | Older parser | **Deprecated** — its README points to ruff; do not use |

**Why ruff wins:** Meta's pyrefly depends on the ruff crates directly via a pinned git revision (`git = "https://github.com/astral-sh/ruff/", rev = <commit>`), proving third-party reusability and establishing the consumption pattern. The critical caveat: Astral has **not** published these crates and offers **no API-stability guarantee** ([issues #10417](https://github.com/astral-sh/ruff/issues/10417), [#17970](https://github.com/astral-sh/ruff/issues/17970)). The unofficial crates.io forks are not Astral releases — avoid them for production.

**Two reference architectures.** [ty/red-knot](https://github.com/astral-sh/ty) is Salsa-based, lazy, query-driven, fine-grained incremental — LSP-optimal but complex; cross-module inference must funnel through cached Type-level queries. [pyrefly](https://github.com/facebook/pyrefly) is eager, rayon-parallel, module-level incremental, no Salsa — ~1.8M LOC/sec, full Instagram codebase in 13.4s; predictable memory; simpler; CI/throughput-optimal but coarser (editing a function re-checks the whole module + dependents).

**Recommendation for Mollify.** Build on `ruff_python_parser` + `ruff_python_ast` + `ruff_text_size` (MIT), consumed via a **pinned git revision** (pyrefly's proven pattern), with vendoring as the fallback. Reuse `ruff_python_semantic`'s scope/binding model and module resolver as the starting point for symbol resolution and the import graph. For the engine architecture: **start with pyrefly's eager + rayon, module-level-incremental model** (Mollify is primarily a CLI/CI analyzer); **add ty's Salsa query model later** if/when an LSP/watch mode needs keystroke-latency re-analysis. Add tree-sitter-python only as an optional fast-reparse layer, and Rust `libcst` only for byte-faithful autofixes. The genuinely missing, possibly-publishable piece nobody has shipped: a standalone Python module-graph/import-resolver crate.

---

## 6. Gaps and White Space: Where Mollify Wins

1. **The integrated deterministic single pass.** No Python tool ties module graph + dead code + duplication + complexity + dep hygiene + architecture into one deterministic pass with stable IDs. Mollify is the single Rust binary that does layering + independence + forbidden + unused/missing/transitive/dev-split + cycle detection + dupes + hotspots — fallow's exact menu, ported.

2. **Reachability done right, with granular dead members.** Only Skylos (diluted) and Bury (pre-alpha) do entry-point reachability for Python. Mollify can be the *focused* reachability tool — vulture-trusted but graph-correct, ruff-fast but whole-project, Skylos-accurate but not sprawling — and surface what name-table tools miss cleanly: dead files, unused `__all__` entries, enum members, class methods, properties.

3. **Semantic (Type-2) clone detection — the emptiest quadrant.** A Rust-fast, Python-AST-aware suffix-array detector with strict/mild/weak/semantic modes (fallow's engine ports directly; only the tokenizer/normalizer needs Python-specific work for significant whitespace and identifier/literal blinding) would be net-new for Python. Add fingerprinted, ownership-aware (CODEOWNERS) clone families.

4. **Churn × complexity hotspots.** Wily reports trends, not a combined high-complexity-AND-high-churn refactor-priority score. A first-class hotspot ranking is unfilled.

5. **Named architecture presets.** import-linter/tach give primitives; nobody ships bulletproof/layered/hexagonal/feature-sliced templates. Mapping: `layered`→layers; `feature-sliced`→independence; `hexagonal`→forbidden + tach symbol interfaces; `bulletproof`→layers + forbidden + no-cycles.

6. **Composite cross-signal verdicts no single tool produces.** "Unused *and* CVE-vulnerable dependency → delete" (deptry + pip-audit); "test-only and never-executed-in-prod → delete with the test" (reachability + coverage); "import-side-effect-free → safe to delete."

7. **Runtime-coverage cold-path evidence — cheaper than JS.** Fallow's *paid* killer feature is 30–40× cheaper in Python via [PEP 669 `sys.monitoring`](https://peps.python.org/pep-0669/) (~5% overhead) and [SlipCover](https://github.com/plasma-umass/slipcover) (~5% vs coverage.py's ~180%). Three-state verdicts: statically-dead (HIGH) / reachable-but-never-executed (cold, strong delete candidate) / hot. Turns "probably dead" into "executed zero times across N production days."

8. **Type-coverage / Any-leakage scoring — no JS analog at all.** Fallow explicitly excludes type-checker findings. A "type health score" (% annotated defs, `Any` leakage/contamination, untyped defs) is uniquely Pythonic and fits the "intelligence" brand; ride ty/pyrefly or embed natively.

9. **A genuinely Rust core vs Skylos.** Skylos is Python + tree-sitter + optional LLM; a true Rust engine (the Ruff playbook: ~100× over Flake8) is a measurable, hard-to-copy wedge — lead with hard latency numbers.

10. **The full agent contract.** Ship `mollify/types`-equivalent version-pinned typed contracts + an `auto_fixable` actions array + a separate `mollify-skills` repo (Claude Code/Cursor/Codex/Gemini CLI) — the parity-plus move Skylos lacks. Avoid the crowded SAST middle (Semgrep/Sonar own security); win on structural truth + safe LibCST-style autofix.

**Licensing/naming moat:** MIT/Apache to beat AGPL deadcode; `mollify` is available on PyPI and crates.io (use `@mollify/*` on npm; grab GitHub org `mollify`/`mollify-rs`). The name extends fallow's gentle-cultivation family ("soothe your Python").

---

## 7. Appendix: Verified Claims

| # | Claim | Verdict | Notes |
|---|---|---|---|
| 1 | fallow is a Cargo workspace of 12 crates (config, types, extract, graph, core, cli, lsp, mcp, napi, license, v8-coverage, benchmarks) | **Confirmed** | Verified via `Cargo.toml` (`members = ["crates/*"]`) and the crates tree. Caveat: fallow is TS/JS, not Python; `napi`/`v8-coverage` confirm JS orientation. Count is a glob snapshot. |
| 2 | Parsing is purely syntactic via Oxc (no types); README says so; Cargo.toml pins oxc_parser/semantic v0.126, resolver v11 | **Confirmed** | Both halves verified. Caveat: Oxc is a fast-moving 0.x crate (already at 0.137 by June 2026); all Oxc crates MIT. |
| 3 | Dead-code via mark-reachable traversal from plugin entry points; unreached exports flagged (attributed to predicates.rs) | **Partly** | Mechanism real; file attribution wrong — `mark_reachable` BFS is in `graph/reachability.rs`, not `predicates.rs`. |
| 4 | Duplication uses suffix array + LCP (not quadratic) with 4 modes (strict/mild/weak/semantic) + family grouping | **Confirmed** | All sub-claims verified in `mod.rs`/`normalize.rs`/`families.rs`. Caveat: tokenizer is JS/TS-specific; needs Python reimplementation. |
| 5 | Plugins are a Plugin trait of pure static data (entry_patterns, resolve_config, used_exports, path_aliases → PluginResult) | **Partly** | Trait and methods real and named correctly; "pure static data" overgeneralizes — trait is static defaults *plus* dynamic AST-driven `resolve_config`. |
| 6 | Audit incrementalism uses git changed-files + base worktree (BaseWorktree::create) + xxHash3-64 cache key; introduced-vs-inherited attribution; NewOnly gates only new issues | **Confirmed** | All elements verified in `audit.rs`. Nuance: "SHA" = the included git commit SHA, not the hash algorithm (xxh3 is not SHA-family). |
| 7 | Free OSS layer covers all static analysis + CLI/LSP/MCP + all formats; paid Runtime adds V8/Istanbul coverage gated by offline Ed25519 JWT | **Partly** | Architecture confirmed (MIT; Ed25519/EdDSA, algorithm-pinned; offline). Refuted premise: fallow is NOT a Python analyzer (Oxc = JS/TS). Nuance: static coverage-gaps free, runtime coverage paid; some normalization closed-source. |
| 8 | Caching: bitcode v0.6, persistent .fallow/cache/, extraction cap FALLOW_CACHE_MAX_SIZE default 256MB, base-snapshot cap 16MiB | **Confirmed** | All four verified (`Cargo.lock` bitcode 0.6.9; `MAX_AUDIT_BASE_SNAPSHOT_CACHE_SIZE = 16*1024*1024`). Nuance: extraction cache is `.fallow/cache.bin`; base snapshots under `.fallow/cache/`. |
| 9 | Ruff has no whole-project/reachability dead-code rule; checks are per-file pyflakes reimplementations (F401/F811/F841/ARG) | **Confirmed** | Verified via Ruff registry source + open [issue #872](https://github.com/astral-sh/ruff/issues/872) + Skylos benchmark (Ruff 62.67%). Skylos benchmark is vendor-run/small. |
| 10 | Vulture and deadcode are whole-project in scope but name-table matching (not reachability); a symbol called only by other dead code counts as used | **Confirmed** | Verified in vulture `core.py` (`defined_* − used_names`) and deadcode docs. Vulture MIT v2.16; deadcode AGPL-3.0, stale since 2024. |
| 11 | Skylos is the only mature mainstream Python tool doing entry-point reachability over a call/package graph; framework-aware | **Partly** | Concrete sub-claims (v4.25.0, Apache-2.0, selectors, framework-awareness) confirmed; "only" is unprovable/marketing-grade and "mature mainstream" is debatable (single-maintainer, vendor benchmarks). |
| 12 | Skylos has expanded into a general PR scanner (security/secrets/CVEs/AI-code mistakes), diluting dead-code focus | **Confirmed** | Verified on PyPI/GitHub. Nuance: default scan still focuses on dead code; broader checks behind `-a`; dead code actively maintained. |
| 13 | A Rust reachability dead-code detector for Python already exists (Bury) but is pre-alpha and unproven | **Confirmed** | Verified: neural-garage/tools, 0 stars, 15 commits, no tags/releases, no crate, stale (last push 2025-12-24), no autofix. Now multi-language (Python+TS). |
| 14 | The Rust type checkers ty and pyrefly build whole-project graphs but do not provide dead-code/reachability reporting | **Confirmed** | Verified: ty frames reachability as future "will power" work; pyrefly's only report covers type-annotation coverage. Minor: "10–50x faster" is ty's figure vs mypy; pyrefly's headline is "up to 125x faster updated diagnostics." |
| 15 | No actively-maintained mainstream Python duplication tool does suffix-array + semantic (Type-2/renamed-variable) clone detection | **Partly** | Precise conjunction holds for Python. But Type-2/renamed detection ships in mainstream CLIs for other languages (PMD CPD `--ignore-identifiers` Java/C++; Simian Java/C), and the cited fallow source is JS/TS, not Python. |


---

## 8. Deep Python Tool Review (2026 currency pass)

This appendix re-verifies the competitive landscape against PyPI/GitHub releases as of June 2026, cluster by cluster. Each section gives a prose review, an updated comparison table, corrections to earlier claims with verdicts, and any tools we missed. The load-bearing axis everywhere is *detection technique*, not feature lists.

### 8.1 Dead-code / unused-symbol tooling

The space splits into four technically-distinct families, and our framing is essentially right: (1) AST-local per-file checkers (pyflakes, ruff F401/F811/F841/ARG/RUF029/RUF100, autoflake, pycln, unimport) that are correct-by-construction because they never claim global death; (2) whole-project name-table set-difference tools (vulture, deadcode, dead.py, and the new PyDeadCode) that report `defined - used` and therefore treat a function called only by other dead code as "used" — the exact blind spot we target; (3) entry-point reachability over a call/package graph (skylos, and the pre-alpha Bury); and (4) whole-project type-checker graphs that build the graph but expose no dead-code findings (ty, pyrefly). The material currency change is that **skylos 4.25.0 (2026-06-19) already does whole-project, framework-aware reachability with a confidence threshold and autofix** — actively maintained. Our central "the reachability+framework+confidence niche is open" positioning is therefore *imprecise*: the niche is contested, not empty. The honest, defensible wedge is narrower and still real: skylos dilutes dead-code into a sprawling SAST/secrets/CVE/LLM PR scanner, uses lossy tree-sitter, and carries an optional non-deterministic LLM step; and it exposes a single confidence *threshold*, not tiered Certain/Likely/Uncertain *verdicts with reason strings* and a safe-autofix gate. No tool combines reachability with granular dead-*members* (enum members, `__all__` entries, properties, methods) as a first-class, evidence-carrying output. Lead with focused + deterministic + tiered + granular-members, not with "first/only."

| name | language | license | 2026 version | technique | key limitation |
|---|---|---|---|---|---|
| vulture | Python | MIT | 2.16 (2026-03-25) | whole-project name-table set-difference (`defined - used`); NOT reachability | name-matching, scope-blind flat 60-100%% heuristic; no JSON/SARIF; no real autofix |
| deadcode | Python | AGPL-3.0 | 2.4.1 | name-table with scope/namespace tracking (still set-difference) | AGPL blocks commercial adoption; same reachability blind spot |
| dead (dead.py) | Python | MIT | v2.x | AST def/use over `git ls-files`; name-table class | name-matching not reachability; FPs on interfaces/metaclasses; no autofix/JSON |
| PyDeadCode | Rust (tree-sitter) | Unverified (site-only) | newcomer 2025-2026 | tree-sitter parse + whole-project name-table | still name-table not reachability; maturity/license/repo unconfirmed (low confidence) |
| ruff | Rust | MIT | 0.14.x line | AST-local per-file pyflakes reimpl (F401/F811/F841/ARG/RUF029/RUF100) | no whole-project/reachability rule; issue #872 still OPEN |
| pyflakes | Python | MIT | 3.4.0 (2025-06-20) | AST-local single-file | per-file only; no cross-module; no autofix/JSON |
| autoflake | Python | MIT | 2.3.3 | consumes pyflakes per-file results; rewrites source | imports/vars only; no functions/classes; no reachability |
| pycln | Python | MIT | 2.6.0 | AST-local per-file; understands `__all__`/`TYPE_CHECKING` | unused imports only |
| unimport | Python | MIT | 1.4.0 (2026-06-02) | AST-local per-file; side-effect imports/aliases | imports only |
| flake8-eradicate | Python | MIT | 1.5.0 | regex match of commented-out code (wraps eradicate) | different problem (commented code, not unused symbols) |
| skylos | Python core + tree-sitter | Apache-2.0 | 4.25.0 (2026-06-19) | tree-sitter + call/package-graph entry-point reachability; framework presets | scope creep (SAST/secrets/CVE); lossy CST; optional LLM non-determinism; single threshold not tiered |
| Bury | Rust (tree-sitter) | MIT/Apache | pre-alpha (no release) | tree-sitter + BFS/DFS reachability from entry points | 0 stars, ~15 commits, no crate/release/autofix; not usable |
| ty / pyrefly | Rust | MIT (/Apache) | ty 0.x preview; pyrefly active | whole-project semantic graph | no dead-code/reachability findings exposed in 2026 |

**Corrections to earlier claims**
- *"No mainstream tool does whole-project framework-aware reachability with confidence tiers."* **Verdict: imprecise.** skylos 4.25.0 does whole-project framework-aware reachability with a configurable confidence threshold and autofix today. Reframe from "only/first" to focused + deterministic + tiered + granular-members.
- *"Ruff has no whole-project/reachability dead-code rule; per-file pyflakes reimplementations."* **Verdict: correct.** Confirmed June 2026: #872 still OPEN (filed 2022-11); #19797 ("rule for unused functions") closed as duplicate of #872; maintainers steer reachability into ty, not the linter. Add RUF029 and RUF100 to the per-file enumeration.
- *"Skylos is Python (tree-sitter) / a true Rust engine is a hard-to-copy wedge vs Skylos."* **Verdict: imprecise.** Skylos is now multi-language (Python, TS/JS, Java, Go, PHP, Rust, Dart, C#, Shell) with package-graph reachability, MCP, TUI, optional LLM. The Rust-core speed wedge is real, but PyDeadCode (Rust+tree-sitter, claiming 10-50x over vulture) now also exists, so "genuinely Rust" alone is no longer unique — pair it with determinism + ruff-AST + tiered confidence.
- *"Vulture v2.16, MIT."* **Verdict: correct.** Confirmed 2026-03-25, MIT, Python >=3.9, flat 60-100%% heuristic.
- *"deadcode is AGPLv3, stale since 2024."* **Verdict: imprecise.** AGPL-3.0 confirmed (the durable objection), but 2.4.1 is referenced in current pre-commit config — treat as low-but-not-dead maintenance.
- *"Bury is pre-alpha, no releases/crate/autofix."* **Verdict: correct.** Confirmed June 2026; now Python+TypeScript via tree-sitter + BFS/DFS.

**Missing tools we should add:** PyDeadCode (contests the Rust/10-50x speed wedge, though name-table not reachability); ruff RUF029/RUF100 (completes the per-file enumeration); flake8-eradicate/eradicate (commented-out code, a different sub-problem); **ty as the real future ruff dead-code path** (maintainers defer reachability to ty, not ruff); coverage-driven dead code (coverage.py/SlipCover) and skylos "runtime smart tracing" (the paid runtime-merge layer is becoming table stakes, not a moat).

### 8.2 Dependency hygiene + supply-chain tooling

This cluster is two orthogonal axes, not one list: **hygiene/correctness** (is each declared dep used? is each used import declared?) via deptry, FawltyDeps, pipdeptree, pip-tools, and `uv lock --check`; and **security/supply-chain** via pip-audit, osv-scanner, the new uv audit, safety, and bandit (code-level SAST, a different angle). Our "deptry-equivalent" hygiene anchor is correct — deptry is the de-facto standard, with rule codes DEP001 (missing), DEP002 (unused), DEP003 (transitive), DEP004 (dev-in-prod), DEP005 (stdlib-as-dep). The key precision fix: deptry's Rust core (added v0.14.0, 2024-03; extended v0.15.0) covers only the *import-extraction* stage via ruff's parser; the rule engine, manifest parsing, and import-name to distribution mapping stay in Python, and the mapping runs against the *installed environment* via `importlib.metadata.packages_distributions()`. That is the single biggest operational caveat: deptry must be installed in the project venv with packages installed, or it emits false DEP001+DEP002 pairs for any distro whose import name differs from its project name (PyYAML/yaml, Pillow/PIL, opencv-python/cv2). The "unused AND vulnerable" composite is endorsed but no single tool computes it — implement it as a join across two tools on the PEP 503-normalized distribution name (deptry/FawltyDeps JSON intersect pip-audit/osv-scanner/uv audit), and treat the intersection as a prioritized review signal, not auto-delete, because "unused" is a false positive for dynamic/entry-point/plugin packages.

| name | language | license | 2026 version | technique | key limitation |
|---|---|---|---|---|---|
| deptry | Python + Rust (import extraction) | MIT | 0.25.1 (repo now osprey-oss/deptry) | Rust (ruff parser) extracts imports; Python maps to distribution via installed-env metadata; compares to manifest | must run in project venv w/ packages installed or emits false DEP001+DEP002; blind to dynamic/plugin imports; no CVE data |
| FawltyDeps | Python | MIT | 0.20.0 | static AST imports; mapping via custom TOML / identity / installed env | same static blind spots; no CVE; smaller CI footprint |
| pip-audit | Python | Apache-2.0 | 2.9.x | resolves dep set, queries vuln service per package/version | Python-only; CycloneDX SBOM only (no SPDX); no malware/typosquat |
| osv-scanner | Go | Apache-2.0 | v2.x (V2 GA Mar 2025) | parses lockfiles (incl. poetry.lock, uv.lock), matches osv.dev | guided remediation/autofix is npm+Maven only, NOT Python; needs a lockfile |
| uv audit | Rust (uv) | Apache-2.0 / MIT | uv 0.10.12+ (preview) | reads uv.lock, queries OSV; opt-in MAL malware check via `UV_MALWARE_CHECK=1` | preview/unstable; uv projects only; OSV-only data |
| safety (Safety CLI) | Python | open-core / commercial | 3.x | scans deps vs curated DB; org policy/firewall | free tier single-user, not for commercial use; full DB requires paid (~$25/seat/mo) + login |
| bandit | Python | Apache-2.0 | 1.8.x | AST SAST for insecure code patterns (~47 B-codes) | answers a DIFFERENT question (insecure code, not dependency CVEs); no taint/cross-file dataflow |
| pipdeptree | Python | MIT | 2.x | walks installed metadata; forward/reverse tree | installed env only; no hygiene/unused; no CVE |
| pip-tools | Python | BSD-3-Clause | 7.x | resolves+pins transitive deps; `--generate-hashes` | not a detector; largely superseded by uv |

**Corrections to earlier claims**
- *"deptry has a Rust core via ruff_python_ast."* **Verdict: imprecise.** True only for the import-extraction stage; the rule engine, manifest parsing, and import->distribution mapping remain Python, and mapping uses `importlib.metadata.packages_distributions()` against the installed env.
- *"deptry rule codes DEP001-005; manifests poetry/pdm/uv/PEP621/PEP735."* **Verdict: correct.** Confirmed exactly, latest 0.25.1; Poetry 2.0 PEP 621 since 0.23.0; PEP 735 `[dependency-groups]` supported.
- *"unused AND vulnerable composite verdict."* **Verdict: correct.** Sound and high-ROI, but no single tool computes it; implement as a join on PEP 503-normalized names and treat as a review signal, not auto-delete.
- *"deptry-equivalent as the hygiene anchor."* **Verdict: correct.** deptry is the right de-facto standard; FawltyDeps is the main alternative (better mapping ergonomics, notebooks, dynamic-import warnings on roadmap); pipdeptree/pip-tools are complements.

**Missing tools we should add:** **uv audit** (OSV-backed, ~4-10x faster than pip-audit, plus opt-in OSV malware (MAL) checks — the only listed tool addressing malware/typosquatting); `uv lock --check`/`--locked` (fast reproducibility gate); semgrep (taint/cross-file dataflow SAST covering what bandit cannot); grype/trivy (polyglot SBOM+CVE scanners) if we claim to survey the supply-chain space.

### 8.3 Complexity / churn / maintainability tooling

Four functional layers: single-snapshot metric calculators (radon, lizard, cognitive_complexity); threshold gates (xenon, ruff C901, flake8-cognitive-complexity); history/trend trackers (wily, the only FOSS one); and coverage-weighted risk (the CRAP concept, no FOSS Python-native implementation). radon 6.0.1 remains the de-facto metric engine (McCabe CC with A-F grades, all Halstead metrics, Maintainability Index, raw SLOC) and is what xenon and wily shell out to. complexipy 5.6.1 (~2026-06-16) is the fast Rust cognitive-complexity tool, and our claims about it are confirmed: `--diff` enables ratchet mode (exits 1 only on new/modified functions breaching the limit, tolerating existing debt) and it emits SARIF (json/csv/gitlab/sarif) with documented GitHub Actions upload. Both of our headline niche claims hold for FOSS Python: **churn x complexity hotspot ranking is genuinely unfilled** (the only real product is commercial CodeScene; wily has the git time-axis but reports metric *trends/deltas*, not a churn x complexity *product* ranking; radon/complexipy/lizard have complexity but no churn; ruff/xenon are pure gates), and **CRAP-style coverage-weighting is an opening** (no maintained FOSS Python-native CRAP tool; crap4j is dead Java; the only current Python CRAP is commercial Qt Coco 7.5). Build-vs-reuse guidance: reuse complexipy's shipped diff/ratchet/SARIF and differentiate on the churn and coverage axes it lacks; derive churn cheaply from `git log --numstat` (no checkouts) joined to a single current-tree complexity pass, rather than re-walking commits like wily.

| name | language | license | 2026 version | technique | key limitation |
|---|---|---|---|---|---|
| radon | Python | MIT | 6.0.1 (Oct 2025) | AST visitor: CC = decision points + 1; Halstead; MI = f(SLOC,CC,volume) | snapshot only, no history/churn; pure-Python slow on large trees; CC not cognitive |
| xenon | Python | MIT | 0.9.x | wraps radon CC; threshold gate (absolute/modules/average) | CC only; no history; pass/fail, no ranking |
| wily | Python | Apache-2.0 | 1.25.x | checks out last N revisions, runs radon per revision into a cache | tracks metric TRENDS not churn x complexity ranking; slow (physical checkouts) |
| complexipy | Python (Rust core) | MIT | 5.6.1 (~Jun 16 2026) | Sonar/Campbell cognitive complexity (flow breaks + nesting) | cognitive only (no CC/Halstead/MI); snapshot+baseline diff, no git churn |
| lizard | Multi (15+ langs) | MIT | 1.17.x | lightweight per-language tokenizer; CCN/NLOC/tokens + clone detection | CC only; no history/churn; less precise than full AST on edge cases |
| cognitive_complexity / flake8-cognitive-complexity | Python | MIT | cc 1.3.x; plugin 0.1.x | AST walk implementing Campbell cognitive complexity | pure-Python (slow vs complexipy); no history; gate-only in flake8 form |
| ruff (C901) | Python (Rust core) | MIT | 0.12.x line | C901 = McCabe CC via `[tool.ruff.lint.mccabe]` | CC only (no cognitive/Halstead/MI/history); 2026 bug #24004 mis-flags `# noqa: C901` |
| SonarQube / sonar-python | Python (analyzer in Java) | LGPL + commercial | 2025.x / 10.8+ | S3776 cognitive complexity (default 15/fn) among 500+ rules | heavyweight server, not a CLI metric tool; IMPORTS SARIF, not primary emitter; commercial for full features |

**Corrections to earlier claims**
- *"churn x complexity hotspot ranking is an unfilled niche."* **Verdict: correct.** Confirmed for FOSS Python; only CodeScene (commercial) ranks change-frequency x code-health; nothing FOSS emits a (commit frequency x current complexity) ranking.
- *"CRAP-style coverage-weighting is an opening."* **Verdict: correct.** No maintained FOSS Python-native CRAP tool; coverage.py and radon both exist but nothing FOSS joins them into CRAP = comp^2*(1-cov)^3+comp.
- *"complexipy provides diff/ratchet/SARIF."* **Verdict: correct.** All confirmed at 5.6.1; target the 5.x API.
- *"wily provides git-history churn awareness."* **Verdict: imprecise.** wily has git-history machinery but tracks how a metric MOVES over time, not churn-weighted hotspot ranking, and is slow because it checks out each commit; derive churn from `git log --numstat` instead.
- *"SonarQube emits SARIF for these complexity metrics."* **Verdict: imprecise.** SonarQube primarily IMPORTS external SARIF 2.1.0 as issues; complexipy and ruff are the native SARIF emitters in this stack.

**Missing tools we should add:** `git log --numstat` (built-in, the cheapest churn source — per-file change frequency without checkouts); coverage.py / pytest-cov JSON export (the missing input to compute a FOSS Python CRAP score joined to radon CC); CodeScene (the reference commercial hotspot product the niche competes against); cyclop (a separate fast CC CLI); mccabe (PyCQA, the original standalone checker ruff C901 and flake8 derive from).

### 8.4 Duplication / clone-detection tooling

Grounding the verdicts on the clone-type taxonomy: Type-1 (identical modulo whitespace/comments), Type-2 (identical structure, identifiers/literals renamed — what we call "semantic"), Type-3 (Type-2 + small edits/gaps), Type-4 (functionally equivalent, structurally different — research/ML only). Technique is the load-bearing fact. pylint R0801 hashes runs of N successive source *lines* — text-based, not tokenized — so it is Type-1 only and cannot detect renamed-variable clones. The important correction: **lizard `-Eduplicate` is token-based and DOES normalize identifiers** (its `NestingStackWithUnifiedTokens` maps identifiers to `v0`/`v1`..., constants to `1`, builds 31-token rolling windows, hashes and extends matches), so it detects Type-2 in Python today and is actively maintained (1.22.1, April 2026, MIT). That directly contradicts our "no actively-maintained Python tool does Type-2" claim. jscpd v5 (2025) is now a ground-up Rust rewrite (24-37x faster than the old TS engine) with native SARIF and an MCP server, but uses Rabin-Karp over tokens with strict/mild/weak modes that control comment/format tolerance, *not* identifier renaming — so jscpd is Type-1/threshold-Type-3, no Type-2. PMD CPD's `--ignore-identifiers`/`--ignore-literals` (the Type-2 enablers) are Java/C++ only, NOT Python, so on Python it is Type-1. Simian is line-based with `ignoreVariableNames` (coarse Type-2 approximation, "initial" Python support, now Apache-2.0). The genuinely defensible differentiation is *narrower than we stated*: a **suffix-array (SA-IS) + LCP whole-corpus engine** is absent from mainstream Python tooling (lizard uses fixed-window rolling-hash; jscpd/PMD use Rabin-Karp; pylint hashes lines), and porting it with a Python tokenizer and four normalization tiers IS novel — but Type-2 itself is not the moat. Lead with the engine + sub-quadratic scale + tiered normalization + fingerprinted clone families with refactor suggestions + first-class SARIF/MCP; cite lizard as prior art and NiCad as the Type-2 validation oracle.

| name | language | license | 2026 version | technique | key limitation |
|---|---|---|---|---|---|
| pylint (R0801 / similar.py) | Python | GPL-2.0 | 3.3.x / 4.x line | hash of N successive source LINES (text-based, not tokenized) | Type-1 only; cannot detect renamed-variable (Type-2); slow on large repos |
| lizard (-Eduplicate) | Python tool, ~15 langs | MIT | 1.22.1 (Apr 2026) | TOKEN-based with identifier UNIFICATION (v0/v1..., constants to 1); 31-token rolling windows | fixed-window window-hash (not suffix-array); no clone families/refactor suggestions; no SARIF/MCP |
| jscpd (v5) | Rust engine | MIT | v5.x (Rust rewrite 2025) | Prism tokenize + Rabin-Karp rolling hash; strict/mild/weak modes | NO identifier normalization, so no Type-2 (Type-1 / threshold Type-3 only) |
| PMD CPD | Java tool, Python + ~20 langs | BSD-style | PMD 7.x | per-language tokenizer + Karp-Rabin; optional anonymization | `--ignore-identifiers`/`--ignore-literals` are Java/C++ ONLY, NOT Python -> Type-1 on Python |
| Simian | Java/.NET, any text incl. Python | Apache-2.0 | 4.0.0 (2022-23 era) | line/text comparison with normalization flags | line-based (coarse); Python support "initial"; sparse maintenance; no SARIF/MCP |
| NiCad | TXL-based; Python/Java/C/C# | BSD-style / academic | 7.x (research) | TXL parse + pretty-print normalization + blind renaming + LCS/threshold | research tooling; clunky to embed; not CI-friendly; no SARIF/MCP |

**Corrections to earlier claims**
- *"Semantic (Type-2 renamed-variable) clones - no actively-maintained Python tool does this."* **Verdict: wrong.** lizard `-Eduplicate` (1.22.1, Apr 2026, MIT) does token-normalized Type-2 in Python today. Soften the claim: differentiate on the SA-IS+LCP engine, whole-corpus sub-quadratic scaling, tiered normalization, clone families, and SARIF/MCP — NOT on being first to Type-2.
- *"Duplication via suffix array + LCP, 4 modes (strict/mild/weak/semantic)."* **Verdict: correct.** The SA-IS+LCP approach with four modes is genuinely absent from mainstream Python clone tools; porting it for Python is differentiated at the algorithm level. Keep it — just stop claiming Type-2 itself is unprecedented.
- *"PMD CPD provides --ignore-identifiers Type-2 usable for Python."* **Verdict: wrong.** Those options are Java/C++ only; on Python PMD CPD is Type-1 only. Do not cite it as Python Type-2 prior art.
- *"standalone dupes slower than jscpd."* **Verdict: imprecise.** jscpd v5 is now Rust (24-37x over the old TS engine) and already ships SARIF + MCP, so those are not standalone differentiators vs jscpd; the integration-over-speed positioning still holds.
- *"pylint R0801 as a weak duplication baseline."* **Verdict: correct.** Confirmed it hashes N successive source LINES (text-based), so Type-1 only.

**Missing tools we should add:** **lizard `-Eduplicate`** (our biggest blind spot — actively-maintained MIT pure-Python tool that already does token-normalized Type-2 for Python; must be named as prior art); NiCad (best Type-2/Type-3 recall, the right validation oracle); jscpd-server (jscpd already exposes clones over MCP, so MCP is not itself a differentiator); Simian `ignoreVariableNames` (line-based Type-2 approximation, now Apache-2.0); CCFinderX (legacy suffix-*tree* Type-2 precedent).

### 8.5 Architecture-boundary / import-graph / circular-dependency tooling

Two tools dominate enforceable Python architecture boundaries and have converged on a Rust-accelerated core but differ in philosophy. **tach 0.34.1 (Apr 2026)** is Rust-implemented, config-first (`tach.toml`), modular-monolith oriented, with its own Rust import parser; it does NOT use grimp. It enforces declared-dependency-only imports, public-interface-only cross-module access, and no cycles, plus first-class layers, interfaces with symbol-level `expose`, visibility/exclusive rules, and monorepo/namespace support. **import-linter 2.12 (2026-06-23)** is a pure-Python contract engine (layers/forbidden/independence/custom) sitting on top of **grimp 3.14**, which is the real engine we under-named: grimp 2.0 dropped networkx, 3.6 reimplemented the whole graph in Rust with a Rust import parser, and `find_illegal_dependencies_for_layers` (2.5) powers import-linter 2.7+ for layers and independence (note 3.10 was yanked for large-graph perf — use >=3.11). grimp builds a module-level import graph, not a symbol/call graph. The rest are visualization or research tools (pydeps, pyreverse, PyCG, Jarvis, legacy snakefood). Our "net-new" claim splits cleanly: **symbol-level public-interface enforcement is NOT net-new** — tach's `interfaces[].expose` is already a list of regex patterns over member/symbol names with `from`/`visibility`/`exclusive` modifiers, i.e. symbol-level public-API enforcement compiled from config today; to be net-new we must go beyond import/name granularity (per-symbol type/data-contract enforcement, re-export tracking, or call-site rather than import-site enforcement). **Named architecture presets ARE genuinely net-new** — neither tach, import-linter, grimp, pydeps, nor pyreverse ships opinionated named presets (hexagonal/ports-and-adapters, feature-sliced, bulletproof, clean/onion); they all give primitives and make you hand-author the topology. Implement presets as a *compiler* that emits import-linter contracts and/or tach.toml, so we inherit Rust-fast graphs, cycle detection, namespace/monorepo handling, and CI integrations for free. Ship presets as the headline differentiator; position symbol-level enforcement as "tach parity, extended," not net-new.

| name | language | license | 2026 version | technique | key limitation |
|---|---|---|---|---|---|
| tach | Python (engine in Rust) | MIT | 0.34.1 (Apr 3 2026) | own Rust import parser building a module import graph; does NOT use grimp | config-driven only (no programmatic custom contract types); preset-less; interface enforcement at import/name granularity, not type/data-contract |
| import-linter | Python | BSD-2-Clause | 2.12 (Jun 23 2026) | contracts over grimp's import graph (reachability incl. indirect imports) | module-level only (no symbol/interface granularity); perf entirely dependent on grimp; no presets; no autofix |
| grimp | Python + Rust extension | BSD-2-Clause | 3.14 (Dec 10 2025) | Rust-reimplemented graph (3.6) + Rust import parser; `find_illegal_dependencies_for_layers` | library not a CLI; module-level import graph only (no call/symbol graph); 3.10 yanked for perf (use >=3.11) |
| pydeps | Python | BSD-2-Clause | 3.0.6 | reads import opcodes from compiled bytecode via modulefinder (NOT AST) | visualization-first, not a CI contract gate; needs importable code |
| pyreverse | Python | GPL-2.0 (pylint) | ships with current pylint | astroid-based static analysis; infers UML relations | diagram generation only; not for CI gating of architecture rules |
| PyCG | Python | Apache-2.0 | research (ICSE 2021) | flow/context-insensitive points-to call-graph (~99.2%% prec, ~69.9%% recall) | call graph not boundary linter; scales poorly to large apps; little maintenance |
| Jarvis | Python | research (open) | research (2023) | demand-driven application-centered points-to call-graph | research-grade, not productionized; call graph, not boundary enforcer |
| snakefood / snakefood3 | Python | GPL-2.0 | snakefood3 ~May 2022 (unmaintained) | AST parse of imports | stale; original is Python 2; no enforcement/contracts |

**Corrections to earlier claims**
- *"Symbol-level public interface enforcement is net-new vs existing tools."* **Verdict: wrong.** tach already enforces it via `interfaces[].expose` (regex over symbol names) with from/visibility/exclusive. Position our version as "tach parity, extended"; net-new requires going beyond import/name granularity.
- *"Named presets (layered/hexagonal/feature-sliced/bulletproof) compiled to contracts is net-new."* **Verdict: correct.** No tool found ships named opinionated presets; all ship only primitives. Implement as a compiler emitting import-linter contracts and/or tach.toml.
- *"import-linter is the primary engine to verify (grimp a sub-detail)."* **Verdict: imprecise.** grimp is the actual engine and deserves first-class treatment: import-linter 2.x is a thin contract layer over grimp 3.14 (fully Rust graph since 3.6); perf/scaling are grimp questions.
- *"grimp/import-linter have Rust acceleration (to confirm)."* **Verdict: correct.** grimp 3.6 reimplemented the graph in Rust with a Rust parser and exposes `find_illegal_dependencies_for_layers`; tach is independently Rust-implemented and does not depend on grimp.

**Missing tools we should add:** **grimp** (name it explicitly — it IS the import-linter engine; all perf/scaling and namespace/external-package capability lives here); deptry (adjacent dependency hygiene, users conflate it with boundary enforcement); Jarvis (PyCG successor with better precision/recall/scale, the citation if we ever push to call/symbol-level reachability); pyreverse (the "already installed" pylint visualization incumbent); ruff (gravitational center of Rust Python linting — note whether boundary rules could land there as future competition).

### 8.6 Rust foundation for a Python analyzer

The foundation decision still stands and is current: build on the ruff crates via a **pinned git rev** (pyrefly's exact pattern), adopt **pyrefly's eager + rayon module-level-incremental engine first**, and defer **Salsa** to the LSP/watch phase. The two changes worth recording: ty (0.0.52 beta, 2026-06-23) and pyrefly (1.0 May 2026, 1.1 June 2026, stable) both hit real milestones but **neither ships project-wide dead-code/reachability reporting** — that space remains open, which validates our wedge — and the current pyrefly-pinned ruff rev to copy is `db5aa0a5f1b92cb91d910bf0866a967554dd94f5`. The ruff crates remain unpublished on crates.io with no API-stability guarantee (issues #10417/#14051/#17970 confirm Astral has no plans to publish; the `rustpython-ruff_python_*` and `littrs-ruff-python-*` crates.io names are unofficial community forks — avoid them). Pyrefly declares parser/AST/text_size (+ ancillary) as git deps against one shared rev; pin all ruff crates to a single rev to avoid AST-type mismatches. The one place we slightly over-promise is `ruff_python_semantic` reuse: pyrefly — the very engine we adopt — does NOT depend on it and hand-rolls its own binding/scope/resolution; ty's semantic model is a separate Salsa-coupled `ty_python_semantic`, not the linter's crate. So "reuse `ruff_python_semantic`" should be a *reference, not a drop-in*: reuse parser+AST and build resolution yourself, mirroring pyrefly.

| name | language | license | 2026 version | technique | key limitation |
|---|---|---|---|---|---|
| ruff_python_parser | Rust | MIT | unversioned; pin rev db5aa0a5... (ruff line 0.15.16, 2026-06-04) | hand-written error-resilient recursive-descent + Pratt; full ranges | not on crates.io; no API stability; must rev-match sibling crates |
| ruff_python_ast | Rust | MIT | unversioned; same pinned rev | typed AST nodes + visitor traversal; serde feature | unpublished; must share exact rev with parser/text_size |
| ruff_text_size | Rust | MIT | unversioned; same pinned rev | u32 TextSize/TextRange newtypes | unpublished; trivial but must be rev-matched |
| ruff_python_semantic | Rust | MIT | unversioned; pinned rev | scope tree + binding table; linter-shaped name-table model | NOT a drop-in for whole-program reachability; pyrefly does not use it; unpublished |
| ty | Rust | MIT | 0.0.52 (2026-06-23) beta | Salsa demand-driven incremental over ruff_db VFS; internal reachability for narrowing | no shipped project-wide dead-code/reachability (vision only); beta API churn |
| pyrefly | Rust | MIT | 1.0.0 (May 2026), 1.1 (Jun 2026) | eager eval + rayon module-level parallelism + interface-change invalidation; no Salsa | no project-wide dead-code/reachability (only unused imports + `--remove-unused-ignores`) |
| libcst (Rust crate) | Rust (+PyO3) | MIT | 1.8.x on crates.io | tokenize + PEG parse into a lossless CST | second tree to maintain; heavier than ruff AST for read-only analysis |
| tree-sitter-python | C grammar (Rust bindings) | MIT | current (Python 3.10-3.14) | GLR incremental parsing; untyped CST | generic untyped CST -> complement, never the analysis foundation |
| salsa | Rust | MIT/Apache-2.0 | published, actively maintained | demand-driven query graph with revision invalidation | heavier mental model + less memory control than eager+rayon; defer to LSP phase |
| rustpython-parser | Rust | MIT | DEPRECATED / superseded | legacy LALRPOP parser | superseded by ruff's parser; do not build on it |
| pyo3 / maturin | Rust | MIT/Apache-2.0 | current, stable | abi3 FFI bindings + wheel builds | FFI boundary discipline needed; not analysis itself |

**Corrections to earlier claims**
- *"Reuse ruff_python_semantic's scope/binding model as the starting point for symbol resolution and the import graph."* **Verdict: imprecise.** Treat it as a reference, not a drop-in: pyrefly consumes only parser+AST+text_size and hand-rolls resolution; ty's semantic model is a separate Salsa-coupled crate. Build resolution yourself, mirroring pyrefly.
- *"Both ty and pyrefly use the ruff crates."* **Verdict: imprecise.** True for parser/AST/text_size, but consumption depth differs: pyrefly uses parser+AST+text_size only (pins db5aa0a5...) and does not use ruff_python_semantic; ty consumes more of the ruff_db/Salsa stack.
- *"Adopt ty's Salsa query model only later; start with pyrefly's eager+rayon engine."* **Verdict: correct.** Confirmed June 2026: pyrefly 1.0/1.1 uses eager + rayon + interface-change invalidation and deliberately avoids Salsa; ty uses Salsa via ruff_db. Phased decision is sound.
- *"RustPython's parser is deprecated."* **Verdict: correct.** Pulled into ruff and superseded by ruff's hand-written parser; do not build on rustpython-parser. (Minor: RustPython the interpreter still exists and itself pins ruff's parser/ast by git rev.)
- *"Astral does not publish the ruff crates; pin a git revision; avoid the unofficial crates.io forks."* **Verdict: correct.** Still true per #10417/#14051/#17970; current pyrefly-pinned rev is db5aa0a5f1b92cb91d910bf0866a967554dd94f5.
- *"ty/pyrefly are a type-health shell-out option, not dead-code competitors."* **Verdict: correct.** Reinforced: as of June 2026 neither ships project-wide dead-code/reachability (only type-local unused diagnostics); Astral lists dead-code/unused-deps/CVE-reachability as long-term vision only. The "Python has no fallow" wedge holds.

**Missing tools we should add:** **ruff_db** (the Salsa-backed VFS/database crate ty's incremental + semantic model is built on — the real reference if we migrate to Salsa, not ruff_python_semantic); **ty_python_semantic** (ty's actual semantic/type-inference crate, the right comparison for a whole-program semantic model); libcst_derive (companion crate for the format-preserving autofix backend); ruff_source_file / **ruff_notebook** (ancillary crates pyrefly pins at the same rev; ruff_notebook gives free Jupyter source-map handling for native .ipynb analysis).

### Net verdict: does the white-space thesis still hold?

**Yes, but the white space is narrower and more precisely shaped than our first pass implied — and in two clusters the original "nobody does this" framing is simply wrong and must be retired.** The Rust-foundation bet is fully current and the dead-code/reachability gap among the Rust type checkers (ty, pyrefly) remains genuinely unoccupied. But three claims need correcting: skylos 4.25.0 already does whole-project framework-aware reachability with a confidence threshold (so our wedge is determinism + tiered verdicts + granular dead-members + a focused product, not "first/only"); lizard `-Eduplicate` already does token-normalized Type-2 clones for Python (so our duplication wedge is the SA-IS+LCP whole-corpus engine + clone families + integration, not Type-2 itself); and tach already does symbol-level public-interface enforcement (so our architecture wedge is *named presets compiled to contracts*, which IS net-new, not symbol-level enforcement). Where the thesis holds cleanly and unchallenged: FOSS Python churn x complexity hotspot ranking and CRAP-style coverage-weighted risk (no FOSS Python tool does either), the "unused AND vulnerable" composite (no single tool computes it), and a single deterministic Rust-cored audit that unifies all of these with first-class tiered confidence, SARIF/MCP, and safe LibCST autofix. The defensible position is integration + determinism + tiered evidence + the genuinely empty niches — not "first to X." The white space is real; the marketing must be honest about which walls of it are already partly built.
