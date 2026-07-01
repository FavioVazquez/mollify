//! API-hygiene checks. Currently: **private-type leaks** — a public function or
//! method whose signature references a private (`_Name`) type the caller cannot
//! name. fallow's "private type leak" signal, brought to Python.
//!
//! The parser already filters out intentional private type parameters
//! (`_T = TypeVar(...)`), so this stays high-precision. Confidence `likely`
//! (downgraded to `uncertain` only when the module has a dynamic sink).

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

/// Flag private types exposed through public signatures (`private-type-leak`).
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in &graph.modules {
        let confidence = if m.parsed.has_dynamic_sink {
            Confidence::Uncertain
        } else {
            Confidence::Likely
        };
        let mut occ = crate::fingerprint::Occurrences::default();
        for leak in &m.parsed.type_leaks {
            let occ_key = format!("{}\u{1f}{}", leak.function, leak.type_name);
            let occurrence = occ.next(&occ_key);
            let rule = "private-type-leak";
            let position = if leak.is_return {
                "return type"
            } else {
                "a parameter"
            };
            out.push(Finding {
                fingerprint: fingerprint(
                    rule,
                    &[m.rel.as_str(), &leak.function, &leak.type_name, &occurrence],
                ),
                rule: rule.into(),
                category: Category::TypeHealth,
                severity: Severity::Warn,
                confidence,
                attribution: None,
                reason: format!(
                    "public `{}` exposes private type `{}` in {position}",
                    leak.function, leak.type_name
                ),
                location: Location {
                    path: m.path.clone(),
                    line: leak.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "fix-api-leak".into(),
                    description: format!(
                        "Make `{}` public, or don't expose it from `{}`'s signature",
                        leak.type_name, leak.function
                    ),
                    auto_fixable: false,
                    suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                }],
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-api-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn flags_private_type_in_public_signature_but_not_typevars() {
        let d = temp("leak");
        std::fs::write(d.join("__main__.py"), "print('x')\n").unwrap();
        std::fs::write(
            d.join("api.py"),
            "from typing import TypeVar, Optional\n\
             _T = TypeVar(\"_T\")\n\n\
             class _Internal:\n    pass\n\n\
             def public(x: Optional[_Internal]) -> _T:\n    return x\n\n\
             def _private(y: _Internal):\n    return y\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        // `public` leaks `_Internal` in a parameter; `_T` (TypeVar) is fine;
        // `_private` is itself private so it's not an API surface.
        assert!(
            f.iter().any(|x| x.rule == "private-type-leak"
                && x.reason.contains("public")
                && x.reason.contains("_Internal")),
            "got {f:?}"
        );
        assert!(
            !f.iter().any(|x| x.reason.contains("_T")),
            "TypeVar wrongly flagged: {f:?}"
        );
        assert!(!f.iter().any(|x| x.reason.contains("`_private`")));
        std::fs::remove_dir_all(&d).ok();
    }
}
