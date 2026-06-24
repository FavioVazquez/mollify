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

/// Emit one finding per import cycle.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for cycle in graph.find_cycles() {
        let members: Vec<&str> = cycle
            .iter()
            .map(|id| graph.modules[id.0 as usize].dotted.as_str())
            .collect();
        let paths: Vec<&str> = cycle
            .iter()
            .map(|id| graph.modules[id.0 as usize].path.as_str())
            .collect();
        let first = &graph.modules[cycle[0].0 as usize];
        let chain = if members.len() == 1 {
            format!("`{}` imports itself", members[0])
        } else {
            format!("import cycle: {} → {}", members.join(" → "), members[0])
        };
        findings.push(Finding {
            fingerprint: fingerprint("circular-dependency", &paths),
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
