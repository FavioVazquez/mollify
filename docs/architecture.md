# Architecture

Mollify is a Cargo workspace. Data flows in one direction: parse → graph →
engines → report.

```
files ──▶ mollify-parse ──▶ mollify-graph ──▶ mollify-core ──▶ mollify-types
         (ruff AST)         (modules,         (engines)        (JSON contract)
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
                            mollify-mcp  ◀────────────┤
                            mollify-lsp  ◀────────────┘
```

## Crates

| Crate | Responsibility |
|---|---|
| **mollify-types** | The serde **contract**: `Report` (kind-discriminated), `Finding`, `Confidence`, `Severity`, `Category`, `Attribution`, `Summary`, deterministic `sort_findings`. The public API surface — clients depend on the JSON shape, not on internals. |
| **mollify-parse** | Python parsing via Astral's `ruff_python_parser` / `ruff_python_ast` (crates.io, pinned) behind parser-agnostic types (`ParsedModule`, `Definition`, `Import`, `FunctionComplexity`). Extracts defs, imports, `__all__`, decorators, used-name counts, dynamic sinks, per-function complexity, and **scope/binding resolution** (`module_used`). See [ADR-0001](adr/0001-parser-foundation.md). |
| **mollify-graph** | Discovery (`.gitignore`-aware, and always pruning VCS/virtualenv/build/cache directories — see `discover_python_files`), **path-sorted stable FileIds**, dotted-name + relative-import resolution, internal import edges, **BFS reachability** from entry points, symbol-usage queries, and **Tarjan cycle detection**. |
| **mollify-core** | The engines (`deadcode` + `members` for class/enum members, `deps`, `arch`, `complexity`, `hotspots`, `dupes`, `security`, `typehealth` + `apihygiene` for private-type leaks, `commented`, `cohesion`, `coverage`, `supply-chain`, `policy`), framework `plugins`, `config`, `git` gate, `sarif`, `fix`, and `fingerprint`. Assembles `Report` envelopes. Every engine runs under `catch_unwind`: a panicking engine degrades to a single `engine-panic` finding (severity `error`) instead of killing the report. |
| **mollify-cli** | The `mollify` binary (clap). |
| **mollify-mcp** | The MCP stdio server (`mollify mcp`) — one server, many agent front-ends. |
| **mollify-lsp** | The Language Server (`mollify lsp`, stdio) — publishes mollify diagnostics on open/save, reusing the deterministic audit so editor results match CI. |

## Invariants (non-negotiable)

1. **Determinism** — identical input → byte-identical output. Findings are sorted
   before serialization; FileIds are path-sorted; maps that reach output are
   ordered. Every serialized `location.path` is root-relative with `/`
   separators on every OS, so reports, `.mollifyrc` patterns, and fingerprint
   baselines are portable between Linux, macOS, and Windows (CI pins this with
   a cross-OS golden fingerprint set for the sample project).
2. **Candidate / verifier separation** — Mollify emits evidence; only
   `certain` + `auto_fixable` findings may be auto-applied.
3. **Versioned, `kind`-discriminated output** — `schema_version` is pinned by
   agent skills.
4. **Eight co-equal analysis areas** — dead code, duplication, circular deps,
   complexity, architecture, dependency hygiene, type health, and security
   (the `Category` enum in `mollify-types` is the authoritative list). Not
   "a dead-code tool".
5. **Evidence-preserving** — every finding carries a fingerprint, confidence,
   and human reason.

## How dead-code reachability works

1. Discover `*.py`; assign path-sorted FileIds.
2. Resolve imports to internal modules → directed edges. Resolution is
   package-aware (a package `__init__.py`'s relative re-exports resolve against
   the package itself), and lazy imports inside function/class bodies also create
   edges.
3. Seed entry points (`__main__`/`__init__`/`conftest`/`test_*`/`setup.py`, plus
   the module half of each `[project.scripts]` console-script entry point).
4. BFS mark-reachable → unreachable non-entry modules are `unused-file`.
5. A top-level symbol is **used** if a resolved free `Name` load binds to it
   (scope/binding resolution — ignoring shadowing locals and attribute accesses),
   imported by name, referenced by an importer, listed in `__all__`, registered
   by a framework decorator (`plugins`), a pytest `test_*`/`Test*` collection
   root in a test path, or the function named by a `[project.scripts]` entry
   point. Otherwise it's `unused-export`, tiered by confidence. (Modules with a
   dynamic sink fall back to a conservative token-frequency check.)
6. Beyond reachability, the `members` engine flags **`unused-method`** /
   **`unused-attribute`** (class internals, with framework/property/dunder/
   dataclass/ABC awareness) and **`unused-enum-member`**; the parser flags
   **`unreachable-code`** (statements after an unconditional terminator); and
   `deadcode` flags **`duplicate-export`** (a barrel `__init__.py` re-exporting
   the same name from two modules).

Python dead-code detection is undecidable in general, which is why every verdict
is tiered, never boolean.

## Implementation notes

Full-fidelity **ruff AST** parser (ADR-0001); exact duplication via a linear-time
**SA-IS suffix array + LCP**; and real **scope/binding resolution** (LEGB,
`Load`/`Store` aware) for precise symbol usage. Supply-chain matches pinned
versions precisely and resolves declared ranges via the installed environment or
range-intersection. The only remaining roadmap item is a Salsa
keystroke-incremental reparse for the LSP (a performance optimization).
