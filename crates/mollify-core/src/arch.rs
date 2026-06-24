//! Architecture engine. Today: **circular dependency** detection over the
//! module import graph (Tarjan SCC). Named boundary presets
//! (layered/hexagonal/feature-sliced/bulletproof) are planned (PLAN.md §3.6).

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

/// Emit one finding per import cycle.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for cycle in graph.find_cycles() {
        let members: Vec<&str> = cycle
            .iter()
            .map(|id| graph.modules[id.0 as usize].dotted.as_str())
            .collect();
        let paths: Vec<&str> = cycle
            .iter()
            .map(|id| graph.modules[id.0 as usize].path.as_str())
            .collect();
        let first = &graph.modules[cycle[0].0 as usize];
        let chain = if members.len() == 1 {
            format!("`{}` imports itself", members[0])
        } else {
            format!("import cycle: {} → {}", members.join(" → "), members[0])
        };
        findings.push(Finding {
            fingerprint: fingerprint("circular-dependency", &paths),
            rule: "circular-dependency".into(),
            category: Category::CircularDependency,
            severity: Severity::Warn,
            // Cycles are provable from static imports.
            confidence: Confidence::Certain,
            attribution: None,
            reason: chain,
            location: Location {
                path: first.path.clone(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "break-cycle".into(),
                description:
                    "Break the import cycle (move shared code to a lower-level module, or use a local/deferred import)"
                        .into(),
                auto_fixable: false,
                suppression_comment: Some("# mollify: ignore[circular-dependency]".into()),
            }],
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-arch-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }
    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    #[test]
    fn reports_cycle() {
        let d = temp("cyc");
        write(&d, "__init__.py", "import a\n");
        write(&d, "a.py", "import b\n");
        write(&d, "b.py", "import a\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert_eq!(f.len(), 1, "got {f:?}");
        assert_eq!(f[0].rule, "circular-dependency");
        assert_eq!(f[0].confidence, Confidence::Certain);
        std::fs::remove_dir_all(&d).ok();
    }
}
