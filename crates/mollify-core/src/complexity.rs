//! Complexity engine. Flags functions whose cyclomatic or cognitive complexity
//! exceeds a threshold. (Churn × complexity hotspot ranking — the unfilled FOSS
//! Python niche — is planned via `git log --numstat`; PLAN.md §3.5.)

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

/// Default thresholds (tunable via config later).
pub const DEFAULT_CYCLOMATIC: u32 = 10;
pub const DEFAULT_COGNITIVE: u32 = 15;

pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    analyze_with(graph, DEFAULT_CYCLOMATIC, DEFAULT_COGNITIVE)
}

pub fn analyze_with(graph: &ModuleGraph, max_cyclo: u32, max_cog: u32) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in &graph.modules {
        // Occurrence over ALL functions of a name (source order): same-named
        // methods of different classes must not share a fingerprint.
        let mut occ = crate::fingerprint::Occurrences::default();
        for f in &m.parsed.functions {
            let occurrence = occ.next(&f.name);
            let over_cyclo = f.cyclomatic > max_cyclo;
            let over_cog = f.cognitive > max_cog;
            if !over_cyclo && !over_cog {
                continue;
            }
            let rule = "high-complexity";
            let reason = format!(
                "function `{}` is complex (cyclomatic {}, cognitive {}); thresholds {}/{}",
                f.name, f.cyclomatic, f.cognitive, max_cyclo, max_cog
            );
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[m.rel.as_str(), &f.name, &occurrence]),
                rule: rule.into(),
                category: Category::Complexity,
                severity: Severity::Warn,
                // The metric is exact; the *judgement* of "too complex" is the
                // user's threshold, but the measurement is certain.
                confidence: Confidence::Certain,
                attribution: None,
                reason,
                location: Location {
                    path: m.path.clone(),
                    line: f.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "refactor".into(),
                    description: format!(
                        "Refactor `{}` to reduce complexity (extract helpers, flatten nesting)",
                        f.name
                    ),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[high-complexity]".into()),
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
            std::env::temp_dir().join(format!("mollify-core-cx-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }
    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    #[test]
    fn flags_complex_function() {
        let d = temp("cx");
        // A deliberately branchy function.
        let mut body = String::from("def big(x):\n");
        for i in 0..12 {
            body.push_str(&format!("    if x == {i} and x:\n        x += {i}\n"));
        }
        body.push_str("    return x\n");
        write(&d, "__init__.py", &body);
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            f.iter()
                .any(|x| x.rule == "high-complexity" && x.reason.contains("big")),
            "got {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn ignores_simple_function() {
        let d = temp("simple");
        write(&d, "__init__.py", "def small(x):\n    return x + 1\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        assert!(analyze(&g).is_empty());
        std::fs::remove_dir_all(&d).ok();
    }
}
