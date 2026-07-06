//! # mollify-core
//!
//! Analysis orchestration. Builds the graph, runs the engines, and assembles the
//! kind-discriminated [`mollify_types::Report`] envelopes. Engines: dead-code,
//! dependency hygiene, architecture (cycles/layers/contracts/policies),
//! complexity + hotspots, duplication, type-health, security, cohesion,
//! commented-code, coverage, and supply-chain — all folded into `audit`.

use camino::Utf8Path;
use mollify_graph::{discover_python_files_with, ModuleGraph};
use mollify_types::{
    sort_findings, AuditReport, Category, Confidence, Finding, FindingsReport, Severity, Summary,
    SCHEMA_VERSION,
};

pub mod agents;
pub mod apihygiene;
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
pub mod paths;
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
/// Honors `.mollifyrc.json`'s `exclude_dirs` in addition to the builtin
/// discovery denylist (VCS metadata, virtualenvs, build/cache output).
pub fn build_graph(root: &Utf8Path) -> ModuleGraph {
    build_graph_with_includes(root, &[])
}

/// Like [`build_graph`], but `includes` directory names bypass both the
/// builtin denylist and `.mollifyrc.json`'s `exclude_dirs` — the CLI's
/// `--include` override, for one-off scans of normally-excluded directories.
pub fn build_graph_with_includes(root: &Utf8Path, includes: &[String]) -> ModuleGraph {
    let cfg = config::load(root);
    let files = discover_python_files_with(root, &cfg.exclude_dirs, includes);
    let mut graph = ModuleGraph::build(root, &files);
    // Console-script entry points (`[project.scripts]` etc.) are reachability
    // roots even with no in-repo caller.
    graph.mark_entry_points(&deps::entry_point_modules(root));
    graph
}

/// Relativize, sort, apply inline `# mollify: ignore[...]` suppressions and
/// `.mollifyrc` (severity overrides + ignore), then summarize.
fn finalize(
    root: &Utf8Path,
    cfg: &config::Config,
    graph: &ModuleGraph,
    mut findings: Vec<Finding>,
) -> FindingsReport {
    relativize_paths(root, &mut findings);
    apply_suppressions(graph, &mut findings);
    config::apply(cfg, &mut findings);
    sort_findings(&mut findings);
    FindingsReport {
        schema_version: SCHEMA_VERSION.into(),
        summary: Summary::from_findings(&findings, graph.modules.len()),
        findings,
    }
}

/// Rewrite every `location.path` to be **relative to the analysis root**.
/// Identical input must yield byte-identical output no matter how the root was
/// spelled (`.` vs an absolute path) or where the checkout lives — and
/// root-relative paths are what `.mollifyrc` patterns, baselines shared across
/// machines, and CI diffs expect.
fn relativize_paths(root: &Utf8Path, findings: &mut Vec<Finding>) {
    for f in findings {
        if let Ok(rel) = f.location.path.strip_prefix(root) {
            if !rel.as_str().is_empty() {
                f.location.path = rel.to_owned();
            }
        }
        // Identity spelling is `/`-separated on every OS: baselines saved on
        // Linux CI must match a Windows checkout, and `.mollifyrc` `ignore`
        // patterns are written with `/`.
        if f.location.path.as_str().contains('\\') {
            f.location.path =
                camino::Utf8PathBuf::from(f.location.path.as_str().replace('\\', "/"));
        }
    }
}

/// Drop findings silenced by an inline `# mollify: ignore[<rule>]` comment on
/// the finding's line (or a bare `# mollify: ignore` matching any rule).
/// Expects relativized findings (`location.path` keyed by module `rel`).
pub fn apply_suppressions(graph: &ModuleGraph, findings: &mut Vec<Finding>) {
    use rustc_hash::FxHashMap;
    // (path, line) -> set of suppressed rules ("*" = all).
    let mut sup: FxHashMap<(&str, u32), Vec<&str>> = FxHashMap::default();
    for m in &graph.modules {
        for (line, rule) in &m.parsed.ignores {
            sup.entry((m.rel.as_str(), *line))
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

/// Run one engine with panic isolation: a bug in a single engine must degrade
/// to an `engine-panic` finding, not kill the whole report. (A real blowup in
/// the dupes engine previously took `audit` down with it on three corpus
/// repos.) This catches panics only — resource exhaustion that ends in an OOM
/// kill is a process-level signal nothing in-process can intercept.
///
/// The findings the engine produced before panicking are lost by design:
/// partial engine output would silently change summary counts run-to-run.
fn run_engine<F>(name: &str, category: Category, out: &mut Vec<Finding>, f: F)
where
    F: FnOnce() -> Vec<Finding>,
{
    // Engines only read the graph/config; nothing they touch is observable
    // after a panic, so unwinding across them is safe to assert.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(findings) => out.extend(findings),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            out.push(Finding {
                fingerprint: crate::fingerprint::fingerprint("engine-panic", &[name]),
                rule: "engine-panic".into(),
                category,
                severity: Severity::Error,
                confidence: Confidence::Certain,
                attribution: None,
                reason: format!(
                    "the {name} engine panicked ({msg}); its findings are missing from this report — please file a bug"
                ),
                location: mollify_types::Location {
                    path: ".".into(),
                    line: 1,
                    column: 0,
                    end_line: None,
                },
                actions: vec![mollify_types::Action {
                    kind: "report-bug".into(),
                    description: format!(
                        "Report the {name} engine panic to the mollify issue tracker"
                    ),
                    auto_fixable: false,
                    suppression_comment: None,
                }],
            });
        }
    }
}

/// `mollify dead-code` — reachability-based unused files/symbols.
pub fn dead_code_report(root: &Utf8Path) -> FindingsReport {
    dead_code_report_with_includes(root, &[])
}

/// Like [`dead_code_report`], honoring the CLI's `--include` override.
pub fn dead_code_report_with_includes(root: &Utf8Path, includes: &[String]) -> FindingsReport {
    let graph = build_graph_with_includes(root, includes);
    let mut findings = Vec::new();
    run_engine("dead-code", Category::DeadCode, &mut findings, || {
        deadcode::analyze_with(
            &graph,
            &paths::pytest_testpaths(root),
            &deps::entry_point_symbols(root),
        )
    });
    run_engine("members", Category::DeadCode, &mut findings, || {
        members::analyze(&graph)
    });
    run_engine("commented-code", Category::DeadCode, &mut findings, || {
        commented::analyze(&graph)
    });
    finalize(root, &config::load(root), &graph, findings)
}

/// `mollify deps` — dependency hygiene.
pub fn deps_report(root: &Utf8Path) -> FindingsReport {
    deps_report_with_includes(root, &[])
}

/// Like [`deps_report`], honoring the CLI's `--include` override.
pub fn deps_report_with_includes(root: &Utf8Path, includes: &[String]) -> FindingsReport {
    let graph = build_graph_with_includes(root, includes);
    let mut findings = Vec::new();
    run_engine("deps", Category::DependencyHygiene, &mut findings, || {
        deps::analyze(root, &graph)
    });
    run_engine(
        "unresolved-imports",
        Category::DependencyHygiene,
        &mut findings,
        || deps::unresolved(&graph),
    );
    finalize(root, &config::load(root), &graph, findings)
}

/// `mollify arch` — circular dependencies (boundary presets later).
pub fn arch_report(root: &Utf8Path) -> FindingsReport {
    arch_report_with_includes(root, &[])
}

/// Like [`arch_report`], honoring the CLI's `--include` override.
pub fn arch_report_with_includes(root: &Utf8Path, includes: &[String]) -> FindingsReport {
    let graph = build_graph_with_includes(root, includes);
    let cfg = config::load(root);
    let mut findings = Vec::new();
    run_engine("arch", Category::Architecture, &mut findings, || {
        let mut f = arch::analyze(&graph);
        f.extend(arch::analyze_layers(&graph, &cfg.arch_layers));
        f.extend(arch::analyze_contracts(&graph, &cfg.contracts));
        f.extend(arch::private_imports(&graph));
        f
    });
    run_engine("policy", Category::Architecture, &mut findings, || {
        policy::analyze(&graph, &cfg.policies)
    });
    finalize(root, &cfg, &graph, findings)
}

/// `mollify complexity` / `mollify health` — complexity hotspots.
pub fn complexity_report(root: &Utf8Path) -> FindingsReport {
    complexity_report_with_includes(root, &[])
}

/// Like [`complexity_report`], honoring the CLI's `--include` override.
pub fn complexity_report_with_includes(root: &Utf8Path, includes: &[String]) -> FindingsReport {
    let graph = build_graph_with_includes(root, includes);
    let cfg = config::load(root);
    let mut findings = Vec::new();
    run_engine("complexity", Category::Complexity, &mut findings, || {
        complexity::analyze_with(&graph, cfg.max_cyclomatic, cfg.max_cognitive)
    });
    run_engine("hotspots", Category::Complexity, &mut findings, || {
        hotspots::analyze(root, &graph)
    });
    run_engine("cohesion", Category::Complexity, &mut findings, || {
        cohesion::analyze(&graph)
    });
    finalize(root, &cfg, &graph, findings)
}

/// `mollify dupes` — duplication / clone families.
pub fn dupes_report(root: &Utf8Path) -> FindingsReport {
    dupes_report_with_includes(root, &[])
}

/// Like [`dupes_report`], honoring the CLI's `--include` override.
pub fn dupes_report_with_includes(root: &Utf8Path, includes: &[String]) -> FindingsReport {
    let graph = build_graph_with_includes(root, includes);
    let cfg = config::load(root);
    let mut findings = Vec::new();
    run_engine("dupes", Category::Duplication, &mut findings, || {
        dupes::analyze_with(&graph, cfg.dup_min_tokens, cfg.dup_min_lines)
    });
    finalize(root, &cfg, &graph, findings)
}

/// `mollify types` — type-annotation health + API-hygiene (private-type leaks).
pub fn types_report(root: &Utf8Path) -> FindingsReport {
    types_report_with_includes(root, &[])
}

/// Like [`types_report`], honoring the CLI's `--include` override.
pub fn types_report_with_includes(root: &Utf8Path, includes: &[String]) -> FindingsReport {
    let graph = build_graph_with_includes(root, includes);
    let mut findings = Vec::new();
    run_engine("type-health", Category::TypeHealth, &mut findings, || {
        typehealth::analyze(&graph)
    });
    run_engine("api-hygiene", Category::TypeHealth, &mut findings, || {
        apihygiene::analyze(&graph)
    });
    finalize(root, &config::load(root), &graph, findings)
}

/// `mollify security` — security candidates (deterministic; review before acting).
pub fn security_report(root: &Utf8Path) -> FindingsReport {
    security_report_with_includes(root, &[])
}

/// Like [`security_report`], honoring the CLI's `--include` override.
pub fn security_report_with_includes(root: &Utf8Path, includes: &[String]) -> FindingsReport {
    let graph = build_graph_with_includes(root, includes);
    let mut findings = Vec::new();
    run_engine("security", Category::Security, &mut findings, || {
        security::analyze(&graph, &paths::pytest_testpaths(root))
    });
    finalize(root, &config::load(root), &graph, findings)
}

/// `mollify coverage` — cold-path analysis from a coverage.py JSON report.
pub fn coverage_report(root: &Utf8Path, coverage_path: &Utf8Path) -> FindingsReport {
    let graph = build_graph(root);
    let findings = coverage::analyze(root, &graph, coverage_path);
    finalize(root, &config::load(root), &graph, findings)
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
    finalize(root, &config::load(root), &graph, findings)
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

/// Returns true if `path` matches the user's `file` argument: exact, or as a
/// trailing path fragment anchored at a path-separator boundary (`b.py`
/// matches `pkg/b.py` but never `lib.py`).
fn path_matches(path: &str, file: &str) -> bool {
    path == file || path.ends_with(&format!("/{file}"))
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
    // Unused local variables / parameters. (Live-buffer path: the display
    // path doubles as the fingerprint identity; occurrence keeps the scheme
    // aligned with the batch engines.)
    let mut occ = fingerprint::Occurrences::default();
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
                &[path.as_str(), &s.name, &occ.next(&s.name)],
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
    let mut fn_occ = fingerprint::Occurrences::default();
    for f in &parsed.functions {
        let occurrence = fn_occ.next(&f.name);
        if f.cyclomatic > complexity::DEFAULT_CYCLOMATIC
            || f.cognitive > complexity::DEFAULT_COGNITIVE
        {
            findings.push(Finding {
                fingerprint: fingerprint::fingerprint(
                    "high-complexity",
                    &[path.as_str(), &f.name, &occurrence],
                ),
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
            .map(|m| format!("{}\t{}", m.dotted, m.rel))
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
            .map(|m| format!("{}\t{}", m.dotted, m.rel))
            .collect(),
    };
    rows.sort();
    rows
}

/// `mollify audit` — the unified pass across all engines. Produces a quality
/// score over the combined findings.
pub fn audit_report(root: &Utf8Path) -> AuditReport {
    audit_report_with_includes(root, &[])
}

/// Like [`audit_report`], honoring the CLI's `--include` override.
pub fn audit_report_with_includes(root: &Utf8Path, includes: &[String]) -> AuditReport {
    let graph = build_graph_with_includes(root, includes);
    let cfg = config::load(root);
    let mut findings: Vec<Finding> = Vec::new();
    run_engine("dead-code", Category::DeadCode, &mut findings, || {
        deadcode::analyze_with(
            &graph,
            &paths::pytest_testpaths(root),
            &deps::entry_point_symbols(root),
        )
    });
    run_engine("members", Category::DeadCode, &mut findings, || {
        members::analyze(&graph)
    });
    run_engine("commented-code", Category::DeadCode, &mut findings, || {
        commented::analyze(&graph)
    });
    run_engine("deps", Category::DependencyHygiene, &mut findings, || {
        let mut f = deps::analyze(root, &graph);
        f.extend(deps::unresolved(&graph));
        f
    });
    run_engine("arch", Category::Architecture, &mut findings, || {
        let mut f = arch::analyze(&graph);
        f.extend(arch::analyze_layers(&graph, &cfg.arch_layers));
        f.extend(arch::analyze_contracts(&graph, &cfg.contracts));
        f.extend(arch::private_imports(&graph));
        f
    });
    run_engine("policy", Category::Architecture, &mut findings, || {
        policy::analyze(&graph, &cfg.policies)
    });
    run_engine("complexity", Category::Complexity, &mut findings, || {
        complexity::analyze_with(&graph, cfg.max_cyclomatic, cfg.max_cognitive)
    });
    run_engine("dupes", Category::Duplication, &mut findings, || {
        dupes::analyze_with(&graph, cfg.dup_min_tokens, cfg.dup_min_lines)
    });
    run_engine("type-health", Category::TypeHealth, &mut findings, || {
        typehealth::analyze(&graph)
    });
    run_engine("api-hygiene", Category::TypeHealth, &mut findings, || {
        apihygiene::analyze(&graph)
    });
    run_engine("security", Category::Security, &mut findings, || {
        security::analyze(&graph, &paths::pytest_testpaths(root))
    });
    run_engine("hotspots", Category::Complexity, &mut findings, || {
        hotspots::analyze(root, &graph)
    });
    run_engine("cohesion", Category::Complexity, &mut findings, || {
        cohesion::analyze(&graph)
    });
    // Supply-chain runs only when a local advisory DB is present (keeps audit
    // offline + deterministic; no implicit network).
    let db_path = root.join(DEFAULT_ADVISORY_DB);
    if let Some(advisories) = supplychain::load_db(&db_path) {
        run_engine(
            "supply-chain",
            Category::DependencyHygiene,
            &mut findings,
            || supplychain::analyze(root, &advisories),
        );
    }
    relativize_paths(root, &mut findings);
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

/// A simple, deterministic 0–100 health score: start at 100, subtract weighted
/// penalties per finding (errors hurt more than warnings), floor at 0.
///
/// Penalties are scaled by **confidence** so that low-confidence candidates —
/// which are, by design, the noisier tier — don't tank the headline number the
/// way a confirmed defect does. A repo full of `Uncertain` findings should not
/// read the same as one full of `Certain` ones (a real-world audit scored
/// 20/100 almost entirely on uncertain false positives).
pub fn quality_score(findings: &[Finding], files: usize) -> u8 {
    if files == 0 {
        return 100;
    }
    let mut penalty = 0.0f64;
    for f in findings {
        let severity_weight = match f.severity {
            Severity::Error => 3.0,
            Severity::Warn => 1.0,
            // `Off` and any future severity (#[non_exhaustive]) score zero.
            _ => 0.0,
        };
        let confidence_weight = match f.confidence {
            Confidence::Certain => 1.0,
            Confidence::Likely => 0.5,
            // `Uncertain` and any future tier score as the noisiest tier.
            _ => 0.15,
        };
        penalty += severity_weight * confidence_weight;
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
    use mollify_types::Report;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-lib-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn windows_spelled_paths_share_one_identity() {
        // The identity layer (serialized location.path, and therefore what
        // baselines and `.mollifyrc` patterns see) must be `/`-separated on
        // every OS: a fingerprint computed from a `\`-spelled rel must equal
        // the `/`-spelled one.
        let mut findings = vec![Finding {
            fingerprint: "x".into(),
            rule: "unused-import".into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence: Confidence::Certain,
            attribution: None,
            reason: "r".into(),
            location: mollify_types::Location {
                path: r"billing\app.py".into(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![],
        }];
        relativize_paths(Utf8Path::new("."), &mut findings);
        assert_eq!(findings[0].location.path.as_str(), "billing/app.py");
        assert_eq!(
            fingerprint::fingerprint("unused-import", &[findings[0].location.path.as_str(), "os"]),
            fingerprint::fingerprint("unused-import", &["billing/app.py", "os"]),
        );
    }

    #[test]
    fn engine_panic_degrades_to_finding() {
        // One sick engine must not kill the report: the dupes OOM previously
        // took `audit` down with it on three corpus repos.
        let mut findings = Vec::new();
        run_engine("boom", Category::Duplication, &mut findings, || {
            panic!("synthetic engine failure")
        });
        run_engine("fine", Category::DeadCode, &mut findings, Vec::new);
        assert_eq!(findings.len(), 1, "got {findings:?}");
        let f = &findings[0];
        assert_eq!(f.rule, "engine-panic");
        assert_eq!(f.severity, Severity::Error);
        assert!(f.reason.contains("boom") && f.reason.contains("synthetic"));
    }

    #[test]
    fn noqa_suppresses_unused_import_end_to_end() {
        // The WSGI entry-point idiom from flask's test apps: the import is the
        // module's entire purpose and the author already said so with noqa.
        let d = temp("noqa");
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        std::fs::write(d.join("hello.py"), "app = object()\n").unwrap();
        std::fs::write(d.join("wsgi.py"), "from hello import app  # noqa: F401\n").unwrap();
        let r = dead_code_report(&d);
        assert!(
            !r.findings
                .iter()
                .any(|f| f.rule == "unused-import" && f.location.path.as_str().contains("wsgi")),
            "noqa'd import leaked: {:?}",
            r.findings
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn report_paths_are_root_relative_regardless_of_root_spelling() {
        // Identical input must yield byte-identical output whether the root
        // was `.`, `sub/dir`, or absolute — so location paths must never echo
        // the caller's spelling, and machine-specific prefixes must not leak.
        let d = temp("relpath");
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        std::fs::write(d.join("lib.py"), "import os\n").unwrap();
        let abs = dead_code_report(&d);
        let f = abs
            .findings
            .iter()
            .find(|f| f.rule == "unused-import")
            .expect("unused-import finding");
        assert_eq!(f.location.path.as_str(), "lib.py", "got {:?}", f.location);
        // Same tree via a different spelling of the root (`…/x/.`): the
        // serialized findings must be identical byte for byte.
        let respelled = dead_code_report(&d.join("."));
        assert_eq!(
            serde_json::to_string(&abs.findings).unwrap(),
            serde_json::to_string(&respelled.findings).unwrap(),
            "root spelling changed the output"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn security_confidence_capped_in_dev_trees() {
        let d = temp("devtree");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("tests")).unwrap();
        std::fs::write(d.join("src/app.py"), "import requests\nrequests.get(url)\n").unwrap();
        std::fs::write(
            d.join("tests/test_app.py"),
            "import subprocess\nsubprocess.run(c, shell=True)\n",
        )
        .unwrap();
        let r = security_report(&d);
        let test_hit = r
            .findings
            .iter()
            .find(|f| f.location.path.as_str().starts_with("tests/"))
            .expect("test-tree security candidate");
        assert_eq!(test_hit.confidence, Confidence::Uncertain);
        assert!(
            test_hit.reason.contains("test/docs/example code"),
            "reason not tagged: {}",
            test_hit.reason
        );
        // Production hits keep their per-rule confidence (shell=True → Likely).
        let src_hit = r
            .findings
            .iter()
            .find(|f| f.location.path.as_str().starts_with("src/"))
            .expect("src security candidate");
        assert!(!src_hit.reason.contains("test/docs/example"));
        std::fs::remove_dir_all(&d).ok();
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

    #[test]
    fn score_weights_penalty_by_confidence() {
        fn finding(conf: Confidence) -> Finding {
            Finding {
                fingerprint: "x".into(),
                rule: "r".into(),
                category: Category::DeadCode,
                severity: Severity::Warn,
                confidence: conf,
                attribution: None,
                reason: String::new(),
                location: mollify_types::Location {
                    path: Utf8PathBuf::from("a.py"),
                    line: 1,
                    column: 0,
                    end_line: None,
                },
                actions: vec![],
            }
        }
        // Same count + severity, different confidence → uncertain hurts least.
        let repeat = |c, n| vec![finding(c); n];
        let certain = quality_score(&repeat(Confidence::Certain, 5), 1);
        let uncertain = quality_score(&repeat(Confidence::Uncertain, 5), 1);
        assert!(
            uncertain > certain,
            "uncertain {uncertain} should beat certain {certain}"
        );
        // No findings → perfect score.
        assert_eq!(quality_score(&[], 3), 100);
    }

    #[test]
    fn src_layout_entry_point_suppresses_dead_code() {
        // A src/ layout: `src/pkg/cli.py` is named by a console-script entry
        // point. `dotted_name` strips the leading `src/`, so the dotted name is
        // `pkg.cli` and the entry-point wiring matches it. The module must not be
        // `unused-file`, and its `main` must not be `unused-export`.
        let d = temp("srclayout");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"pkg\"\n\n[project.scripts]\nserve = \"pkg.cli:main\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(d.join("src/pkg")).unwrap();
        std::fs::write(d.join("src/pkg/__init__.py"), "").unwrap();
        std::fs::write(
            d.join("src/pkg/cli.py"),
            "def main():\n    return 0\n\n\ndef _orphan():\n    return 1\n",
        )
        .unwrap();
        let report = dead_code_report(&d);
        let dead: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule == "unused-file" || f.rule == "unused-export")
            .map(|f| f.reason.clone())
            .collect();
        assert!(
            !dead
                .iter()
                .any(|r| r.contains("pkg.cli") || r.contains("`main`")),
            "entry-point module/function wrongly flagged in src/ layout: {dead:?}"
        );
        // The genuinely-dead sibling is still flagged (sanity).
        assert!(
            dead.iter().any(|r| r.contains("_orphan")),
            "real dead code missed: {dead:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }
}
