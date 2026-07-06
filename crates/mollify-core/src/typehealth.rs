//! Type-health engine — annotation coverage for public functions. A
//! Python-specific signal with no fallow analog (RESEARCH.md §8: clean white
//! space). Flags fully-untyped public functions (params, but zero annotations
//! and no return type).
//!
//! **Package rollup:** a package where most public functions are untyped is
//! *deliberately* untyped — 327 per-function findings on requests carry one
//! bit of information. When at least `ROLLUP_MIN_FUNCS` eligible functions in
//! a top-level package are `ROLLUP_RATIO` untyped, the package gets a single
//! `likely` package-level finding and the per-function findings are demoted
//! to `uncertain` (evidence preserved, default reports stay readable).

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use std::collections::BTreeMap;

/// Minimum eligible public functions before a package can roll up.
const ROLLUP_MIN_FUNCS: u32 = 20;
/// Untyped fraction at which a package counts as deliberately untyped.
const ROLLUP_RATIO: f64 = 0.6;

/// Census of one top-level package's eligible public functions.
#[derive(Default)]
struct PackageCensus {
    total: u32,
    untyped: u32,
    /// Anchor for the rollup finding: the package `__init__.py` when present,
    /// otherwise the first module in graph order (deterministic).
    anchor: Option<(camino::Utf8PathBuf, bool)>,
}

fn eligible(f: &mollify_parse::FunctionComplexity) -> bool {
    !f.name.starts_with('_') && f.params_total > 0
}

fn untyped(f: &mollify_parse::FunctionComplexity) -> bool {
    f.params_annotated == 0 && !f.return_annotated
}

/// Top-level package key for a module (`requests.api` → `requests`).
fn package_key(dotted: &str) -> &str {
    dotted.split('.').next().unwrap_or(dotted)
}

pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    // Pass 1 — census per top-level package (BTreeMap: deterministic order).
    let mut census: BTreeMap<String, PackageCensus> = BTreeMap::new();
    for m in &graph.modules {
        let c = census
            .entry(package_key(&m.dotted).to_string())
            .or_default();
        for f in &m.parsed.functions {
            if eligible(f) {
                c.total += 1;
                if untyped(f) {
                    c.untyped += 1;
                }
            }
        }
        let is_pkg_init = m.is_package && m.dotted == package_key(&m.dotted);
        let better = match &c.anchor {
            None => true,
            Some((_, anchored_on_init)) => is_pkg_init && !anchored_on_init,
        };
        if better {
            c.anchor = Some((m.path.clone(), is_pkg_init));
        }
    }
    let rolled_up = |dotted: &str| {
        census.get(package_key(dotted)).is_some_and(|c| {
            c.total >= ROLLUP_MIN_FUNCS && f64::from(c.untyped) >= ROLLUP_RATIO * f64::from(c.total)
        })
    };

    let mut findings = Vec::new();
    // Package-level rollup findings first (stable position in the report).
    let rule = "untyped-function";
    for (key, c) in &census {
        if c.total < ROLLUP_MIN_FUNCS || f64::from(c.untyped) < ROLLUP_RATIO * f64::from(c.total) {
            continue;
        }
        let Some((path, _)) = &c.anchor else {
            continue;
        };
        let pct = (f64::from(c.untyped) * 100.0 / f64::from(c.total)).round() as u32;
        findings.push(Finding {
            // Keyed by package name only: stable as files move within it.
            fingerprint: fingerprint(rule, &["package", key]),
            rule: rule.into(),
            category: Category::TypeHealth,
            severity: Severity::Warn,
            confidence: Confidence::Likely,
            attribution: None,
            reason: format!(
                "package `{key}` looks deliberately untyped: {}/{} public functions have no type annotations ({pct}%); per-function findings are graded uncertain",
                c.untyped, c.total
            ),
            location: Location {
                path: path.clone(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "add-annotations".into(),
                description: format!(
                    "Adopt type annotations across package `{key}` (or suppress if intentional)"
                ),
                auto_fixable: false,
                suppression_comment: Some("# mollify: ignore[untyped-function]".into()),
            }],
        });
    }
    for m in &graph.modules {
        // Occurrence over ALL functions sharing a name (source order), so
        // fingerprints survive edits elsewhere in the file.
        let mut occ = crate::fingerprint::Occurrences::default();
        for f in &m.parsed.functions {
            let occurrence = occ.next(&f.name);
            // Only public functions that take parameters.
            if f.name.starts_with('_') || f.params_total == 0 {
                continue;
            }
            // Flag only fully-untyped: no annotated params and no return type.
            if f.params_annotated > 0 || f.return_annotated {
                continue;
            }
            let demoted = rolled_up(&m.dotted);
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[m.rel.as_str(), &f.name, &occurrence]),
                rule: rule.into(),
                category: Category::TypeHealth,
                severity: Severity::Warn,
                confidence: if demoted {
                    Confidence::Uncertain
                } else {
                    Confidence::Likely
                },
                attribution: None,
                reason: if demoted {
                    format!(
                        "public function `{}` has no type annotations (0/{} params typed, no return type; the whole package is untyped — see the package-level finding)",
                        f.name, f.params_total
                    )
                } else {
                    format!(
                    "public function `{}` has no type annotations (0/{} params typed, no return type)",
                    f.name, f.params_total
                )
                },
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
    fn deliberately_untyped_package_rolls_up() {
        let d = temp("rollup");
        write(&d, "pkg/__init__.py", "");
        // 20 fully-untyped public functions across two modules: over both
        // rollup thresholds (>= 20 eligible, >= 60% untyped).
        let funcs = |lo: u32, hi: u32| -> String {
            (lo..hi)
                .map(|i| format!("def fn{i}(a, b):\n    return a\n\n"))
                .collect()
        };
        write(&d, "pkg/alpha.py", &funcs(0, 10));
        write(&d, "pkg/beta.py", &funcs(10, 20));
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let rollups: Vec<_> = f
            .iter()
            .filter(|x| x.reason.contains("deliberately untyped"))
            .collect();
        assert_eq!(rollups.len(), 1, "want one package rollup: {rollups:?}");
        assert_eq!(rollups[0].confidence, Confidence::Likely);
        assert!(
            rollups[0].location.path.as_str().ends_with("__init__.py"),
            "rollup should anchor on the package __init__: {:?}",
            rollups[0].location
        );
        assert!(
            rollups[0].reason.contains("20/20"),
            "got {}",
            rollups[0].reason
        );
        // Per-function evidence is preserved but demoted to uncertain.
        let per_fn: Vec<_> = f
            .iter()
            .filter(|x| x.reason.starts_with("public function"))
            .collect();
        assert_eq!(per_fn.len(), 20, "evidence dropped: {}", per_fn.len());
        assert!(
            per_fn.iter().all(|x| x.confidence == Confidence::Uncertain),
            "per-function findings must be uncertain in a rolled-up package"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn mostly_typed_package_does_not_roll_up() {
        let d = temp("no-rollup");
        write(&d, "pkg/__init__.py", "");
        // 20 eligible functions, only 8 untyped (40% < 60%): no rollup, and
        // the per-function findings keep their normal likely grade.
        let mut src = String::new();
        for i in 0..8 {
            src.push_str(&format!("def untyped{i}(a):\n    return a\n\n"));
        }
        for i in 0..12 {
            src.push_str(&format!("def typed{i}(a: int) -> int:\n    return a\n\n"));
        }
        write(&d, "pkg/mod.py", &src);
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            !f.iter().any(|x| x.reason.contains("deliberately untyped")),
            "40% untyped must not roll up: {f:?}"
        );
        assert!(
            f.iter()
                .filter(|x| x.reason.contains("public function"))
                .all(|x| x.confidence == Confidence::Likely),
            "per-function findings must stay likely without a rollup"
        );
        std::fs::remove_dir_all(&d).ok();
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
