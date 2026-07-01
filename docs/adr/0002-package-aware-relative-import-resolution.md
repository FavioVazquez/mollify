# ADR-0002: Package-aware relative-import resolution

- **Status:** Accepted (2026-06-30).
- **Context:** A real-world audit (running mollify v0.1.2 on an external Python
  package) showed that **every** relative
  import in a package `__init__.py` (`from .aa import x`, `from . import bb`) was
  reported `unresolved-import`, which cascaded into `unused-file` for the
  submodules and `unused-export` for their public symbols — roughly 200 of the
  384 findings, the dominant false-positive source.

  Root cause: `dotted_name` strips the `/__init__` suffix, so a package's
  `__init__.py` carries the **package's own** dotted name (`pkg`, not
  `pkg.__init__`). But `resolve_relative` treated every importer as a *module*
  and dropped `dots` trailing segments — correct for `a/b/c.py` (one dot →
  package `a.b`), but one segment too many for `pkg/__init__.py` (one dot should
  be `pkg` itself, not its parent).

- **Decision:**
  1. **Package-aware resolution.** `ModuleInfo` carries `is_package`
     (`path.file_name() == "__init__.py"`). `resolve_relative` drops `dots - 1`
     segments for a package and `dots` for a module. So `.aa` in `pkg/__init__.py`
     resolves to `pkg.aa`; `..x` in `pkg/sub/__init__.py` resolves to `pkg.x`.
  2. **Unconditional submodule resolution.** `from . import bb` resolves its
     target to the package itself; the `from pkg import submod` fallback
     (`{target}.{name}` lookup) now runs **even when the package target
     resolves**, so the edge to `pkg.bb` is recorded.
  3. **Self-edge skip.** Because `from . import bb` resolves the *direct* target
     to the importing package, the importer→self edge is dropped (an
     `__init__.py` importing its own submodules is not a self-cycle).
  (`crates/mollify-graph/src/lib.rs`: `resolve_relative`, `resolve_edges`,
  `unresolved_imports`, `ModuleInfo::is_package`.)

- **Consequences:**
  - ✅ Re-exporting packages no longer emit phantom `unresolved-import` /
    `unused-file` / `unused-export`; submodules become reachable through their
    `__init__.py` surface.
  - ✅ Surfaces genuine cycles the old bug hid: the cookbook sample's
    `invoice ↔ ledger` cycle (both top-level imports) is now correctly detected
    — a fixed false *negative*.
  - ⚠️ Module-vs-package is determined purely by filename (`__init__.py`);
    namespace packages without an `__init__.py` are still treated as plain dirs
    (acceptable — they have no re-export surface to resolve against).
  - Covered by `relative_import_resolution_from_package_init`,
    `package_init_reexports_resolve` (`mollify-graph`).
