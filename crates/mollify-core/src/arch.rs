//! Architecture engine: **circular dependency** detection (Tarjan SCC) plus
//! **named layer presets** — ordered layers from `.mollifyrc` where a layer may
//! import same/lower layers but importing a *higher* layer is a
//! `layer-violation`. (`layered`/`bulletproof` use this directly; hexagonal /
//! feature-sliced map onto forbidden/independence contracts — future.)

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

/// The layer index of a dotted module name given ordered `layers` (top→bottom):
/// the first layer whose name appears as a path segment. `None` if unlayered.
fn layer_of(dotted: &str, layers: &[String]) -> Option<usize> {
    let segs: Vec<&str> = dotted.split('.').collect();
    layers.iter().position(|l| segs.iter().any(|s| s == l))
}

/// Emit `layer-violation` findings for imports that point "up" the layer order.
pub fn analyze_layers(graph: &ModuleGraph, layers: &[String]) -> Vec<Finding> {
    if layers.len() < 2 {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for (importer, imported) in graph.import_edges() {
        let (Some(ia), Some(ib)) = (layer_of(importer, layers), layer_of(imported, layers)) else {
            continue;
        };
        // Smaller index = higher layer. Importing a higher layer (ib < ia) is illegal.
        if ib < ia {
            let path = graph
                .path_of_dotted(importer)
                .map(|p| p.to_owned())
                .unwrap_or_default();
            let rule = "layer-violation";
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[importer, imported]),
                rule: rule.into(),
                category: Category::Architecture,
                severity: Severity::Warn,
                confidence: Confidence::Certain,
                attribution: None,
                reason: format!(
                    "layer violation: `{importer}` (layer `{}`) imports `{imported}` (higher layer `{}`)",
                    layers[ia], layers[ib]
                ),
                location: Location {
                    path,
                    line: 1,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "respect-layers".into(),
                    description: format!(
                        "`{}` must not depend on the higher layer `{}` — invert or relocate the dependency",
                        layers[ia], layers[ib]
                    ),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[layer-violation]".into()),
                }],
            });
        }
    }
    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.reason.cmp(&b.reason))
    });
    findings
}

/// Does a dotted module name fall under a contract prefix?
fn under(dotted: &str, prefix: &str) -> bool {
    dotted == prefix || dotted.starts_with(&format!("{prefix}."))
}

/// Evaluate declarative import contracts (forbidden + independence).
pub fn analyze_contracts(
    graph: &ModuleGraph,
    contracts: &crate::config::Contracts,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let edges = graph.import_edges();
    let path_of = |dotted: &str| {
        graph
            .path_of_dotted(dotted)
            .map(|p| p.to_owned())
            .unwrap_or_default()
    };
    let mut push = |rule: &'static str, importer: &str, imported: &str, reason: String| {
        findings.push(Finding {
            fingerprint: fingerprint(rule, &[importer, imported]),
            rule: rule.into(),
            category: Category::Architecture,
            severity: Severity::Warn,
            confidence: Confidence::Certain,
            attribution: None,
            reason,
            location: Location {
                path: path_of(importer),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "respect-contract".into(),
                description: "Invert or relocate the dependency to satisfy the contract.".into(),
                auto_fixable: false,
                suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
            }],
        });
    };

    for (importer, imported) in &edges {
        // Forbidden contracts.
        for c in &contracts.forbidden {
            if under(importer, &c.from) && c.to.iter().any(|t| under(imported, t)) {
                push(
                    "forbidden-import",
                    importer,
                    imported,
                    format!(
                        "contract violation: `{importer}` must not import `{imported}` (forbidden from `{}`)",
                        c.from
                    ),
                );
            }
        }
        // Independence groups: two distinct members must not import each other.
        for group in &contracts.independent {
            let ia = group.iter().find(|m| under(importer, m));
            let ib = group.iter().find(|m| under(imported, m));
            if let (Some(a), Some(b)) = (ia, ib) {
                if a != b {
                    push(
                        "independence-violation",
                        importer,
                        imported,
                        format!("independence violation: `{a}` and `{b}` must not depend on each other (`{importer}` → `{imported}`)"),
                    );
                }
            }
        }
    }
    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.reason.cmp(&b.reason))
    });
    // Global (not adjacent-only) dedup: overlapping contracts can produce the
    // same fingerprint with different reasons that don't sort next to each
    // other. Keep the first in the sorted order.
    let mut seen = rustc_hash::FxHashSet::default();
    findings.retain(|f| seen.insert(f.fingerprint.clone()));
    findings
}

/// Emit one finding per import cycle.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for cycle in graph.find_cycles() {
        let members: Vec<&str> = cycle
            .iter()
            .map(|id| graph.modules[id.0 as usize].dotted.as_str())
            .collect();
        let first = &graph.modules[cycle[0].0 as usize];
        let chain = if members.len() == 1 {
            format!("`{}` imports itself", members[0])
        } else {
            format!("import cycle: {} → {}", members.join(" → "), members[0])
        };
        // Dotted names are checkout- and spelling-independent identity.
        findings.push(Finding {
            fingerprint: fingerprint("circular-dependency", &members),
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

/// Public-API / interface enforcement (tach/knip parity): flag a module that
/// reaches **across a top-level package boundary** to import another package's
/// **private** (`_name`) symbol. Intra-package private imports are a package's
/// own business and are not flagged; relative imports (always intra-package) are
/// skipped. Confidence `likely`.
pub fn private_imports(graph: &ModuleGraph) -> Vec<Finding> {
    use rustc_hash::FxHashSet;
    let internal_tops: FxHashSet<&str> = graph
        .modules
        .iter()
        .filter_map(|m| m.dotted.split('.').next())
        .filter(|s| !s.is_empty())
        .collect();

    let mut findings = Vec::new();
    for m in &graph.modules {
        let importer_top = m.dotted.split('.').next().unwrap_or("");
        let mut occ = crate::fingerprint::Occurrences::default();
        for imp in &m.parsed.imports {
            if imp.relative_dots > 0 {
                continue; // relative imports are intra-package by construction
            }
            let Some(src_top) = imp.module.split('.').next() else {
                continue;
            };
            // Must cross into a *different* first-party package.
            if src_top.is_empty() || src_top == importer_top || !internal_tops.contains(src_top) {
                continue;
            }
            for name in &imp.names {
                if !(name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__"))) {
                    continue;
                }
                let rule = "private-import";
                let occ_key = format!("{}\u{1f}{name}", imp.module);
                findings.push(Finding {
                    fingerprint: fingerprint(
                        rule,
                        &[m.rel.as_str(), &imp.module, name, &occ.next(&occ_key)],
                    ),
                    rule: rule.into(),
                    category: Category::Architecture,
                    severity: Severity::Warn,
                    confidence: Confidence::Likely,
                    attribution: None,
                    reason: format!(
                        "imports private name `{name}` from another package `{}` — reaching past its public API",
                        imp.module
                    ),
                    location: Location {
                        path: m.path.clone(),
                        line: imp.line,
                        column: 0,
                        end_line: None,
                    },
                    actions: vec![Action {
                        kind: "respect-interface".into(),
                        description: format!(
                            "Import `{name}` only via `{}`'s public API, or make it public",
                            src_top
                        ),
                        auto_fixable: false,
                        suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                    }],
                });
            }
        }
    }
    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.location.line.cmp(&b.location.line))
            .then(a.reason.cmp(&b.reason))
    });
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
    fn flags_cross_package_private_import_only() {
        let d = temp("privimport");
        // Package `core` exposes `_secret`; package `app` reaches across to it.
        write(&d, "core/__init__.py", "");
        write(
            &d,
            "core/util.py",
            "def _secret():\n    return 1\n\ndef public():\n    return 2\n",
        );
        write(&d, "app/__init__.py", "");
        write(&d, "app/main.py", "from core.util import _secret, public\n");
        // Intra-package private import inside `core` must NOT be flagged.
        write(&d, "core/other.py", "from core.util import _secret\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = private_imports(&g);
        assert_eq!(
            f.len(),
            1,
            "only the cross-package private import, got {f:?}"
        );
        assert!(
            f[0].reason.contains("_secret") && f[0].location.path.as_str().contains("app/main.py")
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reports_layer_violation() {
        let d = temp("layers");
        // domain imports api (lower imports higher) -> violation.
        write(&d, "api/__init__.py", "");
        write(
            &d,
            "api/routes.py",
            "import domain.core
",
        );
        write(&d, "domain/__init__.py", "");
        write(
            &d,
            "domain/core.py",
            "import api.routes
",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let layers = vec!["api".to_string(), "domain".to_string()];
        let f = analyze_layers(&g, &layers);
        assert!(
            f.iter()
                .any(|x| x.rule == "layer-violation" && x.reason.contains("domain")),
            "got {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn lazy_cross_layer_import_not_layer_violation() {
        let d = temp("lazylayer");
        // `domain` (lower) reaches up to `api` (higher) — but lazily, inside a
        // function. A deliberately-deferred cross-boundary import must NOT be a
        // layer-violation (same rationale as the cycle-breaker).
        write(&d, "api/__init__.py", "");
        write(&d, "api/routes.py", "def handle():\n    return 1\n");
        write(&d, "domain/__init__.py", "");
        write(
            &d,
            "domain/core.py",
            "def run():\n    import api.routes\n    return api.routes.handle()\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let layers = vec!["api".to_string(), "domain".to_string()];
        assert!(
            analyze_layers(&g, &layers).is_empty(),
            "lazy cross-layer import wrongly flagged: {:?}",
            analyze_layers(&g, &layers)
        );
        // Control: the same import at top level IS a violation.
        write(&d, "domain/core.py", "import api.routes\n");
        let files = discover_python_files(&d);
        let g2 = ModuleGraph::build(&d, &files);
        assert!(
            analyze_layers(&g2, &layers)
                .iter()
                .any(|x| x.rule == "layer-violation"),
            "top-level cross-layer import should still violate"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reports_forbidden_and_independence() {
        let d = temp("contracts");
        write(&d, "domain/__init__.py", "");
        write(&d, "domain/core.py", "import web.views\n");
        write(&d, "web/__init__.py", "");
        write(&d, "web/views.py", "");
        write(&d, "featurea/__init__.py", "");
        write(&d, "featurea/x.py", "import featureb.y\n");
        write(&d, "featureb/__init__.py", "");
        write(&d, "featureb/y.py", "");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let contracts = crate::config::Contracts {
            forbidden: vec![crate::config::ForbiddenContract {
                from: "domain".into(),
                to: vec!["web".into()],
            }],
            independent: vec![vec!["featurea".into(), "featureb".into()]],
        };
        let f = analyze_contracts(&g, &contracts);
        assert!(f.iter().any(|x| x.rule == "forbidden-import"), "got {f:?}");
        assert!(
            f.iter().any(|x| x.rule == "independence-violation"),
            "got {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
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
