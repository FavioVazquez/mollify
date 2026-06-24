//! Dead-code engine: reachability-based unused files and unused top-level
//! symbols, with confidence tiers (RESEARCH.md §4 / PLAN.md §4).

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_parse::DefKind;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use rustc_hash::FxHashMap;

/// Run dead-code analysis over the graph.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    unused_files(graph, &mut findings);
    unused_symbols(graph, &mut findings);
    findings
}

fn unused_files(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in graph.unused_files() {
        // A file that cannot be reached is a strong signal, but dynamic imports
        // anywhere in the project mean we can't be certain it is never loaded.
        let confidence = if graph.global_dynamic {
            Confidence::Uncertain
        } else {
            Confidence::Likely
        };
        out.push(Finding {
            fingerprint: fingerprint("unused-file", &[m.path.as_str()]),
            rule: "unused-file".into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason: format!(
                "module `{}` is never imported and is not an entry point",
                m.dotted
            ),
            location: Location {
                path: m.path.clone(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "remove-file".into(),
                description: format!("Delete unused module `{}`", m.path),
                auto_fixable: false, // file deletion is never auto-applied
                suppression_comment: Some("# mollify: ignore[unused-file]".into()),
            }],
        });
    }
}

fn unused_symbols(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in &graph.modules {
        // Count how many top-level defs share each name (to discount def sites).
        let mut def_counts: FxHashMap<&str, u32> = FxHashMap::default();
        for d in &m.parsed.definitions {
            *def_counts.entry(d.name.as_str()).or_insert(0) += 1;
        }
        let dunder_all: Option<&Vec<String>> = m.parsed.dunder_all.as_ref();

        for d in &m.parsed.definitions {
            // Skip dunder/special names and explicit public API (`__all__`).
            if d.name.starts_with("__") && d.name.ends_with("__") {
                continue;
            }
            if let Some(all) = dunder_all {
                if all.contains(&d.name) {
                    continue; // declared public API — treat as used
                }
            }
            // Framework-registered symbols (routes, tasks, fixtures, CLI
            // commands, signal receivers, validators, …) are reached even with
            // zero in-repo callers — the dominant false-positive killer.
            if crate::plugins::is_framework_entry(d) {
                continue;
            }
            let defs_named = def_counts.get(d.name.as_str()).copied().unwrap_or(1);
            if graph.symbol_used(m.id, &d.name, defs_named) {
                continue;
            }

            // Confidence tiering.
            let confidence = if m.parsed.has_dynamic_sink {
                Confidence::Uncertain
            } else if d.private_by_convention {
                Confidence::Certain
            } else {
                Confidence::Likely
            };

            let kind_str = match d.kind {
                DefKind::Function => "function",
                DefKind::Class => "class",
                DefKind::Variable => "variable",
            };
            let rule = "unused-export";
            out.push(Finding {
                fingerprint: fingerprint(rule, &[m.path.as_str(), &d.name]),
                rule: rule.into(),
                category: Category::DeadCode,
                severity: Severity::Warn,
                confidence,
                attribution: None,
                reason: format!(
                    "{kind_str} `{}` has no reachable references in the project",
                    d.name
                ),
                location: Location {
                    path: m.path.clone(),
                    line: d.line,
                    column: 0,
                    end_line: Some(d.end_line),
                },
                actions: vec![Action {
                    kind: "remove-symbol".into(),
                    description: format!("Delete unused {kind_str} `{}`", d.name),
                    // Only Certain findings are ever auto-fixable.
                    auto_fixable: confidence == Confidence::Certain,
                    suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                }],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};
    use mollify_graph::discover_python_files;

    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-dc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn flags_unused_public_function_as_likely() {
        let d = temp("pub");
        write(&d, "__main__.py", "from lib import used\nused()\n");
        write(&d, "lib.py", "def used():\n    return 1\n\ndef dead():\n    return 2\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let dead: Vec<_> = f.iter().filter(|x| x.rule == "unused-export").collect();
        assert_eq!(dead.len(), 1, "got {f:?}");
        assert!(dead[0].reason.contains("dead"));
        assert_eq!(dead[0].confidence, Confidence::Likely);
        assert!(!dead[0].actions[0].auto_fixable);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn private_unused_is_certain_and_autofixable() {
        let d = temp("priv");
        write(&d, "__main__.py", "print('hi')\n");
        write(&d, "lib.py", "def _dead():\n    return 2\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let s = f.iter().find(|x| x.rule == "unused-export").unwrap();
        assert_eq!(s.confidence, Confidence::Certain);
        assert!(s.actions[0].auto_fixable);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn framework_decorator_suppresses_unused() {
        let d = temp("fw");
        write(&d, "__main__.py", "import app
");
        write(&d, "app.py", "import app

@app.route('/x')
def view():
    return 1
");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(!f.iter().any(|x| x.reason.contains("`view`")), "route should be reached, got {f:?}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn dunder_all_suppresses() {
        let d = temp("all");
        write(&d, "__init__.py", "__all__ = ['api']\ndef api():\n    return 1\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(!f.iter().any(|x| x.reason.contains("`api`")));
        std::fs::remove_dir_all(&d).ok();
    }
}
