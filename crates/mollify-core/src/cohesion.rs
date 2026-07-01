//! Class-cohesion engine (LCOM*, Henderson-Sellers). Measures how much a class's
//! methods share instance attributes; a class whose methods touch disjoint
//! attribute sets is doing several unrelated jobs and is a split candidate.
//!
//! `LCOM* = ((1/a)·Σ μ(aᵢ) − m) / (1 − m)` where `m` = method count, `a` =
//! distinct attributes, `μ(aᵢ)` = methods referencing attribute i. ~0 cohesive,
//! ~1 incohesive. We flag only clear cases (enough methods/attrs, high LCOM*).

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

const MIN_METHODS: usize = 3;
const LCOM_THRESHOLD: f64 = 0.8;

pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for module in &graph.modules {
        for class in &module.parsed.classes {
            // Consider methods that reference at least one instance attribute;
            // dunder and attribute-free helpers don't inform cohesion.
            let methods: Vec<&Vec<String>> = class
                .methods
                .iter()
                .filter(|(name, _)| !(name.starts_with("__") && name.ends_with("__")))
                .map(|(_, attrs)| attrs)
                .collect();
            let m = methods.len();
            if m < MIN_METHODS {
                continue;
            }
            let mut all_attrs: std::collections::BTreeSet<&str> = Default::default();
            for attrs in &methods {
                for a in *attrs {
                    all_attrs.insert(a.as_str());
                }
            }
            let a = all_attrs.len();
            if a == 0 {
                continue;
            }
            // Σ μ(aᵢ): for each attribute, how many methods reference it.
            let sum_mu: usize = all_attrs
                .iter()
                .map(|attr| {
                    methods
                        .iter()
                        .filter(|ms| ms.iter().any(|x| x == attr))
                        .count()
                })
                .sum();
            let lcom = ((sum_mu as f64 / a as f64) - m as f64) / (1.0 - m as f64);
            let lcom = lcom.clamp(0.0, 1.0);
            if lcom <= LCOM_THRESHOLD {
                continue;
            }
            let rule = "low-cohesion";
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[module.rel.as_str(), &class.name]),
                rule: rule.into(),
                category: Category::Complexity,
                severity: Severity::Warn,
                confidence: Confidence::Uncertain,
                attribution: None,
                reason: format!(
                    "class `{}` has low cohesion (LCOM* {:.2} over {m} methods / {a} attributes) — its methods share few instance attributes",
                    class.name, lcom
                ),
                location: Location {
                    path: module.path.clone(),
                    line: class.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "split-class".into(),
                    description: format!(
                        "Consider splitting `{}` into cohesive smaller classes.",
                        class.name
                    ),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[low-cohesion]".into()),
                }],
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-coh-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn flags_incohesive_class_not_cohesive_one() {
        let d = temp("c");
        // God: 3 methods each touching a different attribute → incohesive.
        std::fs::write(
            d.join("god.py"),
            "class God:\n    def a(self):\n        return self.x\n    def b(self):\n        return self.y\n    def c(self):\n        return self.z\n",
        )
        .unwrap();
        // Cohesive: all methods share self.v.
        std::fs::write(
            d.join("coh.py"),
            "class Coh:\n    def a(self):\n        return self.v\n    def b(self):\n        return self.v + 1\n    def c(self):\n        return self.v * 2\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(f.iter().any(|x| x.reason.contains("`God`")), "got {f:?}");
        assert!(!f.iter().any(|x| x.reason.contains("`Coh`")), "got {f:?}");
        std::fs::remove_dir_all(&d).ok();
    }
}
