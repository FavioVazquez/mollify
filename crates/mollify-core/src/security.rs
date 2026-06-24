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
        "subprocess-shell-true"
        | "tls-verify-disabled"
        | "unsafe-yaml-load"
        | "weak-hash"
        | "weak-cipher" => Confidence::Likely,
        // Depends on whether input is trusted / context.
        "dangerous-eval" | "unsafe-deserialization" | "sql-injection" => Confidence::Uncertain,
        // Noisy without context: stdlib random is fine for non-security use.
        "insecure-random" | "request-without-timeout" => Confidence::Uncertain,
        // Could be a placeholder / test fixture.
        "hardcoded-secret" => Confidence::Likely,
        _ => Confidence::Likely,
    }
}

/// Best-effort CWE id for a rule, surfaced in the reason for compliance/SARIF.
fn cwe_for(rule: &str) -> Option<&'static str> {
    Some(match rule {
        "dangerous-eval" => "CWE-95",
        "subprocess-shell-true" => "CWE-78",
        "sql-injection" => "CWE-89",
        "unsafe-yaml-load" => "CWE-20",
        "unsafe-deserialization" => "CWE-502",
        "tls-verify-disabled" => "CWE-295",
        "hardcoded-secret" => "CWE-798",
        "weak-hash" | "weak-cipher" => "CWE-327",
        "insecure-random" => "CWE-330",
        "request-without-timeout" => "CWE-400",
        _ => return None,
    })
}

pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in &graph.modules {
        findings.extend(analyze_parsed(&m.path, &m.parsed));
    }
    findings
}

/// Security findings for a single parsed module (also used by the live LSP path).
pub fn analyze_parsed(
    path: &camino::Utf8Path,
    parsed: &mollify_parse::ParsedModule,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for hit in &parsed.security_hits {
        findings.push(Finding {
            fingerprint: fingerprint(hit.rule, &[path.as_str(), &hit.line.to_string()]),
            rule: hit.rule.to_string(),
            category: Category::Security,
            severity: Severity::Warn,
            confidence: confidence_for(hit.rule),
            attribution: None,
            reason: match cwe_for(hit.rule) {
                Some(cwe) => format!("{} [{cwe}]", hit.detail),
                None => hit.detail.clone(),
            },
            location: Location {
                path: path.to_owned(),
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

    #[test]
    fn surfaces_expanded_rules_with_cwe() {
        let d = temp("sec2");
        write(
            &d,
            "__init__.py",
            "import hashlib, os, random\nhashlib.md5(b'x')\nos.system(cmd)\nrandom.random()\ncur.execute(f\"select {x}\")\nrequests.get(url)\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let rules: Vec<_> = f.iter().map(|x| x.rule.as_str()).collect();
        for expected in [
            "weak-hash",
            "subprocess-shell-true",
            "insecure-random",
            "sql-injection",
            "request-without-timeout",
        ] {
            assert!(rules.contains(&expected), "missing {expected}: {rules:?}");
        }
        // CWE is surfaced in the reason.
        assert!(f
            .iter()
            .find(|x| x.rule == "weak-hash")
            .unwrap()
            .reason
            .contains("CWE-327"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn surfaces_weak_cipher_with_cwe() {
        let d = temp("sec3");
        // Import-aliased weak cipher — the real-world (bandit) idiom that the
        // previous call-only matcher missed entirely.
        write(
            &d,
            "__init__.py",
            "from Crypto.Cipher import DES as d\ncipher = d.new(key, d.MODE_ECB)\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let wc = f
            .iter()
            .find(|x| x.rule == "weak-cipher")
            .expect("weak-cipher should be flagged");
        assert_eq!(wc.category, Category::Security);
        assert!(wc.reason.contains("CWE-327"), "got {}", wc.reason);
        std::fs::remove_dir_all(&d).ok();
    }
}
