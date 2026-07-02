//! # mollify-graph
//!
//! Discovers Python modules, assigns **path-sorted stable FileIds** (ADR-004
//! analog), builds the internal import graph, computes **reachability** from
//! entry points, and answers symbol-usage queries. Pure structure — the
//! `mollify-core` crate turns these into [`mollify_parse`]-backed findings.

use camino::{Utf8Path, Utf8PathBuf};
use mollify_parse::{Import, ParsedModule, PyParser};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

/// Stable, path-sorted module identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

/// One module node in the graph.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub id: FileId,
    pub path: Utf8PathBuf,
    /// Path relative to the analysis root. This is the module's **stable
    /// identity** for fingerprints and baselines: unlike `path`, it does not
    /// vary with how the root was spelled (`.` vs an absolute path) or where
    /// the checkout lives on disk.
    pub rel: Utf8PathBuf,
    /// Dotted module name relative to its source root (e.g. `pkg.sub.mod`).
    pub dotted: String,
    pub parsed: ParsedModule,
    /// True if this module is an analysis root (entry point).
    pub is_entry: bool,
    /// True if this module is a package surface (`__init__.py`). Its `dotted`
    /// name is the package itself, so relative imports resolve against it
    /// directly (a leading `.` = this package, not its parent).
    pub is_package: bool,
}

/// An import that looks first-party/relative but resolves to no module.
#[derive(Debug, Clone)]
pub struct UnresolvedImport {
    /// The importing module's file path.
    pub importer: Utf8PathBuf,
    /// The importing module's root-relative path (stable fingerprint identity).
    pub importer_rel: Utf8PathBuf,
    /// The import as written (`.sub.thing` for relative, `pkg.mod` for absolute).
    pub display: String,
    pub line: u32,
    /// True for a relative import (`from . import x`) — these must be internal.
    pub relative: bool,
}

/// The whole project graph.
pub struct ModuleGraph {
    pub modules: Vec<ModuleInfo>,
    by_dotted: FxHashMap<String, FileId>,
    /// Resolved internal import edges from **top-level** imports: importer →
    /// imported. These feed both reachability *and* architecture analysis
    /// (cycles, layers, contracts).
    edges: Vec<(FileId, FileId)>,
    /// Edges from **lazy** imports nested in function/class bodies. These feed
    /// reachability *only* — deferring an import into a function body is the
    /// canonical way to break an import cycle, so they must not count toward
    /// `circular-dependency` / `layer-violation` / contract checks.
    lazy_edges: Vec<(FileId, FileId)>,
    /// For each imported module, the set of symbol names pulled in by importers,
    /// keyed by the imported module's FileId.
    imported_symbols: FxHashMap<FileId, FxHashSet<String>>,
    reachable: FxHashSet<FileId>,
    /// True if any module in the project has a dynamic dispatch/import sink.
    pub global_dynamic: bool,
}

/// Directory names never descended into, regardless of `.gitignore` — VCS
/// metadata, virtualenvs, and build/cache output. Mirrors `ruff`'s default
/// exclude list, since projects in this ecosystem already expect these names
/// to be skipped without configuration.
const DEFAULT_EXCLUDE_DIRS: &[&str] = &[
    ".bzr",
    ".direnv",
    ".eggs",
    ".git",
    ".hg",
    ".svn",
    ".ipynb_checkpoints",
    ".mypy_cache",
    ".nox",
    ".pyenv",
    ".pytest_cache",
    ".pytype",
    ".ruff_cache",
    ".tox",
    ".venv",
    "__pycache__",
    "__pypackages__",
    "_build",
    "buck-out",
    "build",
    "dist",
    "env",
    "node_modules",
    "site-packages",
    "venv",
];

/// True if `entry` is a directory that should be pruned from discovery: a
/// builtin/extra denylisted name, or any directory directly containing a
/// `pyvenv.cfg` (a virtualenv marker that catches custom-named venvs the name
/// denylist can't anticipate). `includes` overrides both the builtin and
/// extra denylists (but not the `pyvenv.cfg` check, since a directory the user
/// explicitly asked to include is never an accidental virtualenv).
fn is_excluded_dir(
    entry: &ignore::DirEntry,
    extra_excludes: &[String],
    includes: &[String],
) -> bool {
    if !entry.file_type().is_some_and(|t| t.is_dir()) {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    // Check the pyvenv.cfg guard first: it always wins, even for a
    // directory the user explicitly --include'd.
    if entry.path().join("pyvenv.cfg").is_file() {
        return true;
    }
    if includes.iter().any(|i| i == name) {
        return false;
    }
    DEFAULT_EXCLUDE_DIRS.contains(&name) || extra_excludes.iter().any(|e| e == name)
}

/// Walk `root` for `*.py` and `*.ipynb` files, honoring `.gitignore` and
/// pruning [`DEFAULT_EXCLUDE_DIRS`] (plus any virtualenv detected via
/// `pyvenv.cfg`). Deterministic order.
pub fn discover_python_files(root: &Utf8Path) -> Vec<Utf8PathBuf> {
    discover_python_files_with(root, &[], &[])
}

/// Like [`discover_python_files`], but also prunes `extra_excludes` directory
/// names (in addition to the builtin denylist) — used to honor a project's
/// `.mollifyrc.json` `exclude_dirs`.
pub fn discover_python_files_excluding(
    root: &Utf8Path,
    extra_excludes: &[String],
) -> Vec<Utf8PathBuf> {
    discover_python_files_with(root, extra_excludes, &[])
}

/// Like [`discover_python_files_excluding`], but `includes` directory names
/// bypass both the builtin denylist and `extra_excludes`, *and* override
/// `.gitignore` for that directory — used to honor a user's `--include`
/// override of the default/configured/VCS exclusions.
///
/// `ignore::WalkBuilder`'s own override mechanism can't express "un-ignore
/// just this one directory, leave `.gitignore` in force everywhere else":
/// once any non-negated override glob is registered, every file that fails
/// to match it is force-excluded (see `ignore::overrides::Override::matched`),
/// which would silently drop the rest of the project. So instead this runs a
/// second, separate walk per included directory with `.gitignore` checking
/// turned off entirely (scoped to that directory's subtree only); the
/// [`is_excluded_dir`] builtin-denylist/`pyvenv.cfg` guard still applies
/// inside it. Results from both walks are merged and deduplicated.
pub fn discover_python_files_with(
    root: &Utf8Path,
    extra_excludes: &[String],
    includes: &[String],
) -> Vec<Utf8PathBuf> {
    let mut out = Vec::new();
    collect_py_files(
        &mut out,
        walk_builder(root, extra_excludes, includes, true).build(),
    );
    for dir in find_dirs_named(root, includes, extra_excludes) {
        collect_py_files(
            &mut out,
            walk_builder(&dir, extra_excludes, includes, false).build(),
        );
    }
    out.sort();
    out.dedup();
    out
}

fn walk_builder(
    root: &Utf8Path,
    extra_excludes: &[String],
    includes: &[String],
    honor_gitignore: bool,
) -> ignore::WalkBuilder {
    let extra = extra_excludes.to_vec();
    let inc = includes.to_vec();
    let mut wb = ignore::WalkBuilder::new(root);
    wb.hidden(false);
    if !honor_gitignore {
        wb.git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false);
    }
    wb.filter_entry(move |e| !is_excluded_dir(e, &extra, &inc));
    wb
}

fn collect_py_files(out: &mut Vec<Utf8PathBuf>, walk: ignore::Walk) {
    for entry in walk.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "py" || e == "ipynb") {
            if let Ok(u) = Utf8PathBuf::from_path_buf(p.to_path_buf()) {
                out.push(u);
            }
        }
    }
}

/// Find every directory under `root` whose name is in `names`, walking with
/// `.gitignore` disabled (so a gitignored match is still found) but the
/// builtin denylist/`pyvenv.cfg` guard still pruning everything else.
fn find_dirs_named(
    root: &Utf8Path,
    names: &[String],
    extra_excludes: &[String],
) -> Vec<Utf8PathBuf> {
    if names.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for entry in walk_builder(root, extra_excludes, names, false)
        .build()
        .flatten()
    {
        if entry.depth() == 0 || !entry.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        if names.iter().any(|n| n == name) {
            if let Ok(u) = Utf8PathBuf::from_path_buf(entry.path().to_path_buf()) {
                found.push(u);
            }
        }
    }
    found
}

/// Read a module's source. For `.ipynb`, extract and concatenate code cells into
/// one Python source (line numbers are relative to that concatenation — a
/// documented v1 simplification). Jupyter magics/shell-escapes are skipped.
pub fn read_source(path: &Utf8Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    if path.extension() != Some("ipynb") {
        return Some(raw);
    }
    let nb: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let cells = nb.get("cells")?.as_array()?;
    let mut src = String::new();
    for cell in cells {
        if cell.get("cell_type").and_then(|t| t.as_str()) != Some("code") {
            continue;
        }
        match cell.get("source") {
            Some(serde_json::Value::Array(lines)) => {
                for l in lines {
                    if let Some(s) = l.as_str() {
                        let t = s.trim_start();
                        if t.starts_with('%') || t.starts_with('!') {
                            continue;
                        }
                        src.push_str(s);
                    }
                }
                src.push('\n');
            }
            Some(serde_json::Value::String(s)) => {
                src.push_str(s);
                src.push('\n');
            }
            _ => {}
        }
    }
    Some(src)
}

/// Compute a module's dotted name relative to a source root. `src/` is treated
/// as a source root if present; otherwise the project root is used.
fn dotted_name(root: &Utf8Path, path: &Utf8Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut rel = rel.to_owned();
    // src-layout: drop a leading `src/` segment.
    if rel.starts_with("src") {
        if let Ok(stripped) = rel.strip_prefix("src") {
            rel = stripped.to_owned();
        }
    }
    let no_ext = rel.as_str().strip_suffix(".py").unwrap_or(rel.as_str());
    // A root-level `__init__.py` has the bare name `__init__` (no slash):
    // the analysis root is itself a package, whose dotted name is "".
    let no_init = no_ext
        .strip_suffix("/__init__")
        .unwrap_or(if no_ext == "__init__" { "" } else { no_ext });
    no_init.replace('/', ".").trim_matches('.').to_string()
}

fn is_entry(path: &Utf8Path) -> bool {
    let name = path.file_name().unwrap_or("");
    name == "__main__.py"
        || name == "conftest.py"
        || name == "setup.py"
        || name == "__init__.py" // package surface is a public root
        || name.starts_with("test_")
        || name.ends_with("_test.py")
        || path.extension() == Some("ipynb")
}

impl ModuleGraph {
    /// Parse all files (in parallel) and build the graph.
    pub fn build(root: &Utf8Path, files: &[Utf8PathBuf]) -> Self {
        // Parse in parallel; each rayon task gets its own parser.
        let parsed: Vec<(Utf8PathBuf, ParsedModule)> = files
            .par_iter()
            .filter_map(|p| {
                let src = read_source(p)?;
                let mut parser = PyParser::new().ok()?;
                let pm = parser.parse(p, &src).ok()?;
                Some((p.clone(), pm))
            })
            .collect();

        // Stable FileIds by sorted path (already sorted from discovery, but be safe).
        let mut parsed = parsed;
        parsed.sort_by(|a, b| a.0.cmp(&b.0));

        let mut modules = Vec::with_capacity(parsed.len());
        let mut by_dotted = FxHashMap::default();
        let mut global_dynamic = false;
        for (i, (path, pm)) in parsed.into_iter().enumerate() {
            let id = FileId(i as u32);
            let dotted = dotted_name(root, &path);
            global_dynamic |= pm.has_dynamic_sink;
            by_dotted.entry(dotted.clone()).or_insert(id);
            let is_package = path.file_name() == Some("__init__.py");
            let rel = path
                .strip_prefix(root)
                .map(Utf8Path::to_path_buf)
                .unwrap_or_else(|_| path.clone());
            // A module with an `if __name__ == "__main__":` guard is a
            // runnable script — a reachability root even with no importer.
            let is_entry = is_entry(&path) || pm.has_main_guard;
            modules.push(ModuleInfo {
                id,
                is_entry,
                is_package,
                rel,
                path,
                dotted,
                parsed: pm,
            });
        }

        let mut g = ModuleGraph {
            modules,
            by_dotted,
            edges: Vec::new(),
            lazy_edges: Vec::new(),
            imported_symbols: FxHashMap::default(),
            reachable: FxHashSet::default(),
            global_dynamic,
        };
        g.resolve_edges();
        g.compute_reachability();
        g
    }

    fn module(&self, id: FileId) -> &ModuleInfo {
        &self.modules[id.0 as usize]
    }

    /// Resolve each import to an internal module if possible, recording edges
    /// and which symbol names each importer pulls from the target.
    fn resolve_edges(&mut self) {
        let mut edges = Vec::new();
        let mut lazy_edges = Vec::new();
        let mut imported_symbols: FxHashMap<FileId, FxHashSet<String>> = FxHashMap::default();

        for m in &self.modules {
            // Top-level imports feed both reachability and architecture; lazy
            // (in-function/class-body) imports feed reachability only — they are
            // collected into `lazy_edges` so cycle/layer/contract checks never
            // see them (deferring an import is the canonical cycle-breaker).
            let top = m.parsed.imports.iter().map(|i| (i, false));
            let lazy = m.parsed.nested_imports.iter().map(|i| (i, true));
            for (imp, is_lazy) in top.chain(lazy) {
                let target_dotted = if imp.relative_dots > 0 {
                    resolve_relative(&m.dotted, imp.relative_dots, &imp.module, m.is_package)
                } else {
                    imp.module.clone()
                };
                let sink = if is_lazy { &mut lazy_edges } else { &mut edges };
                // Try the full dotted path, then progressively shorter prefixes
                // (handles `from pkg.mod import name` where `pkg.mod` is a module
                // and `name` is a symbol, vs `import pkg.mod`).
                if let Some(&tid) = self.lookup(&target_dotted) {
                    // Skip self-references: `from . import x` in a package's own
                    // `__init__.py` resolves the target to the package itself —
                    // not a real edge (and a spurious self-cycle otherwise).
                    if tid != m.id {
                        sink.push((m.id, tid));
                    }
                    // Symbol-level "used" tracking sees lazy imports too: a lazy
                    // `from helper import go` still means `helper.go` is used.
                    let set = imported_symbols.entry(tid).or_default();
                    for n in &imp.names {
                        set.insert(n.clone());
                    }
                    if imp.is_star {
                        set.insert("*".into());
                    }
                }
                // `from pkg import submod` where `submod` is itself a module.
                // Runs even when `target_dotted` resolves (a package surface), so
                // `from . import bb` in `pkg/__init__.py` reaches `pkg.bb`.
                if !imp.names.is_empty() {
                    for n in &imp.names {
                        let candidate = join_dotted(&target_dotted, n);
                        if let Some(&tid) = self.lookup(&candidate) {
                            if tid != m.id {
                                sink.push((m.id, tid));
                            }
                        }
                    }
                }
            }
        }
        edges.sort();
        edges.dedup();
        lazy_edges.sort();
        lazy_edges.dedup();
        self.edges = edges;
        self.lazy_edges = lazy_edges;
        self.imported_symbols = imported_symbols;
    }

    fn lookup(&self, dotted: &str) -> Option<&FileId> {
        self.by_dotted.get(dotted)
    }

    /// True if an import target resolves to an internal module — directly, or as
    /// `from pkg import submodule` where `pkg.submodule` is a module.
    fn import_resolves(&self, target: &str, imp: &Import) -> bool {
        if self.lookup(target).is_some() {
            return true;
        }
        imp.names
            .iter()
            .any(|n| self.lookup(&join_dotted(target, n)).is_some())
    }

    /// Imports that *look* internal but resolve to no module in the project:
    /// every relative import that fails to resolve, plus absolute imports under a
    /// first-party top-level package that fail to resolve. These are typically a
    /// typo or a broken refactor — distinct from third-party `missing-dependency`.
    pub fn unresolved_imports(&self) -> Vec<UnresolvedImport> {
        // First-party top-level package names (the first segment of any module).
        let mut first_party: FxHashSet<&str> = FxHashSet::default();
        for k in self.by_dotted.keys() {
            if let Some(top) = k.split('.').next() {
                first_party.insert(top);
            }
        }
        let mut out = Vec::new();
        for m in &self.modules {
            // Lazy (in-function) imports resolve by the same rules — a typo'd
            // relative import inside a function is just as broken at call time.
            for imp in m.parsed.imports.iter().chain(&m.parsed.nested_imports) {
                let relative = imp.relative_dots > 0;
                let target = if relative {
                    resolve_relative(&m.dotted, imp.relative_dots, &imp.module, m.is_package)
                } else {
                    imp.module.clone()
                };
                if target.is_empty() || self.import_resolves(&target, imp) {
                    continue;
                }
                if !relative {
                    let top = target.split('.').next().unwrap_or(&target);
                    if !first_party.contains(top) {
                        continue; // third-party → handled by dependency hygiene
                    }
                }
                let display = if relative {
                    format!("{}{}", ".".repeat(imp.relative_dots as usize), imp.module)
                } else {
                    imp.module.clone()
                };
                out.push(UnresolvedImport {
                    importer: m.path.clone(),
                    importer_rel: m.rel.clone(),
                    display,
                    line: imp.line,
                    relative,
                });
            }
        }
        out.sort_by(|a, b| {
            a.importer
                .cmp(&b.importer)
                .then(a.line.cmp(&b.line))
                .then(a.display.cmp(&b.display))
        });
        out.dedup_by(|a, b| a.importer == b.importer && a.line == b.line && a.display == b.display);
        out
    }

    /// BFS mark-reachable from all entry modules over import edges.
    fn compute_reachability(&mut self) {
        let mut adj: FxHashMap<FileId, Vec<FileId>> = FxHashMap::default();
        // Reachability follows both top-level and lazy import edges — a lazily
        // imported module is still loaded at runtime, just deferred.
        for (a, b) in self.edges.iter().chain(&self.lazy_edges) {
            adj.entry(*a).or_default().push(*b);
        }
        let mut queue: Vec<FileId> = self
            .modules
            .iter()
            .filter(|m| m.is_entry)
            .map(|m| m.id)
            .collect();
        let mut seen: FxHashSet<FileId> = queue.iter().copied().collect();
        while let Some(id) = queue.pop() {
            if let Some(neighbors) = adj.get(&id) {
                for &n in neighbors {
                    if seen.insert(n) {
                        queue.push(n);
                    }
                }
            }
        }
        self.reachable = seen;
    }

    /// Mark additional modules as analysis roots by dotted name (e.g. the module
    /// half of a `[project.scripts]` entry point `pkg.cli:main`), then recompute
    /// reachability. A no-op for names that match no module.
    pub fn mark_entry_points(&mut self, dotted_modules: &[String]) {
        let wanted: FxHashSet<&str> = dotted_modules.iter().map(|s| s.as_str()).collect();
        let mut changed = false;
        for m in &mut self.modules {
            if !m.is_entry && wanted.contains(m.dotted.as_str()) {
                m.is_entry = true;
                changed = true;
            }
        }
        if changed {
            self.compute_reachability();
        }
    }

    /// Files that are neither entries nor reachable from any entry.
    pub fn unused_files(&self) -> Vec<&ModuleInfo> {
        self.modules
            .iter()
            .filter(|m| !m.is_entry && !self.reachable.contains(&m.id))
            .collect()
    }

    /// Whether a symbol defined in `module` is referenced internally or by any
    /// importer of that module. `defs_named` is how many top-level defs share
    /// the name (to discount the definition site in the internal count).
    pub fn symbol_used(&self, module: FileId, name: &str, defs_named: u32) -> bool {
        let m = self.module(module);
        // Internal use. With scope/binding resolution, a top-level symbol is used
        // iff some free `Name` load resolves to it (module_used) — precise: it
        // ignores shadowing function-locals and attribute accesses. In modules
        // with a dynamic sink (getattr/eval/importlib) we can't trust static
        // resolution, so fall back to the conservative token-frequency count.
        let internal = if m.parsed.has_dynamic_sink {
            m.parsed.name_counts.get(name).copied().unwrap_or(0) > defs_named
        } else {
            m.parsed
                .module_used
                .binary_search_by(|s| s.as_str().cmp(name))
                .is_ok()
        };
        if internal {
            return true;
        }
        // Imported by name from this module (covers `from m import name`).
        if let Some(set) = self.imported_symbols.get(&module) {
            if set.contains(name) || set.contains("*") {
                return true;
            }
        }
        // Cross-module: any module that imports `module` (eagerly or lazily —
        // the in-function cycle-breaker pattern) references `name`.
        let importers: Vec<FileId> = self
            .edges
            .iter()
            .chain(&self.lazy_edges)
            .filter(|(_, b)| *b == module)
            .map(|(a, _)| *a)
            .collect();
        for imp in importers {
            let im = self.module(imp);
            if im.parsed.name_counts.contains_key(name) {
                return true;
            }
        }
        false
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Internal import edges as (importer dotted, imported dotted) pairs.
    /// Used by the architecture-boundary engine.
    pub fn import_edges(&self) -> Vec<(&str, &str)> {
        self.edges
            .iter()
            .map(|(a, b)| {
                (
                    self.modules[a.0 as usize].dotted.as_str(),
                    self.modules[b.0 as usize].dotted.as_str(),
                )
            })
            .collect()
    }

    /// The path of a module by its dotted name (first match), for findings.
    pub fn path_of_dotted(&self, dotted: &str) -> Option<&Utf8Path> {
        self.modules
            .iter()
            .find(|m| m.dotted == dotted)
            .map(|m| m.path.as_path())
    }

    /// Find import cycles: strongly-connected components of size > 1, plus
    /// self-loops. Tarjan's algorithm; results are deterministic (each cycle's
    /// members sorted, and the list sorted). Cross-module circular imports.
    pub fn find_cycles(&self) -> Vec<Vec<FileId>> {
        let n = self.modules.len();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut self_loops: Vec<usize> = Vec::new();
        for (a, b) in &self.edges {
            if a == b {
                self_loops.push(a.0 as usize);
            } else {
                adj[a.0 as usize].push(b.0 as usize);
            }
        }

        // Iterative Tarjan to avoid stack overflow on large graphs.
        let mut index = vec![u32::MAX; n];
        let mut low = vec![0u32; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        let mut idx: u32 = 0;
        let mut out: Vec<Vec<FileId>> = Vec::new();

        // Explicit DFS frame: (node, next child position).
        for start in 0..n {
            if index[start] != u32::MAX {
                continue;
            }
            let mut call: Vec<(usize, usize)> = vec![(start, 0)];
            while let Some(&(v, ci)) = call.last() {
                if ci == 0 {
                    index[v] = idx;
                    low[v] = idx;
                    idx += 1;
                    stack.push(v);
                    on_stack[v] = true;
                }
                if ci < adj[v].len() {
                    let w = adj[v][ci];
                    call.last_mut().unwrap().1 += 1;
                    if index[w] == u32::MAX {
                        call.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(index[w]);
                    }
                } else {
                    if low[v] == index[v] {
                        let mut comp = Vec::new();
                        loop {
                            let w = stack.pop().unwrap();
                            on_stack[w] = false;
                            comp.push(FileId(w as u32));
                            if w == v {
                                break;
                            }
                        }
                        if comp.len() > 1 {
                            comp.sort();
                            out.push(comp);
                        }
                    }
                    call.pop();
                    if let Some(&(parent, _)) = call.last() {
                        low[parent] = low[parent].min(low[v]);
                    }
                }
            }
        }

        for s in self_loops {
            out.push(vec![FileId(s as u32)]);
        }
        out.sort();
        out
    }
}

/// Resolve a relative import (`from ..pkg import x` inside `a.b.c`) to a dotted
/// module name. `dots`=1 means the current package.
///
/// `is_package` distinguishes a package surface (`__init__.py`, whose `dotted`
/// name *is* the current package) from a regular module (whose package is its
/// parent). For a module `a.b.c`, one dot resolves against the package `a.b`
/// (drop the module's own segment); for the package `a.b`'s `__init__.py`, one
/// dot already *is* `a.b` (drop nothing).
/// Join a dotted prefix and a name; when the prefix is the root package
/// (empty string — a root-level `__init__.py`), the name stands alone.
fn join_dotted(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn resolve_relative(importer_dotted: &str, dots: u8, module: &str, is_package: bool) -> String {
    let parts: Vec<&str> = importer_dotted.split('.').collect();
    // A package's own dotted name is already the current package, so a single
    // leading dot keeps every segment; a module must first drop its own segment.
    let drop = if is_package {
        (dots as usize).saturating_sub(1)
    } else {
        dots as usize
    };
    let keep = parts.len().saturating_sub(drop);
    let base = parts[..keep].join(".");
    match (base.is_empty(), module.is_empty()) {
        (true, _) => module.to_string(),
        (false, true) => base,
        (false, false) => format!("{base}.{module}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-graph-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn dotted_name_handles_init_and_src() {
        let root = Utf8Path::new("/proj");
        assert_eq!(
            dotted_name(root, Utf8Path::new("/proj/pkg/mod.py")),
            "pkg.mod"
        );
        assert_eq!(
            dotted_name(root, Utf8Path::new("/proj/pkg/__init__.py")),
            "pkg"
        );
        assert_eq!(dotted_name(root, Utf8Path::new("/proj/src/a/b.py")), "a.b");
        // The analysis root is itself a package: its `__init__.py` is the
        // root package surface, not a module named `__init__`.
        assert_eq!(dotted_name(root, Utf8Path::new("/proj/__init__.py")), "");
    }

    #[test]
    fn root_level_init_resolves_sibling_relative_imports() {
        // When the analysis root is itself a package, `from . import mod` in
        // the root `__init__.py` must reach the sibling module `mod`.
        let d = temp("rootinit");
        write(&d, "__init__.py", "from . import mod\n");
        write(&d, "mod.py", "def go():\n    return 1\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        assert!(
            !g.unused_files().iter().any(|m| m.dotted == "mod"),
            "sibling of a root __init__.py wrongly unused"
        );
        assert!(
            g.unresolved_imports().is_empty(),
            "root-relative import wrongly unresolved"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn main_guard_scripts_are_entry_points() {
        // A plain script with `if __name__ == "__main__":` is runnable — it
        // and everything it imports must not be reported dead.
        let d = temp("mainguard");
        write(
            &d,
            "run.py",
            "from lib import go\n\nif __name__ == \"__main__\":\n    go()\n",
        );
        write(&d, "lib.py", "def go():\n    return 1\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        assert!(
            g.unused_files().is_empty(),
            "script project wrongly dead: {:?}",
            g.unused_files()
                .iter()
                .map(|m| m.dotted.as_str())
                .collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn lazy_importer_counts_for_symbol_use() {
        // A symbol used exclusively through a deferred `import helper` inside
        // a function (the cycle-breaker pattern) is NOT dead.
        let d = temp("lazysym");
        write(&d, "__main__.py", "import app\napp.run()\n");
        write(
            &d,
            "app.py",
            "def run():\n    import helper\n    return helper.go()\n",
        );
        write(&d, "helper.py", "def go():\n    return 1\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let helper = g
            .modules
            .iter()
            .find(|m| m.dotted == "helper")
            .expect("helper module");
        assert!(
            g.symbol_used(helper.id, "go", 1),
            "lazily-imported symbol wrongly unused"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn nested_unresolved_relative_imports_are_reported() {
        let d = temp("nestedunres");
        write(&d, "pkg/__init__.py", "");
        write(&d, "pkg/app.py", "def f():\n    from .helprs import go\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let unresolved = g.unresolved_imports();
        assert!(
            unresolved.iter().any(|u| u.display == ".helprs" && u.relative),
            "typo'd in-function relative import not reported: {unresolved:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn relative_import_resolution() {
        // Module case (`a/b/c.py`): one dot resolves against the parent package.
        assert_eq!(resolve_relative("a.b.c", 1, "d", false), "a.b.d");
        assert_eq!(resolve_relative("a.b.c", 2, "d", false), "a.d");
        assert_eq!(resolve_relative("a.b.c", 1, "", false), "a.b");
    }

    #[test]
    fn relative_import_resolution_from_package_init() {
        // Package surface (`pkg/__init__.py`, dotted == "pkg"): one dot is the
        // package itself, so `.aa` -> `pkg.aa` (the v0.1.2 cascade bug).
        assert_eq!(resolve_relative("pkg", 1, "aa", true), "pkg.aa");
        assert_eq!(resolve_relative("pkg", 1, "", true), "pkg");
        // Subpackage `pkg/sub/__init__.py` (dotted == "pkg.sub"): two dots go up
        // one real level to the parent package.
        assert_eq!(resolve_relative("pkg.sub", 1, "x", true), "pkg.sub.x");
        assert_eq!(resolve_relative("pkg.sub", 2, "x", true), "pkg.x");
    }

    #[test]
    fn entry_points_become_reachability_roots() {
        let d = temp("entrypts");
        // `cli.py` has no in-repo importer; only a console-script references it.
        write(&d, "cli.py", "def main():\n    return 1\n");
        let files = discover_python_files(&d);
        let mut g = ModuleGraph::build(&d, &files);
        // Before marking, cli is unused.
        assert!(g.unused_files().iter().any(|m| m.dotted == "cli"));
        g.mark_entry_points(&["cli".to_string()]);
        // After marking, it's an entry root → not unused.
        assert!(!g.unused_files().iter().any(|m| m.dotted == "cli"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn lazy_imports_are_collected_and_create_edges() {
        let d = temp("lazyedge");
        write(&d, "__main__.py", "import app\napp.run()\n");
        // `app` lazily imports the internal `helper` module inside a function.
        write(
            &d,
            "app.py",
            "def run():\n    from helper import go\n    return go()\n",
        );
        write(&d, "helper.py", "def go():\n    return 1\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        // The lazy `from helper import go` makes `helper` reachable.
        assert!(
            !g.unused_files().iter().any(|m| m.dotted == "helper"),
            "lazy-imported module wrongly unused"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn lazy_import_does_not_create_arch_cycle() {
        // The canonical cycle-breaker: A imports B at top level; B imports A
        // *inside a function* to avoid the cycle. mollify must NOT report a
        // circular-dependency — but B must still be reachable, and A reachable
        // from B's lazy import (no false unused-file).
        let d = temp("lazycycle");
        write(&d, "__main__.py", "import a\na.go()\n");
        write(&d, "a.py", "import b\n\ndef go():\n    return b.helper()\n");
        write(&d, "b.py", "def helper():\n    import a\n    return a.go\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        // The lazy b→a edge must not form a cycle in the arch view.
        assert!(
            g.find_cycles().is_empty(),
            "lazy import wrongly produced a cycle: {:?}",
            g.find_cycles()
        );
        // Both modules are still reachable (lazy edges feed reachability).
        let unused: Vec<_> = g.unused_files().iter().map(|m| m.dotted.clone()).collect();
        assert!(
            !unused.contains(&"a".to_string()) && !unused.contains(&"b".to_string()),
            "modules wrongly unused: {unused:?}"
        );
        // Sanity: a *top-level* b→a import (no deferral) still IS a cycle.
        write(&d, "b.py", "import a\n\ndef helper():\n    return a.go\n");
        let files = discover_python_files(&d);
        let g2 = ModuleGraph::build(&d, &files);
        assert!(
            !g2.find_cycles().is_empty(),
            "top-level mutual import should still be a cycle"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn package_init_reexports_resolve() {
        // A package whose __init__.py re-exports submodules via relative imports
        // must NOT produce unresolved-import / unused-file / unused-export.
        let d = temp("pkginit");
        write(&d, "__main__.py", "from pkg import helper\nhelper()\n");
        write(
            &d,
            "pkg/__init__.py",
            "from .aa import helper\nfrom . import bb\n",
        );
        write(&d, "pkg/aa.py", "def helper():\n    return 1\n");
        write(&d, "pkg/bb.py", "def thing():\n    return 2\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        // Both relative re-exports resolve: no unresolved imports.
        assert!(
            g.unresolved_imports().is_empty(),
            "unexpected unresolved: {:?}",
            g.unresolved_imports()
        );
        // The submodules are reachable, so neither is an unused file.
        let unused: Vec<_> = g.unused_files().iter().map(|m| m.dotted.clone()).collect();
        assert!(
            !unused.contains(&"pkg.aa".to_string()) && !unused.contains(&"pkg.bb".to_string()),
            "submodules wrongly unused: {unused:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn unused_file_detected() {
        let d = temp("unused");
        write(&d, "__main__.py", "from used import helper\nhelper()\n");
        write(&d, "used.py", "def helper():\n    return 1\n");
        write(&d, "orphan.py", "def never():\n    return 2\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let unused: Vec<_> = g.unused_files().iter().map(|m| m.dotted.clone()).collect();
        assert!(unused.contains(&"orphan".to_string()), "got {unused:?}");
        assert!(!unused.contains(&"used".to_string()));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reads_notebook_code_cells() {
        let d = temp("nb");
        let nb = r##"{"cells":[{"cell_type":"markdown","source":["title"]},{"cell_type":"code","source":["def nb_fn(x):\n","    return x\n"]}]}"##;
        write(&d, "analysis.ipynb", nb);
        let files = discover_python_files(&d);
        assert!(files.iter().any(|f| f.as_str().ends_with("analysis.ipynb")));
        let g = ModuleGraph::build(&d, &files);
        let nbmod = g
            .modules
            .iter()
            .find(|m| m.path.as_str().ends_with("analysis.ipynb"))
            .unwrap();
        assert!(
            nbmod.parsed.definitions.iter().any(|x| x.name == "nb_fn"),
            "notebook code not parsed"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn detects_import_cycle() {
        let d = temp("cycle");
        write(
            &d,
            "__init__.py",
            "import a
import b
",
        );
        write(
            &d,
            "a.py",
            "import b
",
        );
        write(
            &d,
            "b.py",
            "import a
",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let cycles = g.find_cycles();
        assert!(
            cycles.iter().any(|c| c.len() == 2),
            "expected a 2-cycle, got {cycles:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn symbol_use_cross_module() {
        let d = temp("symuse");
        write(&d, "__main__.py", "from lib import used_fn\nused_fn()\n");
        write(
            &d,
            "lib.py",
            "def used_fn():\n    return 1\n\ndef dead_fn():\n    return 2\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let lib = g.modules.iter().find(|m| m.dotted == "lib").unwrap().id;
        assert!(g.symbol_used(lib, "used_fn", 1));
        assert!(!g.symbol_used(lib, "dead_fn", 1));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn discovery_skips_venvs_vcs_and_caches() {
        let d = temp("excluded-dirs");
        write(&d, "src/app.py", "def main():\n    return 1\n");
        write(
            &d,
            ".venv/lib/python3.12/site-packages/somepkg/__init__.py",
            "def pkg_fn():\n    return 1\n",
        );
        write(&d, ".git/hooks/pre-commit.py", "raise SystemExit(0)\n");
        write(&d, "__pycache__/app.cpython-312.py", "stale_cache = 1\n");
        write(&d, "node_modules/foo/bar.py", "bar = 1\n");
        // A custom-named virtualenv: only `pyvenv.cfg` marks it, not its name.
        write(&d, "myenv/pyvenv.cfg", "home = /usr/bin\n");
        write(&d, "myenv/lib/site.py", "site_fn = 1\n");

        let files = discover_python_files(&d);
        let rel: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(&d).unwrap().to_string())
            .collect();
        assert_eq!(rel, vec!["src/app.py".to_string()], "got {rel:?}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn discovery_honors_extra_excludes() {
        let d = temp("extra-excludes");
        write(&d, "src/app.py", "def main():\n    return 1\n");
        write(&d, "vendor/thirdparty.py", "x = 1\n");

        let files = discover_python_files_excluding(&d, &["vendor".to_string()]);
        let rel: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(&d).unwrap().to_string())
            .collect();
        assert_eq!(rel, vec!["src/app.py".to_string()], "got {rel:?}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn discovery_include_overrides_default_and_extra_excludes() {
        let d = temp("include-overrides");
        write(&d, "src/app.py", "def main():\n    return 1\n");
        write(&d, "node_modules/foo/bar.py", "bar = 1\n");
        write(&d, "vendor/thirdparty.py", "x = 1\n");

        let files = discover_python_files_with(
            &d,
            &["vendor".to_string()],
            &["node_modules".to_string(), "vendor".to_string()],
        );
        let mut rel: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(&d).unwrap().to_string())
            .collect();
        rel.sort();
        assert_eq!(
            rel,
            vec![
                "node_modules/foo/bar.py".to_string(),
                "src/app.py".to_string(),
                "vendor/thirdparty.py".to_string(),
            ],
            "got {rel:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn discovery_include_overrides_gitignore() {
        let d = temp("include-overrides-gitignore");
        write(&d, ".gitignore", "node_modules/\n");
        write(&d, "src/app.py", "def main():\n    return 1\n");
        write(&d, "node_modules/foo/bar.py", "bar = 1\n");

        // Without --include, .gitignore prunes node_modules/ entirely.
        let plain = discover_python_files(&d);
        let plain_rel: Vec<String> = plain
            .iter()
            .map(|f| f.strip_prefix(&d).unwrap().to_string())
            .collect();
        assert_eq!(
            plain_rel,
            vec!["src/app.py".to_string()],
            "got {plain_rel:?}"
        );

        // With --include node_modules, the override wins over .gitignore.
        let files = discover_python_files_with(&d, &[], &["node_modules".to_string()]);
        let mut rel: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(&d).unwrap().to_string())
            .collect();
        rel.sort();
        assert_eq!(
            rel,
            vec![
                "node_modules/foo/bar.py".to_string(),
                "src/app.py".to_string(),
            ],
            "got {rel:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn discovery_include_does_not_override_pyvenv_guard() {
        let d = temp("include-pyvenv-guard");
        write(&d, "src/app.py", "def main():\n    return 1\n");
        // A directory the user explicitly --include's, but which is itself a
        // virtualenv (has pyvenv.cfg) — the guard must still win.
        write(&d, "vendor/pyvenv.cfg", "home = /usr/bin\n");
        write(&d, "vendor/lib/site.py", "site_fn = 1\n");

        let files = discover_python_files_with(&d, &[], &["vendor".to_string()]);
        let rel: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(&d).unwrap().to_string())
            .collect();
        assert_eq!(rel, vec!["src/app.py".to_string()], "got {rel:?}");
        std::fs::remove_dir_all(&d).ok();
    }
}
