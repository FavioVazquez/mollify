//! # mollify-graph
//!
//! Discovers Python modules, assigns **path-sorted stable FileIds** (ADR-004
//! analog), builds the internal import graph, computes **reachability** from
//! entry points, and answers symbol-usage queries. Pure structure — the
//! `mollify-core` crate turns these into [`mollify_parse`]-backed findings.

use camino::{Utf8Path, Utf8PathBuf};
use mollify_parse::{ParsedModule, PyParser};
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
    /// Dotted module name relative to its source root (e.g. `pkg.sub.mod`).
    pub dotted: String,
    pub parsed: ParsedModule,
    /// True if this module is an analysis root (entry point).
    pub is_entry: bool,
}

/// The whole project graph.
pub struct ModuleGraph {
    pub modules: Vec<ModuleInfo>,
    by_dotted: FxHashMap<String, FileId>,
    /// Resolved internal import edges: importer → imported.
    edges: Vec<(FileId, FileId)>,
    /// For each imported module, the set of symbol names pulled in by importers,
    /// keyed by the imported module's FileId.
    imported_symbols: FxHashMap<FileId, FxHashSet<String>>,
    reachable: FxHashSet<FileId>,
    /// True if any module in the project has a dynamic dispatch/import sink.
    pub global_dynamic: bool,
}

/// Walk `root` for `*.py` and `*.ipynb` files, honoring `.gitignore`.
/// Deterministic order.
pub fn discover_python_files(root: &Utf8Path) -> Vec<Utf8PathBuf> {
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .build()
        .flatten()
    {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "py" || e == "ipynb") {
            if let Ok(u) = Utf8PathBuf::from_path_buf(p.to_path_buf()) {
                out.push(u);
            }
        }
    }
    out.sort();
    out
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
    let no_init = no_ext.strip_suffix("/__init__").unwrap_or(no_ext);
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
            modules.push(ModuleInfo {
                id,
                is_entry: is_entry(&path),
                path,
                dotted,
                parsed: pm,
            });
        }

        let mut g = ModuleGraph {
            modules,
            by_dotted,
            edges: Vec::new(),
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
        let mut imported_symbols: FxHashMap<FileId, FxHashSet<String>> = FxHashMap::default();

        for m in &self.modules {
            for imp in &m.parsed.imports {
                let target_dotted = if imp.relative_dots > 0 {
                    resolve_relative(&m.dotted, imp.relative_dots, &imp.module)
                } else {
                    imp.module.clone()
                };
                // Try the full dotted path, then progressively shorter prefixes
                // (handles `from pkg.mod import name` where `pkg.mod` is a module
                // and `name` is a symbol, vs `import pkg.mod`).
                if let Some(&tid) = self.lookup(&target_dotted) {
                    edges.push((m.id, tid));
                    let set = imported_symbols.entry(tid).or_default();
                    for n in &imp.names {
                        set.insert(n.clone());
                    }
                    if imp.is_star {
                        set.insert("*".into());
                    }
                } else if !imp.names.is_empty() {
                    // `from pkg import submod` where submod is itself a module.
                    for n in &imp.names {
                        let candidate = format!("{target_dotted}.{n}");
                        if let Some(&tid) = self.lookup(&candidate) {
                            edges.push((m.id, tid));
                        }
                    }
                }
            }
        }
        edges.sort();
        edges.dedup();
        self.edges = edges;
        self.imported_symbols = imported_symbols;
    }

    fn lookup(&self, dotted: &str) -> Option<&FileId> {
        self.by_dotted.get(dotted)
    }

    /// BFS mark-reachable from all entry modules over import edges.
    fn compute_reachability(&mut self) {
        let mut adj: FxHashMap<FileId, Vec<FileId>> = FxHashMap::default();
        for (a, b) in &self.edges {
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
        // Internal use: appears more times than it is defined.
        let internal = m.parsed.name_counts.get(name).copied().unwrap_or(0) > defs_named;
        if internal {
            return true;
        }
        // Imported by name from this module (covers `from m import name`).
        if let Some(set) = self.imported_symbols.get(&module) {
            if set.contains(name) || set.contains("*") {
                return true;
            }
        }
        // Cross-module: any module that imports `module` references `name`.
        let importers: Vec<FileId> = self
            .edges
            .iter()
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
fn resolve_relative(importer_dotted: &str, dots: u8, module: &str) -> String {
    let parts: Vec<&str> = importer_dotted.split('.').collect();
    // The importer's package = drop the module's own last segment.
    // For `a.b.c`, package is `a.b`; one extra dot goes up one more level.
    let keep = parts.len().saturating_sub(dots as usize);
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
    }

    #[test]
    fn relative_import_resolution() {
        assert_eq!(resolve_relative("a.b.c", 1, "d"), "a.b.d");
        assert_eq!(resolve_relative("a.b.c", 2, "d"), "a.d");
        assert_eq!(resolve_relative("a.b.c", 1, ""), "a.b");
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
}
