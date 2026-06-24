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
pub mod config;
pub mod complexity;
pub mod deadcode;
pub mod deps;
pub mod dupes;
pub mod fix;
pub mod fingerprint;
pub mod git;
pub mod sarif;
pub mod known;
pub mod plugins;

/// Build the graph for a project root once, to be shared across engines.
pub fn build_graph(root: &Utf8Path) -> ModuleGraph {
    let files = discover_python_files(root);
    ModuleGraph::build(root, &files)
}

/// Sort, apply `.mollifyrc` (severity overrides + ignore), and summarize.
fn finalize(cfg: &config::Config, files: usize, mut findings: Vec<Finding>) -> FindingsReport {
    config::apply(cfg, &mut findings);
    sort_findings(&mut findings);
    FindingsReport {
        schema_version: SCHEMA_VERSION.into(),
        summary: Summary::from_findings(&findings, files),
        findings,
    }
}

/// `mollify dead-code` — reachability-based unused files/symbols.
pub fn dead_code_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), graph.modules.len(), deadcode::analyze(&graph))
}

/// `mollify deps` — dependency hygiene.
pub fn deps_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), graph.modules.len(), deps::analyze(root, &graph))
}

/// `mollify arch` — circular dependencies (boundary presets later).
pub fn arch_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), graph.modules.len(), arch::analyze(&graph))
}

/// `mollify complexity` / `mollify health` — complexity hotspots.
pub fn complexity_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let findings = complexity::analyze_with(&graph, cfg.max_cyclomatic, cfg.max_cognitive);
    finalize(&cfg, graph.modules.len(), findings)
}

/// `mollify dupes` — duplication / clone families.
pub fn dupes_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    finalize(&config::load(root), graph.modules.len(), dupes::analyze(&graph))
}

/// `mollify audit` — the unified pass across all engines. Produces a quality
/// score over the combined findings.
pub fn audit_report(root: &Utf8Path) -> AuditReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let mut findings: Vec<Finding> = Vec::new();
    findings.extend(deadcode::analyze(&graph));
    findings.extend(deps::analyze(root, &graph));
    findings.extend(arch::analyze(&graph));
    findings.extend(complexity::analyze_with(&graph, cfg.max_cyclomatic, cfg.max_cognitive));
    findings.extend(dupes::analyze(&graph));
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
        let base = std::env::temp_dir().join(format!("mollify-core-lib-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
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
