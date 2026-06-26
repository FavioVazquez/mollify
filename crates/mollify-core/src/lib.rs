//! # mollify-core
//!
//! Analysis orchestration. Builds the graph, runs the engines, and assembles the
//! kind-discriminated [`mollify_types::Report`] envelopes. Engines: dead-code,
//! dependency hygiene, architecture (cycles/layers/contracts/policies),
//! complexity + hotspots, duplication, type-health, security, cohesion,
//! commented-code, coverage, and supply-chain — all folded into `audit`.

use camino::Utf8Path;
use mollify_graph::{discover_python_files, ModuleGraph};
use mollify_types::{
    sort_findings, AuditReport, Category, Finding, FindingsReport, Report, Severity, Summary,
    SCHEMA_VERSION,
};

pub mod agents;
pub mod arch;
pub mod baseline;
pub mod cohesion;
pub mod commented;
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
pub mod installed;
pub mod known;
pub mod members;
pub mod metrics;
pub mod plugins;
pub mod policy;
pub mod sarif;
pub mod security;
pub mod suffix;
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
    let mut findings = deadcode::analyze(&graph);
    findings.extend(members::analyze(&graph));
    findings.extend(commented::analyze(&graph));
    finalize(&config::load(root), &graph, findings)
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
    findings.extend(arch::analyze_contracts(&graph, &cfg.contracts));
    findings.extend(policy::analyze(&graph, &cfg.policies));
    finalize(&cfg, &graph, findings)
}

/// `mollify complexity` / `mollify health` — complexity hotspots.
pub fn complexity_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let mut findings = complexity::analyze_with(&graph, cfg.max_cyclomatic, cfg.max_cognitive);
    findings.extend(hotspots::analyze(root, &graph));
    findings.extend(cohesion::analyze(&graph));
    finalize(&cfg, &graph, findings)
}

/// `mollify dupes` — duplication / clone families.
pub fn dupes_report(root: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let findings = dupes::analyze_with(&graph, cfg.dup_min_tokens, cfg.dup_min_lines);
    finalize(&cfg, &graph, findings)
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

/// A per-file evidence bundle: the matched module, its findings, and its import
/// neighborhood. Shared by `mollify inspect` (CLI) and the `mollify_inspect`
/// MCP tool.
pub struct Inspection {
    pub file: String,
    pub module: Option<String>,
    pub findings: Vec<Finding>,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,
}

/// Returns true if `path` matches the user's `file` argument (exact, or as a
/// trailing path fragment).
fn path_matches(path: &str, file: &str) -> bool {
    path == file || path.ends_with(file) || path.ends_with(&format!("/{file}"))
}

/// Build the evidence bundle for a single file.
pub fn inspect(root: &Utf8Path, file: &str) -> Inspection {
    let report = audit_report(root);
    let findings: Vec<Finding> = report
        .findings
        .into_iter()
        .filter(|f| path_matches(f.location.path.as_str(), file))
        .collect();
    let graph = build_graph(root);
    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(m.path.as_str(), file))
        .map(|m| m.dotted.clone());
    let trace = module.as_deref().and_then(|d| trace::module(&graph, d));
    Inspection {
        file: file.to_string(),
        module,
        findings,
        imports: trace
            .as_ref()
            .map(|t| t.imports.clone())
            .unwrap_or_default(),
        imported_by: trace
            .as_ref()
            .map(|t| t.imported_by.clone())
            .unwrap_or_default(),
    }
}

/// File-local diagnostics from an in-memory buffer (no disk, no graph) — the
/// live LSP path for `textDocument/didChange`. Covers the intra-file rules
/// (security, unused variables/parameters, complexity, commented-out code);
/// cross-file rules (dead exports, deps, architecture) are produced by the full
/// audit on save. Returns sorted findings, honoring inline suppressions.
pub fn analyze_text(path: &Utf8Path, source: &str) -> Vec<Finding> {
    let mut parser = match mollify_parse::PyParser::new() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let Ok(parsed) = parser.parse(path, source) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    findings.extend(security::analyze_parsed(path, &parsed));
    findings.extend(commented::analyze_source(path, source));
    // Unused local variables / parameters.
    for s in &parsed.scope_findings {
        let (rule, kind, confidence) = if s.is_param {
            (
                "unused-parameter",
                "parameter",
                mollify_types::Confidence::Uncertain,
            )
        } else {
            (
                "unused-variable",
                "local variable",
                mollify_types::Confidence::Likely,
            )
        };
        findings.push(Finding {
            fingerprint: fingerprint::fingerprint(
                rule,
                &[path.as_str(), &s.name, &s.line.to_string()],
            ),
            rule: rule.into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason: format!("{kind} `{}` is assigned but never used", s.name),
            location: mollify_types::Location {
                path: path.to_owned(),
                line: s.line,
                column: 0,
                end_line: None,
            },
            actions: vec![],
        });
    }
    // High complexity over default thresholds.
    for f in &parsed.functions {
        if f.cyclomatic > complexity::DEFAULT_CYCLOMATIC
            || f.cognitive > complexity::DEFAULT_COGNITIVE
        {
            findings.push(Finding {
                fingerprint: fingerprint::fingerprint("high-complexity", &[path.as_str(), &f.name]),
                rule: "high-complexity".into(),
                category: Category::Complexity,
                severity: Severity::Warn,
                confidence: mollify_types::Confidence::Certain,
                attribution: None,
                reason: format!(
                    "function `{}` is complex (cyclomatic {}, cognitive {})",
                    f.name, f.cyclomatic, f.cognitive
                ),
                location: mollify_types::Location {
                    path: path.to_owned(),
                    line: f.line,
                    column: 0,
                    end_line: Some(f.end_line),
                },
                actions: vec![],
            });
        }
    }
    // Honor inline `# mollify: ignore[...]` on the buffer's own lines.
    let mut sup: rustc_hash::FxHashMap<u32, Vec<&str>> = rustc_hash::FxHashMap::default();
    for (line, rule) in &parsed.ignores {
        sup.entry(*line).or_default().push(rule.as_str());
    }
    findings.retain(|f| {
        sup.get(&f.location.line)
            .map(|rules| !rules.iter().any(|r| *r == "*" || *r == f.rule))
            .unwrap_or(true)
    });
    sort_findings(&mut findings);
    findings
}

/// Export the module import graph as Graphviz DOT or Mermaid `flowchart`.
pub fn graph_export(root: &Utf8Path, mermaid: bool) -> String {
    let graph = build_graph(root);
    let mut edges: Vec<(String, String)> = graph
        .import_edges()
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    edges.sort();
    edges.dedup();
    let id = |s: &str| s.replace(['.', '-', '/'], "_");
    let mut out = String::new();
    if mermaid {
        out.push_str("flowchart LR\n");
        for (a, b) in &edges {
            out.push_str(&format!("    {}[\"{a}\"] --> {}[\"{b}\"]\n", id(a), id(b)));
        }
    } else {
        out.push_str("digraph imports {\n  rankdir=LR;\n  node [shape=box];\n");
        for (a, b) in &edges {
            out.push_str(&format!("  \"{a}\" -> \"{b}\";\n"));
        }
        out.push_str("}\n");
    }
    out
}

/// Topology listing for `mollify list` / `mollify_list`.
pub fn list_topology(root: &Utf8Path, kind: &str) -> Vec<String> {
    let graph = build_graph(root);
    let mut rows: Vec<String> = match kind {
        "files" => graph
            .modules
            .iter()
            .map(|m| format!("{}\t{}", m.dotted, m.path))
            .collect(),
        "frameworks" => {
            let mut fw: std::collections::BTreeSet<String> = Default::default();
            for m in &graph.modules {
                for d in &m.parsed.definitions {
                    if plugins::is_framework_entry(d) {
                        for dec in &d.decorators {
                            fw.insert(dec.split('.').next().unwrap_or(dec).to_string());
                        }
                    }
                }
            }
            fw.into_iter().collect()
        }
        _ => graph
            .modules
            .iter()
            .filter(|m| m.is_entry)
            .map(|m| format!("{}\t{}", m.dotted, m.path))
            .collect(),
    };
    rows.sort();
    rows
}

/// `mollify audit` — the unified pass across all engines. Produces a quality
/// score over the combined findings.
pub fn audit_report(root: &Utf8Path) -> AuditReport {
    let graph = build_graph(root);
    let cfg = config::load(root);
    let mut findings: Vec<Finding> = Vec::new();
    findings.extend(deadcode::analyze(&graph));
    findings.extend(members::analyze(&graph));
    findings.extend(commented::analyze(&graph));
    findings.extend(deps::analyze(root, &graph));
    findings.extend(arch::analyze(&graph));
    findings.extend(arch::analyze_layers(&graph, &cfg.arch_layers));
    findings.extend(arch::analyze_contracts(&graph, &cfg.contracts));
    findings.extend(policy::analyze(&graph, &cfg.policies));
    findings.extend(complexity::analyze_with(
        &graph,
        cfg.max_cyclomatic,
        cfg.max_cognitive,
    ));
    findings.extend(dupes::analyze_with(
        &graph,
        cfg.dup_min_tokens,
        cfg.dup_min_lines,
    ));
    findings.extend(typehealth::analyze(&graph));
    findings.extend(security::analyze(&graph));
    findings.extend(hotspots::analyze(root, &graph));
    findings.extend(cohesion::analyze(&graph));
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
