# Full-repo code review — 2026-07-01

Scope: every crate in the workspace (`mollify-types`, `mollify-parse`,
`mollify-graph`, `mollify-core`, `mollify-cli`, `mollify-mcp`, `mollify-lsp`),
plus docs, cookbook, scripts, CI workflows, and packaging. Method: full-file
reads of all ~14k lines of Rust, cross-checked against the five invariants in
CLAUDE.md; every finding below was **verified empirically** (probe programs
against the crates, or end-to-end runs of the built `mollify` binary) before
being recorded. Speculative findings were discarded.

Baseline at review time (commit 9a71aa9, v0.1.3): `cargo build` clean, all
tests pass, `cargo clippy --all-targets` clean, and order-level determinism
holds end-to-end (two identical runs are byte-identical).

## Executive summary

The overall engineering quality is high — the invariants are mostly real, not
aspirational (sorting before emit is systematic, Tarjan/SA-IS are correct,
the candidate/verifier gate is enforced where it's declared, CI runs
fmt/clippy/test/version checks). The review still surfaced:

- **2 critical bugs**, both in the one code path allowed to write to user
  files: `fix --apply` destroys Jupyter notebooks (C1) and deletes live,
  dynamically-dispatched code it wrongly grades `Certain` (C2).
- **A cross-cutting identity defect**: fingerprints embed the `--path`
  spelling and (for several rules) line numbers, so the baseline feature's
  documented contract ("content-derived, survives file moves, independent of
  minor edits") does not hold in practice (X1–X3).
- **13 majors**: dead-code false positives from resolver gaps (signature
  defaults/annotations, `with`-suite imports, lazy importers, root
  `__init__.py`), a coverage engine that can't fire for imported modules,
  notebook JSON fed to the dupes tokenizer, dependency-analysis false
  positives (psycopg2 alias, dev-group tools), two `read_dir`-order
  nondeterminisms, and three CI-gate trust bugs in the CLI (failed baseline
  write reports success; missing baseline silently disables
  `--fail-on-regression`; nonexistent `--path` scores 100/100 with exit 0) —
  plus one doc-shipped command that doesn't parse.
- ~35 minors/nits across parse semantics, PEP 440 handling, MCP/LSP protocol
  compliance, contract-evolution hygiene, and doc drift.

Nothing here requires an architecture change; the two criticals and the
fingerprint cluster are the items I'd fix before the next release.

---

## Critical

### C1. `mollify fix --apply` corrupts Jupyter notebooks
`crates/mollify-core/src/fix.rs:44-76`, with `deadcode.rs:193-222`

Notebook findings carry line numbers relative to the **concatenated code
cells** (`read_source`), but `fix::apply` opens the raw `.ipynb` file and
drains those line numbers from the JSON text. `unused_imports` marks notebook
imports `Certain` + `auto_fixable` with no `.ipynb` guard.

Reproduced: a notebook whose first cell starts with an unused `import os`
produces the plan `analysis.ipynb:1-1 Remove the unused import`; after
`--apply` the file's opening `{` is deleted and the notebook is no longer
valid JSON.

### C2. Cross-module dynamic use doesn't downgrade confidence — `fix --apply` deletes live code
`crates/mollify-core/src/deadcode.rs:349-355`, with `fix.rs:24-28`

`unused_symbols` downgrades confidence only when the **defining** module has a
dynamic sink (`m.parsed.has_dynamic_sink`); `unused_files` correctly consults
`graph.global_dynamic`, but the symbol path does not. A private function
invoked from another module via `getattr(lib, "_handler_" + name)()` is
reported `certain` + `auto_fixable`.

Reproduced end-to-end: `fix --apply` deleted `_handler_a` from a module while
`__main__.py` invoked it via `getattr` — the program is now broken. This is a
candidate/verifier violation in spirit (invariant 2): a heuristic finding is
claimed `Certain` and auto-applied.

---

## Cross-cutting: fingerprint & baseline identity is unstable (invariants 1 & 5)

The baseline feature's own doc (`baseline.rs:4-5`) promises fingerprints are
"content-derived" and "survive file moves"; `fingerprint.rs:6` promises
"independent of run order and minor edits". Neither holds:

### X1. Fingerprints embed the invocation-root path spelling
`crates/mollify-core/src/fingerprint.rs:7`, callers in every engine

`location.path` is the root-joined path exactly as the user spelled `--path`
(never canonicalized, never made root-relative), and nearly every fingerprint
hashes `m.path.as_str()`. Reproduced two ways:
- The same project audited with `--path .` vs `--path /abs/path` emits
  different bytes: `./billing/app.py` vs `/…/billing/app.py`, and
  `unused-file:12386123` vs `unused-file:acccc57d` for the same finding.
- Byte-identical projects at two directories produce fully disjoint
  fingerprint sets.

Consequence: a baseline captured in CI (`/home/runner/work/…`) marks **every**
finding "new" on a laptop; the "no new issues" gate is defeated. Fingerprints
should hash the root-relative path (which also fixes output portability).

### X2. Line numbers inside fingerprints churn on unrelated edits
`deadcode.rs:101` (unused-import), `typehealth.rs:24`, `coverage.rs:52`,
`security.rs:66`, `commented.rs:80`, `policy.rs:88`, `arch.rs:236`

Reproduced: adding one comment line at the top of a file changed the
`unused-import` fingerprint (`05accef1` → `28f158ad`), so a baselined finding
resurfaces as "new" after any edit above it. Contrast `complexity.rs:32` /
`apihygiene.rs:30`, which correctly hash path+symbol. Better identity for
unused-import: path + imported bindings (+ occurrence index for duplicates).

### X3. Collision/duplication edges of the same identity scheme
- `deadcode.rs:364` — `unused-export` omits the line, so a name defined twice
  in one file yields two findings with **identical** fingerprints (verified),
  which fingerprint-keyed consumers (baseline, `dedup_by`) silently merge.
- `fingerprint.rs:10` — only 32 of 64 hash bits are kept (~1% birthday
  collision at ~10k findings per rule); a collision makes `baseline::split_new`
  silently swallow a genuinely new finding.
- `arch.rs:152` — `dedup_by` on fingerprints after sorting by (path, reason)
  only removes *adjacent* duplicates; overlapping contracts can emit duplicate
  fingerprints that survive.

---

## Major

### Parse layer (feeds every engine)

- **M1. Function signatures are never resolved** —
  `mollify-parse/src/lib.rs:1491` (and lambda at `:1531`). The resolver visits
  decorators and body but skips parameter defaults, parameter annotations, and
  return annotations, despite the adjacent comment claiming otherwise.
  Reproduced end-to-end: a module constant used only as a parameter default
  (`def fetch(url, timeout=DEFAULT_TIMEOUT)`) is reported
  `unused-export … no reachable references` — a dead-code false positive on a
  very common pattern.
- **M2. Imports inside module-level `with`/`for`/`while`/`match` are invisible**
  — `mollify-parse/src/lib.rs:490,599`. `scan_top_level` recurses only into
  `If`/`Try`; `NestedImportVisitor` only records under `FunctionDef`/`ClassDef`.
  `with suppress(ImportError): import ujson` records no import at all →
  the imported module can be reported dead and its distribution unused.

### Graph layer

- **M3. `symbol_used` ignores lazy importers** —
  `mollify-graph/src/lib.rs:572-584`. Cross-module symbol-use checks consult
  `self.edges` only; `lazy_edges` (function-scoped imports — the canonical
  cycle-breaker pattern) feed reachability (`:498`) but not symbol use, so
  `def run(): import helper; return helper.go()` leaves `helper.go` reported
  dead.
- **M4. Root-level `__init__.py` breaks relative imports** —
  `mollify-graph/src/lib.rs:289-291` + `resolve_relative` at `:699`.
  `strip_suffix("/__init__")` misses the bare root `__init__`, so when the
  analysis root is itself a package, `from . import mod` resolves wrongly,
  `mod` is reported an unused file, and `unresolved_imports` stays silent.

### Engines

- **M5. Coverage cold-code detection never fires for imported modules** —
  `mollify-core/src/coverage.rs:46`. "Ran" is defined as *any* line in
  `f.line..=f.end_line` executed — but importing a module executes every `def`
  line, so every function in every imported module counts as ran. Verified
  against real `coverage json` output: `hot()`/`cold()` with only `hot()`
  called → 0 findings. The unit test masks this by hand-writing
  `executed_lines` without the def line.
- **M6. Duplication engine tokenizes raw notebook JSON** —
  `mollify-core/src/dupes.rs:44`. Uses `fs::read_to_string` instead of
  `read_source`, so near-identical notebook JSON scaffolding across unrelated
  notebooks exceeds the 40-token window: verified false clone family across
  two notebooks with completely different code, with meaningless JSON line
  numbers.
- **M7. `psycopg2 → psycopg2-binary` (and `google → google-api-python-client`)
  aliases are wrong for real distributions** — `mollify-core/src/known.rs:151`.
  `psycopg2` is itself a real PyPI dist; the unconditional alias produces
  *paired* false positives (`unused-dependency: psycopg2` +
  `missing-dependency: psycopg2-binary`). Verified.
- **M8. Dev-group tools all flagged unused** — `mollify-core/src/deps.rs:88-116`.
  All dependency groups fold into one set and "unused" requires an import, so
  `black`, `pre-commit`, `pytest-cov`, `mypy`, `ruff` etc. are `Likely`
  false positives on nearly every modern project (deptry exempts dev deps for
  exactly this reason). Verified with a PEP 735 dev group.
- **M9. Import→dist mapping is first-wins over `read_dir` order** —
  `mollify-core/src/installed.rs:28,64-70`. Namespace packages (`google` claimed
  by protobuf + google-cloud-*) resolve to whichever dist-info the OS
  enumerates first → finding *content* varies across machines. Violates
  invariant 1 (sorting at emit can't repair differing content).
- **M10. Multi-requirements-file projects: finding location depends on
  `read_dir` order** — `mollify-core/src/deps.rs:44-67`,
  `supplychain.rs:266-284,348-370`. Manifest attribution and duplicate-pin
  tie-breaks inherit unsorted directory order; sort the file list (and add
  source/line to sort keys).

### Docs / agent assets

- **M14. `AGENTS.md:35` documents a flag that doesn't exist** —
  `mollify graph [--mermaid] --format json`, but `GraphArgs`
  (`mollify-cli/src/main.rs:186-194`) has only `--path` and `--mermaid`;
  running it errors with `unexpected argument '--format'` (verified). This is
  the primary agent-facing instruction sheet, and the same line ships embedded
  in the binary (`crates/mollify-core/assets/AGENTS.md`), so every
  distribution channel carries the broken command. All other docs get it
  right.

### CLI gates (CI trustworthiness)

- **M11. `--save-baseline` failure still reports success, exit 0** —
  `mollify-cli/src/main.rs:459-466`. Verified: write to an unwritable path
  prints the error to stderr *and* "Wrote baseline …" to stdout, exit 0.
- **M12. `--fail-on-regression` silently passes when the baseline file is
  missing/invalid** — `main.rs:468-472,486-498`. A typo'd baseline path in CI
  means the gate never fires. Verified: exit 0.
- **M13. Nonexistent `--path` reports a clean 100/100 audit, exit 0** —
  `main.rs:500,547` and `mollify-mcp/src/lib.rs:169-170`. Verified both via
  CLI and MCP (`files_analyzed: 0`, no error). A typo'd path in CI silently
  passes every gate. 0 files analyzed should at minimum be an error.

---

## Minor

### Parse
- `lib.rs:564` — `else` branch of `if TYPE_CHECKING:` is marked
  `type_checking_only` too (retroactive patch covers `elif_else_clauses`);
  runtime imports in the `else` branch can never be flagged (inverted
  semantics).
- `lib.rs:1582` — comprehension targets treated as function-scope locals
  (Python 3 comprehensions don't leak); a module name used after a same-named
  comprehension variable becomes a dead-code false positive. Verified.
- `lib.rs:523` — `__all__ += […]` / `.extend(…)` silently ignored →
  confidently wrong partial export list feeding `deadcode.rs:332` suppression.
- `lib.rs:497,714,1249` — decorated def/class line numbers point at the first
  decorator, not the `def` line (ruff ranges include decorators); skews
  locations, coverage spans, and type-leak lines.
- `lib.rs:621` — `is_type_checking_guard` uses substring `contains`
  (`MY_TYPE_CHECKING_OVERRIDE` matches) and misses `if not TYPE_CHECKING:` /
  compound guards (`expr_path` is `None` for BoolOp/UnaryOp).
- `lib.rs:1196` — `collect_typevars` doc says "(anywhere)" but only scans the
  top statement list; TypeVars under `if TYPE_CHECKING:` cause private-type
  leak false positives.

### Graph
- `lib.rs:620-627,683-685` — self-loop handling in `find_cycles` is dead code
  (`resolve_edges` filters self-refs), and the doc claims "plus self-loops".
- `lib.rs:444-454` — `unresolved_imports` doc promises "every relative import"
  but never scans `nested_imports`; typo'd relative imports inside functions
  are silently accepted.
- `lib.rs:381-383` — comment claims "progressively shorter prefixes" fallback
  that doesn't exist; `import pkg.sub.mod` creates no edges to ancestor
  packages (reachability survives only because every `__init__.py` is an
  entry).

### Engines
- `lib.rs:252-254` (core) — `inspect` matches by unanchored `ends_with`:
  inspecting `b.py` also returns `lib.py`'s findings. Verified. Same boundary
  bug in `hotspots.rs:36-39`, `coverage.rs:101-106`, `git.rs:152-159,200-204`
  (`"myapp.py".ends_with("app.py")`) — churn/coverage/changed-line data can be
  attributed to the wrong file, flipping introduced-vs-inherited in the PR
  gate; and latently in `mollify-lsp/src/lib.rs:156-160`.
- `fix.rs:57-73` — `fix --apply` rewrites all line endings (CRLF → LF whole
  file, verified) and a mid-loop I/O error aborts with earlier files already
  modified and the applied count lost.
- `git.rs:34-46,176` — non-ASCII filenames arrive octal-quoted from git
  (`core.quotePath`), so changed-file/churn matching silently fails for them;
  run git with `-c core.quotePath=off` (and `-z` where practical).
- `version.rs:21-41,65-92` — epochs are mis-parsed rather than "degraded to no
  match" as documented (`2!1.0` parses as `2`; verified
  `matches_spec("2!1.0", "<3.0") == true`), and `.post1`/`.dev1` compare equal
  to the final release (both violate PEP 440, feeding `Certain` supply-chain
  findings).
- `version.rs:163-185` — `specs_intersect`'s "sound finite sweep" misses gaps:
  `specs_intersect(">2.0", "<2.0.1") == false` (verified) — a false negative
  suppressing a vulnerable-range warning.
- `deps.rs:386-396` — URL/VCS requirement lines produce mangled declared names
  (`git+https://github-com/user/repo-git@v1-0`) plus a spurious
  missing-dependency for the real name; `#egg=` is destroyed by comment-strip.
- `deps.rs:218-221` — `unresolved-import` claims `Certain` for relative
  imports that resolve outside the .py graph (C extensions `._speedups`,
  build-generated `._version`) even inside `try/except ImportError`;
  miscalibrated for Certain-gated CI.
- `supplychain.rs:138-139` — unconstrained declared deps (bare `flask`) are
  never matched against advisories, even "all versions affected" ones —
  inconsistent with the module's own `specs_intersect("", "<2.0") == true`
  semantics.

### Frontends
- `main.rs:505-511,551-557` — `--save-baseline` prints a human line on stdout
  in `--format json` (JSON purity broken; verified).
- `main.rs:500-513` — `quality_score` is computed pre-filter while `summary`
  is recomputed post-filter (`--min-confidence certain` → score 95, total 0;
  verified): internally inconsistent envelope.
- `main.rs:750-917` — `--format sarif|github|junit` silently degrade to human
  output for `trace`/`inspect`/`list`/`metrics` (exit 0), breaking pipelines.
- `mollify-mcp/src/lib.rs:64-79` — `initialize` echoes any client
  `protocolVersion` (verified with `9999-99-99`) instead of responding with a
  version the server supports.
- `mollify-mcp/src/lib.rs:37-43` — malformed JSON gets no `-32700` response
  (client hangs); `:83` — missing `method` returns `-32601` instead of
  `-32600`.
- `mollify-mcp/src/lib.rs:250-254` — `mollify_fix` `apply:true` swallows I/O
  errors (`unwrap_or(0)`): agent sees `isError:false` for a failed mutating
  operation.
- `mollify-lsp/src/lib.rs:93` — unknown request methods get no response
  (verified dangling id; JSON-RPC requires `-32601`), clients hang.
- `mollify-lsp/src/lib.rs:66-94` — no `didClose` handling; stale diagnostics
  persist after a file is closed/deleted.
- `mollify-lsp/src/lib.rs:162-183` — diagnostic ranges: 1-based/UTF-8 column
  passed raw as 0-based/UTF-16 `character`, and `end` is always char 0 of
  `end_line` (reversed range once any engine emits a real column; latent
  today since all engines emit column 0).
- `mollify-lsp/src/lib.rs:270-272` — an unparseable `Content-Length` header
  terminates the server as if EOF, mid-session.
- `mollify-cli/src/osv.rs:49-63` — OSV `querybatch` pagination
  (`next_page_token`) unhandled; truncated advisory sets are cached to the
  offline DB as authoritative with `--refresh`.

### Types (public contract)
- `mollify-types/src/lib.rs:26,39,61,129` — contract enums lack
  `#[non_exhaustive]` and unknown-variant tolerance: adding a `Report`/
  `Category` variant is a breaking change for every consumer, and old
  consumers hard-fail on a new `kind` string — at odds with "additive minor
  bumps".
- `lib.rs:24-34` — `Confidence`/`Severity` `Ord` is load-bearing
  (`--min-confidence` filtering, Certain-only fix gate) but derive-order
  dependent, undocumented, and untested; an innocent variant reorder silently
  inverts the gates. Add a doc line + an ordering assertion test.
- Design gap (deadcode) — plain scripts using `if __name__ == "__main__":`
  are not entry points (`mollify-graph/src/lib.rs:294-303`), so script-driven
  projects report *everything* dead (verified). Worth an explicit heuristic
  (treat `__main__` guard as a root) or a documented limitation + config
  escape hatch.

### Docs, cookbook, scripts, CI

- `cookbook/recipes/06-map-your-codebase.md` (three blocks) and
  `02-anatomy-of-a-finding.md:18-24` — stale "real captured" outputs
  contradicting `cookbook/README.md:6`'s accuracy promise: `metrics` LOC/row
  counts and MI drifted, the mermaid graph is missing the
  `invoice --> ledger` edge, `trace` shows "imports (2)" vs actual 3, and
  `explain unused-export` gained a reachability-roots paragraph. (Recipes
  01/03/04/07 reproduce byte-exact.)
- `docs/ci-integration.md:54` — GitLab example feeds SARIF to
  `artifacts:reports:sast`, which expects GitLab's own schema; SARIF belongs
  under `artifacts:reports:sarif`. Copy-pasting the snippet yields a report
  that fails security-dashboard ingestion.
- `docs/configuration.md:145-150` — advisory-DB section says supply-chain "is
  an *input*, not a network call", contradicting README/usage/CHANGELOG/CLI
  (`--offline` exists because it's live by default). Only `mollify audit` is
  offline-only.
- `crates/mollify-core/Cargo.toml:8` — published crates.io description still
  says "dead-code and dependency-hygiene engines (more to come)" while the
  crate ships ten engines — the metadata does what invariant 4 forbids.
- `scripts/bump-version.sh:22,27` — GNU-only `sed -i` (no suffix arg) breaks
  on macOS/BSD.
- `.github/workflows/*.yml` — third-party actions pinned to mutable
  tags/branches (`dtolnay/rust-toolchain@stable`,
  `pypa/gh-action-pypi-publish@release/v1` in the `id-token: write` job,
  `maturin-action@v1`, `rust-cache@v2`, `audit-check@v2`) in workflows holding
  publish credentials (`CARGO_REGISTRY_TOKEN`, PyPI Trusted Publishing).
  SHA-pin and add Dependabot (`.github/dependabot.yml` doesn't exist).
  Otherwise CI is correct and least-privilege.
- `scripts/mollify-report.sh:36-38` — header promises "never fails the
  action" but the `jq` stages aren't guarded: invalid/partial JSON from a
  crashed `audit … || true` makes `set -e` exit 2, which Claude Code treats as
  a blocking hook error (this script is wired into
  `.claude/settings.json` PostToolUse/Stop).

---

## Nits

- `mollify-core/src/lib.rs:497-502` — `into_report` is dead code and maps every
  non-deps category to `Report::DeadCode` (latent invariant-3 violation).
- `explain.rs:160-164,263` — `policy-violation` is advertised but never
  emitted; real user-defined policy rule ids are unexplainable via
  `mollify explain`.
- `main.rs:610-684` — `supply-chain --format json` emits `kind: "security"`,
  indistinguishable from `mollify security` (no `SupplyChain` report variant).
- CLI/MCP hand-build duplicate `inspect`/`trace` JSON envelopes, neither with
  `schema_version` (invariant 3).
- `update_check.rs:138-150` — cache write can be torn by `process::exit`
  (read side tolerates it; atomic rename would fix).
- `main.rs:828-858` — `mollify inspect <typo>` prints "No findings. ✓", exit 0,
  while `trace` correctly errors — inconsistent.
- `mollify-parse/src/lib.rs:1956` — `# mollify: ignore[rule] -- reason`
  (trailing text) silently fails to parse; suppression ignored.
- `mollify-parse/src/lib.rs:1918` — `security_imports` skips `nested_imports`
  (weak-cipher import inside a function never flagged).
- `mollify-parse/src/lib.rs:214` — public `name_counts` is std `HashMap`
  (RandomState), contra invariant 1's FxHashMap guidance (safe today — lookups
  only — but an unordered map in the public surface invites breakage).
- `mollify-parse/src/lib.rs:524` — chained/tuple assignments produce no
  `Definition` (false-negative direction only).
- `mollify-types/src/lib.rs:74-83` — `column` documented 1-based but uses 0 as
  a skip-serialized "absent" sentinel; `Option<u32>` would be honest.
- `mollify-types` — `schema_version` is a free-form `String` stamped by hand at
  every construction site; a constructor would prevent drift. Non-finite `f64`
  in `MetricsReport` wouldn't round-trip (currently unreachable).
- `mollify-graph/src/lib.rs:493` — "BFS" comment on a LIFO worklist (DFS).
- `mollify-core/src/dupes.rs:243-246,288-307` — dead `|| b[i] == b'x'`
  condition; unterminated single-quoted string double-counts its newline
  (line drift on malformed input only).
- `known.rs:154` — `("markdown", "markdown")` identity alias is dead weight.
- `.mcp.json:7` (and the embedded copy) — sets `MOLLIFY_LOG=error`, but no
  code reads it (only `MOLLIFY_UPDATE_CHECK`/`DO_NOT_TRACK` exist).
- `.agents/skills/mollify/references/cli-reference.md:99` (same line in
  `.claude/…`, `.devin/…`, and the three embedded copies) — garbled sentence
  referencing a nonexistent `references/configuration.md`.
- `cookbook/recipes/05-ci-gate.md:74,82` — older action versions
  (`checkout@v4`, `upload-sarif@v3`) than the rest of the repo (`@v5`/`@v4`).
- `crates/mollify-core/Cargo.toml:27-28`, `crates/mollify-mcp/Cargo.toml:15-16`
  — `serde_json` redundantly listed in both `[dependencies]` and
  `[dev-dependencies]`.
- `README.md:198-206`, `cookbook/recipes/07-json-and-agents.md:13-25` — the
  "JSON contract" examples omit `Action.description`, which is non-optional
  and always serialized.
- `.gemini/commands/mollify/audit.toml` — tells the agent to group findings
  "by `kind`"; findings carry `rule`/`category` (`kind` is the envelope
  discriminator). The `.claude` commands get this right.

---

## Verified clean

Worth recording what was checked and held up:

- **Determinism at the order level**: all emit paths sort before serializing
  (`sort_findings` by path/line/rule/fingerprint; graph edges/SCCs/unresolved
  sorted+deduped; SARIF rules via BTreeSet). Two identical runs are
  byte-identical (verified end-to-end). The remaining determinism issues are
  *content*-level (M9, M10, X1).
- **Tarjan SCC** (iterative): index/lowlink/root logic correct; 3000-node
  cycle chain → one SCC, no stack overflow; overlapping 2-cycles merge
  correctly.
- **suffix.rs** (SA-IS + Kasai): cross-checked against a naive implementation
  over 400 randomized inputs; sentinel/boundary math correct.
- **Candidate/verifier separation**: `plan()` filters exactly
  `Certain` + `auto_fixable`; `auto_fixable == (confidence == Certain)`
  everywhere it's set. The two criticals above are the only effective
  breaches (via miscalibrated `Certain`).
- Line-number math is 1-based and correct with multi-byte UTF-8 (verified
  empirically); `end_line1` is panic-safe; no unguarded panics found in parse;
  adversarial `.ipynb` input degrades to skipping the file.
- Update check is well-gated (TTY-only, `MOLLIFY_UPDATE_CHECK=off`,
  `DO_NOT_TRACK`, CI detection, stderr-only).
- JSON stdout purity holds in normal `--format json` runs (all status chatter
  on stderr) — except the `--save-baseline` case above.
- `config.rs` rule-over-category precedence, `baseline.rs` set logic,
  `members.rs`, `cohesion.rs` (Henderson-Sellers with `m ≥ 3` guard),
  `paths.rs`, `trace.rs`, `plugins.rs`, `agents.rs`, `sarif.rs` (valid 2.1.0):
  no findings.
- **Versions**: workspace, all six internal path-dep constraints, Cargo.lock,
  and CHANGELOG all agree on 0.1.3; pyproject takes its version dynamically
  from the CLI crate; `check-versions.sh` passes and CI enforces it.
- **Agent-asset sync**: `diff -r` of all seven root agent trees plus
  `.mcp.json`/`GEMINI.md`/`AGENTS.md`/`mollify-report.sh` against
  `crates/mollify-core/assets/` is byte-identical, and a test enforces it.
- **Docs claims spot-checked true**: subcommand and MCP-tool counts, exit-code
  model, flag/enum inventories, duplication defaults, rule lists across all
  agent surfaces; cookbook recipes 01/03/04/07 reproduce byte-exact.
- **Packaging**: maturin `bindings = "bin"` config is coherent; the release
  workflow uses OIDC Trusted Publishing (no long-lived PyPI secrets);
  crates-release publishes in dependency order.
