//! `mollify trace <module>` — the static dependency neighborhood of a module:
//! what it imports (callees, "down") and what imports it (callers, "up"). A
//! lightweight, deterministic answer to "what breaks if I touch this?" built
//! straight from the import graph (fallow's `trace`, in Python terms).

use mollify_graph::ModuleGraph;

/// The import neighborhood of a target module, both directions, sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    /// The matched dotted module name (may differ from the user's query).
    pub target: String,
    /// Modules the target imports directly.
    pub imports: Vec<String>,
    /// Modules that directly import the target.
    pub imported_by: Vec<String>,
}

/// Resolve `query` to a module (exact dotted match, else suffix match) and
/// compute its import neighborhood. `None` if nothing matches.
pub fn module(graph: &ModuleGraph, query: &str) -> Option<Trace> {
    let target = resolve(graph, query)?;
    let mut imports = Vec::new();
    let mut imported_by = Vec::new();
    for (importer, imported) in graph.import_edges() {
        if importer == target {
            imports.push(imported.to_string());
        }
        if imported == target {
            imported_by.push(importer.to_string());
        }
    }
    imports.sort();
    imports.dedup();
    imported_by.sort();
    imported_by.dedup();
    Some(Trace {
        target,
        imports,
        imported_by,
    })
}

/// Exact dotted match first; otherwise the lexicographically-first module whose
/// dotted name equals the query's trailing segment(s).
fn resolve(graph: &ModuleGraph, query: &str) -> Option<String> {
    let mut names: Vec<&str> = graph.modules.iter().map(|m| m.dotted.as_str()).collect();
    names.sort();
    if let Some(exact) = names.iter().find(|n| **n == query) {
        return Some((*exact).to_string());
    }
    names
        .iter()
        .find(|n| n.ends_with(&format!(".{query}")) || **n == query)
        .map(|n| (*n).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-trace-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn traces_both_directions() {
        let d = temp("t");
        std::fs::write(d.join("a.py"), "import b\n").unwrap();
        std::fs::write(d.join("b.py"), "import c\n").unwrap();
        std::fs::write(d.join("c.py"), "").unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let t = module(&g, "b").unwrap();
        assert_eq!(t.target, "b");
        assert!(t.imports.contains(&"c".to_string()), "{t:?}");
        assert!(t.imported_by.contains(&"a".to_string()), "{t:?}");
        assert!(module(&g, "nonexistent").is_none());
        std::fs::remove_dir_all(&d).ok();
    }
}
