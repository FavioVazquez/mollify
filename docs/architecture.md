# Architecture

Mollify is a Cargo workspace. Data flows in one direction: parse → graph →
engines → report.

```
files ──▶ mollify-parse ──▶ mollify-graph ──▶ mollify-core ──▶ mollify-types
         (tree-sitter)      (modules,         (engines)        (JSON contract)
                             import edges,        │
                             reachability)        ├─ deadcode
                                                  ├─ deps
                                                  ├─ arch (cycles)
                                                  ├─ complexity
                                                  ├─ dupes
                                                  ├─ plugins (framework decorators)
                                                  ├─ config (.mollifyrc)
                                                  ├─ git (gate attribution)
                                                  ├─ sarif / fix
                                                  └─ fingerprint
                                                       │
                            mollify-cli  ◀────────────┤
                            mollify-mcp  ◀────────────┘
```

## Crates

| Crate | Responsibility |
|---|---|
| **mollify-types** | The serde **contract**: `Report` (kind-discriminated), `Finding`, `Confidence`, `Severity`, `Category`, `Attribution`, `Summary`, deterministic `sort_findings`. The public API surface — clients depend on the JSON shape, not on internals. |
| **mollify-parse** | Python parsing via `tree-sitter-python` behind parser-agnostic types (`ParsedModule`, `Definition`, `Import`, `FunctionComplexity`). Extracts defs, imports, `__all__`, decorators, used-name counts, dynamic sinks, and per-function complexity. See [ADR-0001](adr/0001-parser-tree-sitter.md). |
| **mollify-graph** | Discovery (`.gitignore`-aware), **path-sorted stable FileIds**, dotted-name + relative-import resolution, internal import edges, **BFS reachability** from entry points, symbol-usage queries, and **Tarjan cycle detection**. |
| **mollify-core** | The engines (`deadcode`, `deps`, `arch`, `complexity`, `dupes`), framework `plugins`, `config`, `git` gate, `sarif`, `fix`, and `fingerprint`. Assembles `Report` envelopes. |
| **mollify-cli** | The `mollify` binary (clap). |
| **mollify-mcp** | The MCP stdio server (`mollify mcp`) — one server, many agent front-ends. |

## Invariants (non-negotiable)

1. **Determinism** — identical input → byte-identical output. Findings are sorted
   before serialization; FileIds are path-sorted; maps that reach output are
   ordered.
2. **Candidate / verifier separation** — Mollify emits evidence; only
   `certain` + `auto_fixable` findings may be auto-applied.
3. **Versioned, `kind`-discriminated output** — `schema_version` is pinned by
   agent skills.
4. **Five co-equal analysis areas** — dead code, duplication, circular deps,
   complexity, architecture (+ dependency hygiene). Not "a dead-code tool".
5. **Evidence-preserving** — every finding carries a fingerprint, confidence,
   and human reason.

## How dead-code reachability works

1. Discover `*.py`; assign path-sorted FileIds.
2. Resolve imports to internal modules → directed edges.
3. Seed entry points (`__main__`/`__init__`/`conftest`/`test_*`/`setup.py`).
4. BFS mark-reachable → unreachable non-entry modules are `unused-file`.
5. A top-level symbol is **used** if it's referenced more times than it's
   defined (internal), imported by name, referenced by an importer, listed in
   `__all__`, or registered by a framework decorator (`plugins`). Otherwise it's
   `unused-export`, tiered by confidence.

Python dead-code detection is undecidable in general, which is why every verdict
is tiered, never boolean.

## Known simplifications (vs the plan / fallow)

Tracked in [STATUS.md](STATUS.md): tree-sitter instead of the ruff AST
(ADR-0001), file-level (not line-level) gate attribution, Rabin-Karp duplication
(SA-IS+LCP is the upgrade), and name-table-assisted symbol usage rather than full
scope/binding resolution.
