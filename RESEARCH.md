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
