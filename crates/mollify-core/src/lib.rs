//! # mollify-core
//!
//! Analysis orchestration. Builds the graph, runs the engines, and assembles the
//! kind-discriminated [`mollify_types::Report`] envelopes. Engines implemented:
//! dead-code and dependency hygiene (Phase 1). Duplication, complexity, and
//! architecture engines land in Phase 2 (see `docs/STATUS.md`).

use camino::Utf8Path;
use mollify_graph::{discover_python_files, ModuleGraph};
use mollify_types::{
    sort_findings, AuditReport, Category, Finding, FindingsReport, Report, Severity, Summary,
    SCHEMA_VERSION,
};

pub mod arch;
pub mod baseline;
pub mod complexity;
pub mod config;
pub mod coverage;
pub mod deadcode;
pub mod deps;
pub mod dupes;
pub mod explain;
pub mod fingerprint;
pub mod fix;
pub mod git;
pub mod hotspots;
pub mod known;
pub mod plugins;
pub mod policy;
pub mod sarif;
pub mod security;
pub mod supplychain;
pub mod trace;
pub mod typehealth;
pub mod version;

/// Build the graph for a project root once, to be shared across engines.
pub fn build_graph(root: &Utf8Path) -> ModuleGraph {
    let files = discover_python_files(root);
    ModuleGraph::build(root, &files)
}

/// Sort, apply inline `# mollify: ignore[...]` suppressions and `.mollifyrc`
/// (severity overrides + ignore), then summarize.
fn finalize(
    cfg: &config::Config,
    graph: &ModuleGraph,
    mut findings: Vec<Finding>,
) -> FindingsReport {
    apply_suppressions(graph, &mut findings);
    config::apply(cfg, &mut findings);
    sort_findings(&mut findings);
    FindingsReport {
        schema_version: SCHEMA_VERSION.into(),
        summary: Summary::from_findings(&findings, graph.modules.len()),
        findings,
    }
}

/// Drop findings silenced by an inline `# mollify: ignore[<rule>]` comment on
/// the finding's line (or a bare `# mollify: ignore` matching any rule).
pub fn apply_suppressions(graph: &ModuleGraph, findings: &mut Vec<Finding>) {
    use rustc_hash::FxHashMap;
    // (path, line) -> set of suppressed rules ("*" = all).
    let mut sup: FxHashMap<(&str, u32), Vec<&str>> = FxHashMap::default();
    for m in &graph.modules {
        for (line, rule) in &m.parsed.ignores {
            sup.entry((m.path.as_str(), *line))
                .or_default()
                .push(rule.as_str());
        }
    }
    if sup.is_empty() {
        return;
    }
    findings.retain(|f| {
        if let Some(rules) = sup.get(&(f.location.path.as_str(), f.location.line)) {
            !rules.iter().any(|r| *r == "*" || *r == f.rule)
        } else {
            true
        }
    });
}

/// `mollify dead-code` — reachability-based unused files/symbols.
pub fn dead_code_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), &graph, deadcode::analyze(&graph))
}

/// `mollify deps` — dependency hygiene.
pub fn deps_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), &graph, deps::analyze(root, &graph))
}

/// `mollify arch` — circular dependencies (boundary presets later).
pub fn arch_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let mut findings = arch::analyze(&graph);
    findings.extend(arch::analyze_layers(&graph, &cfg.arch_layers));
    findings.extend(policy::analyze(&graph, &cfg.policies));
    finalize(&cfg, &graph, findings)
}

/// `mollify complexity` / `mollify health` — complexity hotspots.
pub fn complexity_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let mut findings = complexity::analyze_with(&graph, cfg.max_cyclomatic, cfg.max_cognitive);
    findings.extend(hotspots::analyze(root, &graph));
    finalize(&cfg, &graph, findings)
}

/// `mollify dupes` — duplication / clone families.
pub fn dupes_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), &graph, dupes::analyze(&graph))
}

/// `mollify types` — type-annotation health.
pub fn types_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), &graph, typehealth::analyze(&graph))
}

/// `mollify security` — security candidates (deterministic; review before acting).
pub fn security_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), &graph, security::analyze(&graph))
}

/// `mollify coverage` — cold-path analysis from a coverage.py JSON report.
pub fn coverage_report(root: &Utf8Path, coverage_path: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    let findings = coverage::analyze(root, &graph, coverage_path);
    finalize(&config::load(root), &graph, findings)
}

/// `mollify supply-chain` — match pinned/locked dependency versions against a
/// local advisory database (`vulnerable-dependency`). The DB is an input file,
/// so analysis stays deterministic and offline.
pub fn supply_chain_report(root: &Utf8Path, db_path: &Utf8Path) -> FindingsReport {
    let advisories = supplychain::load_db(db_path).unwrap_or_default();
    supply_chain_report_with(root, &advisories)
}

/// Like [`supply_chain_report`] but against an already-loaded advisory set (e.g.
/// fetched live by the CLI). Keeps the network out of `mollify-core`.
pub fn supply_chain_report_with(
    root: &Utf8Path,
    advisories: &[supplychain::Advisory],
) -> FindingsReport {
    let graph = build_graph(root);
    let findings = supplychain::analyze(root, advisories);
    finalize(&config::load(root), &graph, findings)
}

/// The default advisory DB path checked by `audit` when present.
pub const DEFAULT_ADVISORY_DB: &str = ".mollify/advisories.json";

/// `mollify audit` — the unified pass across all engines. Produces a quality
/// score over the combined findings.
pub fn audit_report(root: &Utf8Path) -> AuditReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let mut findings: Vec<Finding> = Vec::new();
    findings.extend(deadcode::analyze(&graph));
    findings.extend(deps::analyze(root, &graph));
    findings.extend(arch::analyze(&graph));
    findings.extend(arch::analyze_layers(&graph, &cfg.arch_layers));
    findings.extend(policy::analyze(&graph, &cfg.policies));
    findings.extend(complexity::analyze_with(
        &graph,
        cfg.max_cyclomatic,
        cfg.max_cognitive,
    ));
    findings.extend(dupes::analyze(&graph));
    findings.extend(typehealth::analyze(&graph));
    findings.extend(security::analyze(&graph));
    findings.extend(hotspots::analyze(root, &graph));
    // Supply-chain runs only when a local advisory DB is present (keeps audit
    // offline + deterministic; no implicit network).
    let db_path = root.join(DEFAULT_ADVISORY_DB);
    if let Some(advisories) = supplychain::load_db(&db_path) {
        findings.extend(supplychain::analyze(root, &advisories));
    }
    apply_suppressions(&graph, &mut findings);
    config::apply(&cfg, &mut findings);
    sort_findings(&mut findings);
    let files = graph.modules.len();
    let summary = Summary::from_findings(&findings, files);
    AuditReport {
        schema_version: SCHEMA_VERSION.into(),
        quality_score: quality_score(&findings, files),
        summary,
        findings,
    }
}

/// Wrap a findings report in the right `Report` variant for a given category.
pub fn into_report(category: Option<Category>, report: FindingsReport) -> Report {
    match category {
        Some(Category::DependencyHygiene) => Report::Deps(report),
        _ => Report::DeadCode(report),
    }
}

/// A simple, deterministic 0–100 health score: start at 100, subtract weighted
/// penalties per finding (errors hurt more than warnings), floor at 0. Tunable.
fn quality_score(findings: &[Finding], files: usize) -> u8 {
    if files == 0 {
        return 100;
    }
    let mut penalty = 0.0f64;
    for f in findings {
        penalty += match f.severity {
            Severity::Error => 3.0,
            Severity::Warn => 1.0,
            Severity::Off => 0.0,
        };
    }
    // Normalize against project size so big repos aren't unfairly punished.
    let per_file = penalty / files as f64;
    let score = (100.0 - per_file * 10.0).clamp(0.0, 100.0);
    score.round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-lib-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn inline_suppression_drops_finding() {
        let d = temp("suppress");
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        // `_dead` is a certain unused-export; the inline comment silences it.
        std::fs::write(
            d.join("lib.py"),
            "def _dead():  # mollify: ignore[unused-export]\n    return 1\n",
        )
        .unwrap();
        let r = dead_code_report(&d);
        assert!(
            !r.findings.iter().any(|f| f.reason.contains("_dead")),
            "suppressed finding leaked: {:?}",
            r.findings
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn audit_is_deterministic_and_scored() {
        let d = temp("audit");
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        std::fs::write(d.join("lib.py"), "def dead():\n    return 1\n").unwrap();
        let r1 = audit_report(&d);
        let r2 = audit_report(&d);
        // Determinism: identical serialization across runs.
        let j1 = serde_json::to_string(&Report::Audit(r1.clone())).unwrap();
        let j2 = serde_json::to_string(&Report::Audit(r2)).unwrap();
        assert_eq!(j1, j2);
        assert!(r1.quality_score <= 100);
        assert!(r1.findings.iter().any(|f| f.rule == "unused-export"));
        std::fs::remove_dir_all(&d).ok();
    }
}
