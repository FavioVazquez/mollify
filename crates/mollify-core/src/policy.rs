//! Declarative **rule packs** (policies). A policy bans an import and/or a call,
//! optionally scoped to path substrings. Unlike the heuristic engines this is
//! pure data → deterministic, no false-positive guessing: a banned import that
//! literally appears is a `Certain` violation. Modeled on fallow's policy packs
//! but expressed in Python terms (RESEARCH.md §5).

use crate::config::Policy;
use crate::fingerprint::fingerprint;
use mollify_graph::{ModuleGraph, ModuleInfo};
use mollify_types::{Action, Category, Confidence, Finding, Location};

/// Does a dotted name match a forbidden prefix? `requests` matches `requests`
/// and `requests.get`; `os.system` matches exactly `os.system`.
fn matches_prefix(name: &str, pat: &str) -> bool {
    name == pat || name.starts_with(&format!("{pat}."))
}

/// True if `path` is in scope for a policy (empty `in_paths` = whole project).
fn in_scope(path: &str, in_paths: &[String]) -> bool {
    in_paths.is_empty() || in_paths.iter().any(|p| path.contains(p.as_str()))
}

/// Evaluate every policy against every module; emit one finding per violation.
pub fn analyze(graph: &ModuleGraph, policies: &[Policy]) -> Vec<Finding> {
    if policies.is_empty() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for m in &graph.modules {
        let path = m.path.as_str();
        // Occurrence over repeated identical violations in a module keeps
        // fingerprints line-independent yet unique.
        let mut occ = crate::fingerprint::Occurrences::default();
        for pol in policies {
            if !in_scope(path, &pol.in_paths) {
                continue;
            }
            if let Some(banned) = &pol.forbid_import {
                for imp in &m.parsed.imports {
                    if matches_prefix(&imp.module, banned) {
                        let what = format!("import of `{}`", imp.module);
                        let occurrence = occ.next(&format!("{}\u{1f}{what}", pol.id));
                        findings.push(violation(pol, m, imp.line, &what, banned, &occurrence));
                    }
                }
            }
            if let Some(banned) = &pol.forbid_call {
                for call in &m.parsed.calls {
                    if matches_prefix(&call.callee, banned) {
                        let what = format!("call to `{}`", call.callee);
                        let occurrence = occ.next(&format!("{}\u{1f}{what}", pol.id));
                        findings.push(violation(pol, m, call.line, &what, banned, &occurrence));
                    }
                }
            }
        }
    }
    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.location.line.cmp(&b.location.line))
            .then(a.rule.cmp(&b.rule))
    });
    findings
}

fn violation(
    pol: &Policy,
    m: &ModuleInfo,
    line: u32,
    what: &str,
    banned: &str,
    occurrence: &str,
) -> Finding {
    let path = m.path.as_path();
    let reason = match &pol.message {
        Some(msg) => format!("policy `{}`: {what} is forbidden — {msg}", pol.id),
        None => format!(
            "policy `{}`: {what} is forbidden (banned: `{banned}`)",
            pol.id
        ),
    };
    Finding {
        fingerprint: fingerprint(&pol.id, &[m.rel.as_str(), what, banned, occurrence]),
        rule: pol.id.clone(),
        category: Category::Architecture,
        severity: pol.severity,
        confidence: Confidence::Certain,
        attribution: None,
        reason,
        location: Location {
            path: path.to_owned(),
            line,
            column: 0,
            end_line: None,
        },
        actions: vec![Action {
            kind: "respect-policy".into(),
            description: format!("Remove or relocate the forbidden {what}."),
            auto_fixable: false,
            suppression_comment: Some(format!("# mollify: ignore[{}]", pol.id)),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Policy;
    use camino::Utf8PathBuf;
    use mollify_graph::discover_python_files;
    use mollify_types::Severity;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-policy-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn flags_forbidden_import_and_call_in_scope() {
        let d = temp("p");
        std::fs::create_dir_all(d.join("domain")).unwrap();
        std::fs::write(
            d.join("domain/core.py"),
            "import requests\n\ndef f():\n    print('x')\n    requests.get('u')\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let policies = vec![
            Policy {
                id: "no-requests-in-domain".into(),
                forbid_import: Some("requests".into()),
                forbid_call: None,
                in_paths: vec!["domain/".into()],
                message: Some("domain must stay pure".into()),
                severity: Severity::Error,
            },
            Policy {
                id: "no-print".into(),
                forbid_import: None,
                forbid_call: Some("print".into()),
                in_paths: vec![],
                message: None,
                severity: Severity::Warn,
            },
        ];
        let f = analyze(&g, &policies);
        assert!(
            f.iter().any(|x| x.rule == "no-requests-in-domain"),
            "got {f:?}"
        );
        assert!(f.iter().any(|x| x.rule == "no-print"), "got {f:?}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn respects_path_scope() {
        let d = temp("scope");
        std::fs::write(d.join("util.py"), "import requests\n").unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let policies = vec![Policy {
            id: "x".into(),
            forbid_import: Some("requests".into()),
            forbid_call: None,
            in_paths: vec!["domain/".into()],
            message: None,
            severity: Severity::Warn,
        }];
        assert!(analyze(&g, &policies).is_empty());
        std::fs::remove_dir_all(&d).ok();
    }
}
