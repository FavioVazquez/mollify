//! Type-health engine — annotation coverage for public functions. A
//! Python-specific signal with no fallow analog (RESEARCH.md §8: clean white
//! space). Flags fully-untyped public functions (params, but zero annotations
//! and no return type).

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in &graph.modules {
        for f in &m.parsed.functions {
            // Only public functions that take parameters.
            if f.name.starts_with('_') || f.params_total == 0 {
                continue;
            }
            // Flag only fully-untyped: no annotated params and no return type.
            if f.params_annotated > 0 || f.return_annotated {
                continue;
            }
            let rule = "untyped-function";
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[m.path.as_str(), &f.name, &f.line.to_string()]),
                rule: rule.into(),
                category: Category::TypeHealth,
                severity: Severity::Warn,
                confidence: Confidence::Likely,
                attribution: None,
                reason: format!(
                    "public function `{}` has no type annotations (0/{} params typed, no return type)",
                    f.name, f.params_total
                ),
                location: Location {
                    path: m.path.clone(),
                    line: f.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "add-annotations".into(),
                    description: format!("Add parameter and return type annotations to `{}`", f.name),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[untyped-function]".into()),
                }],
            });
        }
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
            std::env::temp_dir().join(format!("mollify-core-th-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }
    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    #[test]
    fn flags_untyped_public_but_not_typed_or_private() {
        let d = temp("th");
        write(
            &d,
            "__init__.py",
            "def untyped(a, b):\n    return a\n\ndef typed(a: int) -> int:\n    return a\n\ndef _hidden(a):\n    return a\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert_eq!(f.len(), 1, "got {f:?}");
        assert!(f[0].reason.contains("untyped"));
        std::fs::remove_dir_all(&d).ok();
    }
}
