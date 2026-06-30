# ADR-0003: Nested (lazy) import model — reachability/deps yes, architecture no

- **Status:** Accepted (2026-06-30).
- **Context:** The same Birefringence audit (`docs/birefringenceaudit.md`) showed
  dependencies imported **lazily inside a function body** (e.g. `uvicorn`
  imported inside `main()`) being reported `unused-dependency`, because the parser
  only collected **top-level** imports (`scan_top_level` recurses into `if`/`try`
  but not function/class bodies). Fixing that means collecting nested imports —
  but nested imports must be fed to the right consumers, and *only* those.

  The danger: `ModuleGraph` keeps a single resolved-edge set consumed by both
  reachability (`compute_reachability`) **and** architecture
  (`find_cycles` → `circular-dependency`; `import_edges` → `layer-violation`,
  `forbidden-import`, contracts). Deferring an import into a function body is the
  **canonical way to break an import cycle** — the `circular-dependency` remedy
  text literally says *"defer one import into function scope."* If lazy imports
  fed the arch edge set, mollify would flag code that applied its own fix, and
  broaden layer/contract violations the same way.

- **Decision:**
  - The parser collects function/class-body imports into a separate
    `ParsedModule.nested_imports`, distinct from top-level `imports`
    (`crates/mollify-parse/src/lib.rs`, `NestedImportVisitor`).
  - The graph keeps **two** edge lists: `edges` (top-level) and `lazy_edges`
    (nested). `resolve_edges` routes each import to the right list while keeping
    `imported_symbols` populated from **both** (a lazy `from helper import go`
    still means `helper.go` is used).
  - **Consumers:**
    - `compute_reachability` walks `edges` **+** `lazy_edges` — a lazily imported
      module is still loaded at runtime, just deferred.
    - `find_cycles()` / `import_edges()` read `edges` **only** → cycle, layer,
      and contract checks never see lazy imports.
    - Dependency-usage (`deps.rs` `used_distributions`, `module_imported_dists`)
      reads `m.parsed.nested_imports` **directly** (independent of graph edges),
      so lazy imports count as usage.
  - **Module-scope `unused-import` is unchanged** — it still considers only
    top-level `imports`, so a function-local import used only in that function is
    not mis-judged against module-wide usage.

- **Consequences:**
  - ✅ Lazy deps no longer false-positive as `unused-dependency`; lazily imported
    internal modules stay reachable.
  - ✅ The cycle-breaker pattern (A top-imports B; B lazy-imports A) is not
    flagged `circular-dependency`; deliberate lazy cross-boundary imports don't
    trip `layer-violation`/contracts. Covered by
    `lazy_import_does_not_create_arch_cycle` (`mollify-graph`) and
    `lazy_cross_layer_import_not_layer_violation` (`mollify-core`).
  - ⚠️ **Class-body imports** execute at *import* time, not lazily, yet are
    collected as `nested_imports` (the visitor increments depth on `ClassDef`
    too) and thus excluded from arch. This is a deliberate simplification: a
    class-body import cycle is rare, and missing it is a tolerable false
    *negative* — far preferable to the false *positive* of flagging the
    function-body cycle-breaker. Revisit with function-vs-class depth tracking
    only if a real case appears.
