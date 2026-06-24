//! Security engine — a deterministic **candidate producer** (bandit-style).
//! It emits syntactic candidates; it never decides exploitability (the
//! candidate/verifier split — RESEARCH.md §2.11). Maps parser `SecurityHit`s to
//! findings with per-rule confidence.

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

fn confidence_for(rule: &str) -> Confidence {
    match rule {
        // Provable-ish footguns.
        "subprocess-shell-true" | "tls-verify-disabled" | "unsafe-yaml-load" => Confidence::Likely,
        // Depends on whether input is trusted.
        "dangerous-eval" | "unsafe-deserialization" => Confidence::Uncertain,
        // Could be a placeholder / test fixture.
        "hardcoded-secret" => Confidence::Likely,
        _ => Confidence::Likely,
    }
}

pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in &graph.modules {
        for hit in &m.parsed.security_hits {
            findings.push(Finding {
                fingerprint: fingerprint(hit.rule, &[m.path.as_str(), &hit.line.to_string()]),
                rule: hit.rule.to_string(),
                category: Category::Security,
                severity: Severity::Warn,
                confidence: confidence_for(hit.rule),
                attribution: None,
                reason: hit.detail.clone(),
                location: Location {
                    path: m.path.clone(),
                    line: hit.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "review-security".into(),
                    description: "Review this security candidate; confirm before acting".into(),
                    auto_fixable: false,
                    suppression_comment: Some(format!("# mollify: ignore[{}]", hit.rule)),
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
            std::env::temp_dir().join(format!("mollify-core-sec-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }
    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    #[test]
    fn surfaces_candidates() {
        let d = temp("sec");
        write(
            &d,
            "__init__.py",
            "import subprocess\napi_key = \"sk-abcdef123\"\nsubprocess.run(c, shell=True)\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let rules: Vec<_> = f.iter().map(|x| x.rule.as_str()).collect();
        assert!(rules.contains(&"hardcoded-secret"), "got {rules:?}");
        assert!(rules.contains(&"subprocess-shell-true"), "got {rules:?}");
        assert!(f.iter().all(|x| x.category == Category::Security));
        std::fs::remove_dir_all(&d).ok();
    }
}
