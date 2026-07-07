//! The `mollify` command-line interface.
//!
//! Analysis: `audit`, `dead-code`, `deps`, `arch`, `complexity`, `dupes`,
//! `types`, `security`, `coverage`, `supply-chain`. Workflow: `fix`, `explain`,
//! `trace`, `inspect`, `list`, `metrics`, `graph`, `watch`, `init`, `mcp`,
//! `lsp`. Analysis commands support
//! `--format human|json|sarif|github|junit`, `--path`, `--gate`, and regression baselines
//! (`--save-baseline`/`--baseline`/`--fail-on-regression`/`--brief`). JSON is the
//! kind-discriminated contract from `mollify-types`.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use mollify_types::{Confidence, Finding, Report, Severity, Summary};

mod osv;
mod update_check;

#[derive(Parser)]
#[command(
    name = "mollify",
    version,
    about = "Deterministic codebase intelligence for Python — evidence, not decisions."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Full unified report across all engines, with a 0–100 quality score.
    Audit(Scope),
    /// Reachability-based unused files and symbols.
    #[command(name = "dead-code", alias = "check")]
    DeadCode(Scope),
    /// Dependency hygiene (unused / missing distributions).
    Deps(Scope),
    /// Architecture checks (circular dependencies).
    Arch(Scope),
    /// Complexity hotspots (cyclomatic + cognitive).
    #[command(name = "complexity", alias = "health")]
    Complexity(Scope),
    /// Duplication / clone families.
    Dupes(Scope),
    /// Type-annotation health (fully-untyped public functions).
    Types(Scope),
    /// Security candidates (bandit-style; review before acting).
    Security(Scope),
    /// Cold-path analysis: functions never executed in a coverage.py JSON report.
    Coverage(CoverageArgs),
    /// Supply-chain: match pinned/locked versions against a local advisory DB.
    #[command(name = "supply-chain")]
    SupplyChain(SupplyChainArgs),
    /// Apply safe auto-fixes (certain, auto-fixable unused symbols). Dry-run unless --apply.
    Fix(FixArgs),
    /// Explain a rule id (semantics, confidence, how to act). No argument lists all rules.
    Explain(ExplainArgs),
    /// Show a module's import neighborhood: what it imports and what imports it.
    Trace(TraceArgs),
    /// Re-run `audit` whenever a Python file changes (poll-based; Ctrl-C to stop).
    Watch(WatchArgs),
    /// Evidence bundle for a single file: its findings plus its import neighborhood.
    Inspect(InspectArgs),
    /// List project topology: entry points, modules, and detected frameworks.
    List(ListArgs),
    /// Code metrics: Maintainability Index, Halstead, raw LOC, per-file complexity.
    Metrics(MetricsArgs),
    /// Export the module import graph as Graphviz DOT (or Mermaid with --mermaid).
    Graph(GraphArgs),
    /// Scaffold a .mollifyrc, or install agent integrations with --agent.
    Init(InitArgs),
    /// Run the Model Context Protocol server over stdio (for coding agents).
    Mcp,
    /// Run the Language Server (LSP) over stdio (real-time editor diagnostics).
    Lsp,
}

#[derive(clap::Args)]
struct InitArgs {
    /// Project root to scaffold into.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Install integration files for one or more agents (claude, cursor,
    /// gemini, codex, cascade). Repeatable. Without this, a .mollifyrc is written.
    #[arg(long, value_name = "AGENT")]
    agent: Vec<String>,
    /// Install integrations for every supported agent.
    #[arg(long)]
    all: bool,
    /// Overwrite existing files (default: skip files that already exist).
    #[arg(long)]
    force: bool,
}

#[derive(clap::Args)]
struct CoverageArgs {
    /// Project root.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Path to a coverage.py JSON report (produced by `coverage json`).
    #[arg(long)]
    coverage_file: Utf8PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(clap::Args)]
struct ExplainArgs {
    /// Rule id to explain (e.g. `circular-dependency`). Omit to list all rules.
    rule: Option<String>,
}

#[derive(clap::Args)]
struct TraceArgs {
    /// Module to trace (dotted name or trailing segment, e.g. `app.db` or `db`).
    module: String,
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(clap::Args)]
struct SupplyChainArgs {
    /// Project root.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Advisory DB JSON (mollify-advisories/1). Defaults to `.mollify/advisories.json`.
    #[arg(long)]
    advisory_db: Option<Utf8PathBuf>,
    /// Skip the live OSV fetch; use the local advisory DB only (deterministic).
    #[arg(long)]
    offline: bool,
    /// After a live fetch, write the advisories to the DB path (cache for offline runs).
    #[arg(long)]
    refresh: bool,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(clap::Args)]
struct InspectArgs {
    /// File to inspect (path, or trailing path fragment).
    file: String,
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(clap::Args)]
struct ListArgs {
    /// What to list.
    #[arg(value_enum, default_value_t = ListKind::EntryPoints)]
    kind: ListKind,
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ListKind {
    #[value(name = "entry-points")]
    EntryPoints,
    Files,
    Frameworks,
}

#[derive(clap::Args)]
struct MetricsArgs {
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(clap::Args)]
struct GraphArgs {
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Emit Mermaid `flowchart` instead of Graphviz DOT.
    #[arg(long)]
    mermaid: bool,
}

#[derive(clap::Args)]
struct WatchArgs {
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Poll interval in milliseconds.
    #[arg(long, default_value_t = 1000)]
    interval_ms: u64,
}

#[derive(clap::Args)]
struct FixArgs {
    /// Project root.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Write the changes (default is a dry-run preview).
    #[arg(long)]
    apply: bool,
}

#[derive(clap::Args)]
struct Scope {
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
    /// Attribution gate: `all` (default) or `new-only` (only findings in changed files).
    #[arg(long, value_enum, default_value_t = Gate::All)]
    gate: Gate,
    /// Base git ref to diff against for `--gate new-only` (e.g. origin/main).
    #[arg(long)]
    base: Option<String>,
    /// Write a regression baseline (set of finding fingerprints) to this path and exit 0.
    #[arg(long)]
    save_baseline: Option<Utf8PathBuf>,
    /// Compare against a saved baseline; mark/keep only findings new since then.
    #[arg(long)]
    baseline: Option<Utf8PathBuf>,
    /// With `--baseline`, exit non-zero if any new findings appeared (CI gate).
    #[arg(long)]
    fail_on_regression: bool,
    /// Advisory mode: print the report but always exit 0 (never gate CI).
    #[arg(long)]
    brief: bool,
    /// Only show findings at least this confident (certain > likely > uncertain).
    #[arg(long, value_enum)]
    min_confidence: Option<ConfidenceArg>,
    /// Scan this directory name despite the builtin exclude list,
    /// .mollifyrc.json's exclude_dirs, or .gitignore (e.g. --include
    /// node_modules). Repeatable.
    #[arg(long, value_name = "DIR")]
    include: Vec<String>,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ConfidenceArg {
    Certain,
    Likely,
    Uncertain,
}

/// Drop findings less confident than `--min-confidence`. Certainty order is
/// certain > likely > uncertain (the enum's `Ord` runs certain < uncertain).
fn apply_min_confidence(s: &Scope, findings: &mut Vec<Finding>) {
    let Some(min) = s.min_confidence else { return };
    let threshold = match min {
        ConfidenceArg::Certain => Confidence::Certain,
        ConfidenceArg::Likely => Confidence::Likely,
        ConfidenceArg::Uncertain => Confidence::Uncertain,
    };
    findings.retain(|f| f.confidence <= threshold);
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Human,
    Json,
    Sarif,
    /// GitHub Actions workflow annotations (`::error file=…`).
    Github,
    /// JUnit XML (one testcase per finding) for CI dashboards.
    Junit,
}

/// Render findings as GitHub Actions annotations.
fn github_annotations(findings: &[Finding]) -> String {
    let mut s = String::new();
    for f in findings {
        let level = if f.severity == Severity::Error {
            "error"
        } else {
            "warning"
        };
        s.push_str(&format!(
            "::{level} file={},line={}::{}: {}\n",
            f.location.path, f.location.line, f.rule, f.reason
        ));
    }
    s
}

/// Render findings as JUnit XML (one testcase per finding).
fn junit_xml(findings: &[Finding], suite: &str) -> String {
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(&format!(
        "<testsuites>\n  <testsuite name=\"mollify:{}\" tests=\"{}\" failures=\"{}\">\n",
        esc(suite),
        findings.len(),
        findings.len()
    ));
    for f in findings {
        let name = format!("{}: {}:{}", f.rule, f.location.path, f.location.line);
        s.push_str(&format!(
            "    <testcase name=\"{}\" classname=\"{}\">\n      <failure message=\"{}\"/>\n    </testcase>\n",
            esc(&name),
            esc(f.rule.as_str()),
            esc(&f.reason)
        ));
    }
    s.push_str("  </testsuite>\n</testsuites>\n");
    s
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Gate {
    All,
    #[value(name = "new-only")]
    NewOnly,
}

/// Apply introduced/inherited attribution from git, and (for `new-only`) filter
/// to introduced findings. Returns nothing; mutates `findings` in place.
fn apply_gate(scope: &Scope, findings: &mut Vec<mollify_types::Finding>) {
    use mollify_types::Attribution;
    if scope.gate == Gate::All && scope.base.is_none() {
        return; // no attribution requested
    }
    let Some(changed) = mollify_core::git::changed_files(&scope.path, scope.base.as_deref()) else {
        eprintln!("mollify: --gate requested but this isn't a git repo; reporting all findings.");
        return;
    };
    // Prefer line-level attribution (a finding is introduced only if its line is
    // in a changed hunk); fall back to file-level when no line info is available.
    let lines = mollify_core::git::changed_lines(&scope.path, scope.base.as_deref());
    for f in findings.iter_mut() {
        let file_changed =
            mollify_core::git::path_is_changed(&scope.path, &f.location.path, &changed);
        let introduced = match lines.as_ref().and_then(|m| {
            mollify_core::git::line_is_changed(&scope.path, &f.location.path, f.location.line, m)
        }) {
            Some(in_hunk) => in_hunk,
            None => file_changed,
        };
        f.attribution = Some(if introduced {
            Attribution::Introduced
        } else {
            Attribution::Inherited
        });
    }
    if scope.gate == Gate::NewOnly {
        findings.retain(|f| f.attribution == Some(Attribution::Introduced));
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Audit(s) => run_audit(&s),
        Command::DeadCode(s) => run_findings(
            &s,
            mollify_core::dead_code_report_with_includes,
            Report::DeadCode,
            "dead-code",
        ),
        Command::Deps(s) => run_findings(
            &s,
            mollify_core::deps_report_with_includes,
            Report::Deps,
            "deps",
        ),
        Command::Arch(s) => run_findings(
            &s,
            mollify_core::arch_report_with_includes,
            Report::Arch,
            "arch",
        ),
        Command::Complexity(s) => run_findings(
            &s,
            mollify_core::complexity_report_with_includes,
            Report::Complexity,
            "complexity",
        ),
        Command::Dupes(s) => run_findings(
            &s,
            mollify_core::dupes_report_with_includes,
            Report::Dupes,
            "dupes",
        ),
        Command::Types(s) => run_findings(
            &s,
            mollify_core::types_report_with_includes,
            Report::Types,
            "types",
        ),
        Command::Security(s) => run_findings(
            &s,
            mollify_core::security_report_with_includes,
            Report::Security,
            "security",
        ),
        Command::Coverage(a) => run_coverage(&a),
        Command::SupplyChain(a) => run_supply_chain(&a),
        Command::Fix(a) => run_fix(&a),
        Command::Explain(a) => run_explain(&a),
        Command::Trace(a) => run_trace(&a),
        Command::Watch(a) => run_watch(&a),
        Command::Inspect(a) => run_inspect(&a),
        Command::List(a) => run_list(&a),
        Command::Metrics(a) => run_metrics(&a),
        Command::Graph(a) => match validate_root(&a.path) {
            Some(code) => code,
            None => {
                print!("{}", mollify_core::graph_export(&a.path, a.mermaid));
                0
            }
        },
        Command::Init(a) => run_init(&a),
        Command::Mcp => match mollify_mcp::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("mollify mcp: {e}");
                1
            }
        },
        Command::Lsp => match mollify_lsp::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("mollify lsp: {e}");
                1
            }
        },
    };
    std::process::exit(code);
}

/// Reject a `--path` that doesn't exist or isn't a directory. Analyzing a
/// nonexistent root must never yield a clean empty report; exit 2 keeps it
/// distinct from the findings gate's exit 1.
fn validate_root(path: &camino::Utf8Path) -> Option<i32> {
    if path.is_dir() {
        None
    } else {
        eprintln!("error: project root `{path}` does not exist or is not a directory");
        Some(2)
    }
}

/// Reject `--format` values a command doesn't implement (exit 2) instead of
/// silently falling through to human output.
fn require_human_or_json(cmd: &str, format: Format) -> Option<i32> {
    if matches!(format, Format::Human | Format::Json) {
        None
    } else {
        eprintln!("error: unsupported format for this command (`{cmd}` supports human and json)");
        Some(2)
    }
}

/// Outcome of applying baseline options to a findings set.
enum BaselineOutcome {
    /// `--save-baseline` wrote a snapshot; the note goes to stderr and the
    /// report still prints (exit 0).
    Saved(Utf8PathBuf),
    /// `--save-baseline` could not write the snapshot; the run must fail.
    SaveFailed,
    /// `--baseline` filtered to findings new since the snapshot; `usize` is how many.
    Filtered(usize),
    /// `--fail-on-regression` was set but the baseline is missing/invalid.
    LoadFailed,
    /// No baseline options in effect.
    None,
}

/// Hard baseline failures abort the run before any report is emitted.
fn baseline_failure_exit(outcome: &BaselineOutcome) -> Option<i32> {
    match outcome {
        BaselineOutcome::SaveFailed | BaselineOutcome::LoadFailed => Some(1),
        _ => None,
    }
}

/// Apply `--save-baseline` / `--baseline` to `findings`. With `--baseline`,
/// retains only findings that are new relative to the snapshot.
fn handle_baseline(s: &Scope, findings: &mut Vec<mollify_types::Finding>) -> BaselineOutcome {
    use mollify_core::baseline::{split_new, Baseline};
    if let Some(path) = &s.save_baseline {
        let b = Baseline::from_findings(findings);
        if let Err(e) = b.save(path) {
            eprintln!("error: could not write baseline {path}: {e}");
            return BaselineOutcome::SaveFailed;
        }
        return BaselineOutcome::Saved(path.clone());
    }
    if let Some(path) = &s.baseline {
        let Some(b) = Baseline::load(path) else {
            // A CI regression gate with no baseline must fail loudly, never
            // degrade to "report everything, exit by severity".
            if s.fail_on_regression {
                eprintln!(
                    "error: --fail-on-regression requires a readable baseline; {path} is missing or invalid"
                );
                return BaselineOutcome::LoadFailed;
            }
            eprintln!("mollify: baseline {path} missing or invalid; reporting all findings.");
            return BaselineOutcome::None;
        };
        let new_count = {
            let (new, _known) = split_new(findings, &b);
            let keep: std::collections::HashSet<String> =
                new.iter().map(|f| f.fingerprint.clone()).collect();
            findings.retain(|f| keep.contains(&f.fingerprint));
            findings.len()
        };
        return BaselineOutcome::Filtered(new_count);
    }
    BaselineOutcome::None
}

/// Final exit code honoring `--brief` (always 0) and `--fail-on-regression`.
fn gated_exit(s: &Scope, errors: usize, outcome: &BaselineOutcome) -> i32 {
    if s.brief {
        return 0;
    }
    // `--save-baseline` is documented to exit 0: the snapshot is the product.
    if let BaselineOutcome::Saved(_) = outcome {
        return 0;
    }
    if s.fail_on_regression {
        if let BaselineOutcome::Filtered(n) = outcome {
            if *n > 0 {
                return 1;
            }
        }
    }
    exit_code(errors)
}

/// Recompute summary and quality score from the (possibly filtered) findings.
/// `--gate`/`--min-confidence`/`--baseline` may have dropped findings, and the
/// emitted score must reflect what is emitted.
fn rescore_audit(report: &mut mollify_types::AuditReport) {
    report.summary =
        mollify_types::Summary::from_findings(&report.findings, report.summary.files_analyzed);
    report.quality_score =
        mollify_core::quality_score(&report.findings, report.summary.files_analyzed);
}

fn run_audit(s: &Scope) -> i32 {
    if let Some(code) = validate_root(&s.path) {
        return code;
    }
    let mut report = mollify_core::audit_report_with_includes(&s.path, &s.include);
    apply_gate(s, &mut report.findings);
    apply_min_confidence(s, &mut report.findings);
    let outcome = handle_baseline(s, &mut report.findings);
    if let Some(code) = baseline_failure_exit(&outcome) {
        return code;
    }
    if let BaselineOutcome::Saved(p) = &outcome {
        // stderr, so `--format json` stdout stays pure protocol.
        eprintln!(
            "mollify: wrote baseline with {} fingerprint(s) to {p}",
            report.findings.len()
        );
    }
    rescore_audit(&mut report);
    let errors = report.summary.errors;
    match s.format {
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&Report::Audit(report)).unwrap()
        ),
        Format::Sarif => println!(
            "{}",
            serde_json::to_string_pretty(&mollify_core::sarif::to_sarif(
                &report.findings,
                env!("CARGO_PKG_VERSION")
            ))
            .unwrap()
        ),
        Format::Github => print!("{}", github_annotations(&report.findings)),
        Format::Junit => print!("{}", junit_xml(&report.findings, "audit")),
        Format::Human => {
            println!("Mollify audit — {}", s.path);
            println!("Quality score: {}/100", report.quality_score);
            print_summary(&report.summary);
            print_findings(&report.findings);
            update_check::maybe_nudge();
        }
    }
    gated_exit(s, errors, &outcome)
}

fn run_findings(
    s: &Scope,
    f: fn(&camino::Utf8Path, &[String]) -> mollify_types::FindingsReport,
    wrap: fn(mollify_types::FindingsReport) -> Report,
    label: &str,
) -> i32 {
    if let Some(code) = validate_root(&s.path) {
        return code;
    }
    let mut report = f(&s.path, &s.include);
    apply_gate(s, &mut report.findings);
    apply_min_confidence(s, &mut report.findings);
    let outcome = handle_baseline(s, &mut report.findings);
    if let Some(code) = baseline_failure_exit(&outcome) {
        return code;
    }
    if let BaselineOutcome::Saved(p) = &outcome {
        // stderr, so `--format json` stdout stays pure protocol.
        eprintln!(
            "mollify: wrote baseline with {} fingerprint(s) to {p}",
            report.findings.len()
        );
    }
    report.summary =
        mollify_types::Summary::from_findings(&report.findings, report.summary.files_analyzed);
    let errors = report.summary.errors;
    match s.format {
        Format::Json => println!("{}", serde_json::to_string_pretty(&wrap(report)).unwrap()),
        Format::Sarif => println!(
            "{}",
            serde_json::to_string_pretty(&mollify_core::sarif::to_sarif(
                &report.findings,
                env!("CARGO_PKG_VERSION")
            ))
            .unwrap()
        ),
        Format::Github => print!("{}", github_annotations(&report.findings)),
        Format::Junit => print!("{}", junit_xml(&report.findings, label)),
        Format::Human => {
            println!("Mollify {label} — {}", s.path);
            print_summary(&report.summary);
            print_findings(&report.findings);
            update_check::maybe_nudge();
        }
    }
    gated_exit(s, errors, &outcome)
}

fn run_coverage(a: &CoverageArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    let report = mollify_core::coverage_report(&a.path, &a.coverage_file);
    let errors = report.summary.errors;
    match a.format {
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&Report::Coverage(report)).unwrap()
        ),
        Format::Sarif => println!(
            "{}",
            serde_json::to_string_pretty(&mollify_core::sarif::to_sarif(
                &report.findings,
                env!("CARGO_PKG_VERSION")
            ))
            .unwrap()
        ),
        Format::Github => print!("{}", github_annotations(&report.findings)),
        Format::Junit => print!("{}", junit_xml(&report.findings, "coverage")),
        Format::Human => {
            println!("Mollify coverage — {}", a.path);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    exit_code(errors)
}

fn run_supply_chain(a: &SupplyChainArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    let db = a
        .advisory_db
        .clone()
        .unwrap_or_else(|| a.path.join(mollify_core::DEFAULT_ADVISORY_DB));

    // Live-first (the advisory feed changes constantly), local DB as fallback.
    // `--offline` forces the deterministic DB-only path.
    let report = if a.offline {
        if !db.exists() {
            eprintln!(
                "No advisory DB at {db}. Drop `--offline` to fetch live from OSV, run \
                 `python3 scripts/fetch-advisories.py {db}`, or pass --advisory-db <path>."
            );
            return 1;
        }
        eprintln!("supply-chain: offline mode, using advisory DB {db}.");
        mollify_core::supply_chain_report(&a.path, &db)
    } else {
        let pins = mollify_core::supplychain::collect_pins(&a.path);
        match osv::fetch_for_pins(&pins) {
            Ok(advisories) => {
                eprintln!(
                    "supply-chain: live OSV data for {} pinned package(s).",
                    pins.len()
                );
                if a.refresh {
                    match osv::write_db(&db, &advisories) {
                        Ok(()) => eprintln!(
                            "supply-chain: cached {} advisory(ies) to {db}.",
                            advisories.len()
                        ),
                        Err(e) => eprintln!("supply-chain: could not write cache {db}: {e}"),
                    }
                }
                mollify_core::supply_chain_report_with(&a.path, &advisories)
            }
            Err(e) => {
                if db.exists() {
                    eprintln!("supply-chain: live fetch failed ({e}); falling back to DB {db}.");
                    mollify_core::supply_chain_report(&a.path, &db)
                } else {
                    eprintln!(
                        "supply-chain: live fetch failed ({e}) and no local DB at {db}. \
                         Pass --advisory-db, run scripts/fetch-advisories.py, or check connectivity."
                    );
                    mollify_core::supply_chain_report_with(&a.path, &[])
                }
            }
        }
    };
    let errors = report.summary.errors;
    match a.format {
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&Report::Security(report)).unwrap()
        ),
        Format::Sarif => println!(
            "{}",
            serde_json::to_string_pretty(&mollify_core::sarif::to_sarif(
                &report.findings,
                env!("CARGO_PKG_VERSION")
            ))
            .unwrap()
        ),
        Format::Github => print!("{}", github_annotations(&report.findings)),
        Format::Junit => print!("{}", junit_xml(&report.findings, "supply-chain")),
        Format::Human => {
            println!("Mollify supply-chain — {} (db: {db})", a.path);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    exit_code(errors)
}

fn run_fix(a: &FixArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    let edits = mollify_core::fix::plan(&a.path);
    if edits.is_empty() {
        println!("No auto-fixable findings (only `certain` unused symbols are auto-fixed). ✓");
        return 0;
    }
    println!(
        "{} safe fix(es){}:",
        edits.len(),
        if a.apply {
            ""
        } else {
            " (dry-run — pass --apply to write)"
        }
    );
    for e in &edits {
        println!(
            "  {}:{}-{}  {}",
            e.path, e.start_line, e.end_line, e.description
        );
    }
    if !a.apply {
        return 0;
    }
    let outcome = mollify_core::fix::apply(&edits);
    if outcome.applied > 0 {
        println!(
            "Applied {} fix(es). Re-run `mollify audit` to confirm.",
            outcome.applied
        );
    }
    if !outcome.failures.is_empty() {
        eprintln!(
            "error: {} file(s) failed{}:",
            outcome.failures.len(),
            if outcome.applied > 0 {
                " (the applied fixes above are already on disk)"
            } else {
                ""
            }
        );
        for f in &outcome.failures {
            eprintln!("  {f}");
        }
        return 1;
    }
    0
}

fn run_explain(a: &ExplainArgs) -> i32 {
    match &a.rule {
        Some(rule) => match mollify_core::explain::text(rule) {
            Some(t) => {
                println!("{rule}\n  {t}");
                0
            }
            None => {
                eprintln!("Unknown rule `{rule}`. Run `mollify explain` to list all rules.");
                1
            }
        },
        None => {
            println!("Mollify rules (run `mollify explain <rule>` for details):");
            for r in mollify_core::explain::RULES {
                println!("  {r}");
            }
            0
        }
    }
}

fn run_trace(a: &TraceArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    if let Some(code) = require_human_or_json("trace", a.format) {
        return code;
    }
    let graph = mollify_core::build_graph(&a.path);
    let Some(t) = mollify_core::trace::module(&graph, &a.module) else {
        eprintln!("No module matching `{}` found under {}.", a.module, a.path);
        return 1;
    };
    match a.format {
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "kind": "trace",
                "target": t.target,
                "imports": t.imports,
                "imported_by": t.imported_by,
            }))
            .unwrap()
        ),
        _ => {
            println!("Trace — {}", t.target);
            println!("  imports ({}):", t.imports.len());
            for m in &t.imports {
                println!("    → {m}");
            }
            println!("  imported by ({}):", t.imported_by.len());
            for m in &t.imported_by {
                println!("    ← {m}");
            }
        }
    }
    0
}

/// A cheap, deterministic signature of the project's Python files: the sorted
/// (path, mtime, len) triples. Any add/remove/edit changes it.
fn watch_signature(root: &camino::Utf8Path) -> Vec<(String, u64, u64)> {
    let mut sig: Vec<(String, u64, u64)> = mollify_core::build_graph(root)
        .modules
        .iter()
        .map(|m| {
            let meta = std::fs::metadata(&m.path).ok();
            let mtime = meta
                .as_ref()
                .and_then(|x| x.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let len = meta.as_ref().map(|x| x.len()).unwrap_or(0);
            (m.path.to_string(), mtime, len)
        })
        .collect();
    sig.sort();
    sig
}

fn run_watch(a: &WatchArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    let scope = Scope {
        path: a.path.clone(),
        format: Format::Human,
        gate: Gate::All,
        base: None,
        save_baseline: None,
        baseline: None,
        fail_on_regression: false,
        brief: false,
        min_confidence: None,
        include: Vec::new(),
    };
    println!(
        "Watching {} (every {}ms) — Ctrl-C to stop.\n",
        a.path, a.interval_ms
    );
    let mut last: Option<Vec<(String, u64, u64)>> = None;
    loop {
        let sig = watch_signature(&a.path);
        if last.as_ref() != Some(&sig) {
            println!("── re-running audit ──");
            run_audit(&scope);
            println!();
            last = Some(sig);
        }
        std::thread::sleep(std::time::Duration::from_millis(a.interval_ms));
    }
}

fn run_inspect(a: &InspectArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    if let Some(code) = require_human_or_json("inspect", a.format) {
        return code;
    }
    let ins = mollify_core::inspect(&a.path, &a.file);
    // No module and no findings means nothing in the project matched the
    // argument — an error, like `trace` on an unknown module.
    if ins.module.is_none() && ins.findings.is_empty() {
        eprintln!("No file matching `{}` found under {}.", a.file, a.path);
        return 1;
    }
    match a.format {
        Format::Json => {
            let body = serde_json::json!({
                "kind": "inspect",
                "file": ins.file,
                "module": ins.module,
                "findings": ins.findings,
                "imports": ins.imports,
                "imported_by": ins.imported_by,
            });
            println!("{}", serde_json::to_string_pretty(&body).unwrap());
        }
        _ => {
            println!("Mollify inspect — {}", ins.file);
            if let Some(m) = &ins.module {
                println!("module: {m}");
            }
            println!(
                "imports {} module(s); imported by {} module(s)",
                ins.imports.len(),
                ins.imported_by.len()
            );
            println!("{} finding(s):", ins.findings.len());
            let refs: Vec<&Finding> = ins.findings.iter().collect();
            print_findings_refs(&refs);
        }
    }
    0
}

fn run_list(a: &ListArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    if let Some(code) = require_human_or_json("list", a.format) {
        return code;
    }
    let label = match a.kind {
        ListKind::EntryPoints => "entry-points",
        ListKind::Files => "files",
        ListKind::Frameworks => "frameworks",
    };
    let rows = mollify_core::list_topology(&a.path, label);
    match a.format {
        Format::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({ "kind": "list", "of": label, "items": rows })
                )
                .unwrap()
            );
        }
        _ => {
            println!("Mollify list:{label} — {} item(s)", rows.len());
            for r in &rows {
                println!("  {}", r.replace('\t', "  "));
            }
        }
    }
    0
}

fn run_metrics(a: &MetricsArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    if let Some(code) = require_human_or_json("metrics", a.format) {
        return code;
    }
    let report = mollify_core::metrics::report(&a.path);
    match a.format {
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&Report::Metrics(report)).unwrap()
        ),
        _ => {
            println!("Mollify metrics — {}", a.path);
            println!(
                "{} file(s), {} LOC ({} SLOC), {} function(s); mean MI {:.1}",
                report.totals.files,
                report.totals.loc,
                report.totals.sloc,
                report.totals.functions,
                report.totals.mean_maintainability_index
            );
            for f in &report.files {
                println!(
                    "  [{}] MI {:>5.1}  cc(max {:>2}, sum {:>3})  {} sloc  {}",
                    f.mi_rank,
                    f.maintainability_index,
                    f.max_cyclomatic,
                    f.total_cyclomatic,
                    f.sloc,
                    f.path
                );
            }
        }
    }
    0
}

/// A documented starter `.mollifyrc.json`. Severities default to `warn` for the
/// core analysis areas; `_comment` keys are ignored by the loader and serve as
/// inline docs (JSON has no comments). Complexity thresholds are the engine
/// defaults, surfaced here as obvious knobs. The audit score weights findings by
/// confidence — `uncertain` candidates count least — so a first run is not
/// dominated by low-confidence noise.
const STARTER_RC: &str = r#"{
  "_comment": "mollify config — see docs/configuration.md. Severities: error | warn | off.",
  "severity": {
    "_comment": "Per-rule ids win over category names (dead-code, dependency-hygiene, complexity, security, architecture, type-health).",
    "dead-code": "warn",
    "dependency-hygiene": "warn",
    "type-health": "off"
  },
  "ignore": [],
  "exclude_dirs": [],
  "max_cyclomatic": 10,
  "max_cognitive": 15
}
"#;

fn run_init(a: &InitArgs) -> i32 {
    if let Some(code) = validate_root(&a.path) {
        return code;
    }
    // Agent-integration mode: install skills/rules/hooks/commands/workflows.
    if a.all || !a.agent.is_empty() {
        return run_init_agents(a);
    }
    let cfg = a.path.join(".mollifyrc.json");
    if cfg.exists() {
        println!("{cfg} already exists; leaving it untouched.");
        return 0;
    }
    match std::fs::write(&cfg, STARTER_RC) {
        Ok(()) => {
            println!("Wrote {cfg}");
            0
        }
        Err(e) => {
            eprintln!("error: could not write {cfg}: {e}");
            1
        }
    }
}

/// Resolve the requested agents and scaffold their integration artifacts.
fn run_init_agents(a: &InitArgs) -> i32 {
    use mollify_core::agents::{self, Agent, FileOutcome};
    let agents: Vec<Agent> = if a.all {
        Agent::ALL.to_vec()
    } else {
        let mut resolved = Vec::new();
        for name in &a.agent {
            match Agent::parse(name) {
                Some(ag) => resolved.push(ag),
                None => {
                    eprintln!(
                        "Unknown agent `{name}`. Valid: claude, cursor, gemini, codex, cascade (or --all)."
                    );
                    return 1;
                }
            }
        }
        resolved
    };
    let mut created = 0usize;
    let mut overwritten = 0usize;
    let mut skipped = 0usize;
    for ag in agents {
        match agents::install(&a.path, ag, a.force) {
            Ok(files) => {
                println!("{} — {} file(s):", ag.name(), files.len());
                for f in &files {
                    let tag = match f.outcome {
                        FileOutcome::Created => {
                            created += 1;
                            "create"
                        }
                        FileOutcome::Overwritten => {
                            overwritten += 1;
                            "overwrite"
                        }
                        FileOutcome::Skipped => {
                            skipped += 1;
                            "skip"
                        }
                    };
                    println!("  [{tag}] {}", f.path);
                }
            }
            Err(e) => {
                eprintln!("error: installing {} integration: {e}", ag.name());
                return 1;
            }
        }
    }
    println!("\nDone: {created} created, {overwritten} overwritten, {skipped} skipped.");
    if skipped > 0 && !a.force {
        println!("Re-run with --force to overwrite the skipped files.");
    }
    0
}

fn print_summary(s: &Summary) {
    println!(
        "{} finding(s) across {} file(s) — {} error, {} warn{}",
        s.total,
        s.files_analyzed,
        s.errors,
        s.warnings,
        if s.introduced > 0 {
            format!(", {} introduced", s.introduced)
        } else {
            String::new()
        }
    );
}

fn print_findings(findings: &[Finding]) {
    let refs: Vec<&Finding> = findings.iter().collect();
    print_findings_refs(&refs);
}

fn print_findings_refs(findings: &[&Finding]) {
    if findings.is_empty() {
        println!("  No findings. ✓");
        return;
    }
    for f in findings {
        let sev = match f.severity {
            Severity::Error => "error",
            Severity::Warn => "warn",
            // `Off` and any future severity (#[non_exhaustive]).
            _ => "off",
        };
        let conf = match f.confidence {
            Confidence::Certain => "certain",
            Confidence::Likely => "likely",
            // `Uncertain` and any future tier (#[non_exhaustive]).
            _ => "uncertain",
        };
        let loc = &f.location;
        println!(
            "  {}:{} [{sev}/{conf}] {} — {}  ({})",
            loc.path, loc.line, f.rule, f.reason, f.fingerprint
        );
    }
}

/// Exit non-zero only when there are `error`-severity findings (CI gate).
fn exit_code(errors: usize) -> i32 {
    if errors > 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mollify_types::Severity;

    /// A throwaway project dir with one entry point and one dead module.
    fn temp_project(tag: &str) -> Utf8PathBuf {
        let base = std::env::temp_dir().join(format!("mollify-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dir = Utf8PathBuf::from_path_buf(base).unwrap();
        std::fs::write(dir.join("__main__.py"), "print('hi')\n").unwrap();
        std::fs::write(dir.join("lib.py"), "def dead():\n    return 1\n").unwrap();
        dir
    }

    fn scope(path: Utf8PathBuf) -> Scope {
        Scope {
            path,
            format: Format::Json,
            gate: Gate::All,
            base: None,
            save_baseline: None,
            baseline: None,
            fail_on_regression: false,
            brief: false,
            min_confidence: None,
            include: Vec::new(),
        }
    }

    fn sample_finding(severity: Severity) -> Finding {
        Finding {
            fingerprint: "unused-export:0000".into(),
            rule: "unused-export".into(),
            category: mollify_types::Category::DeadCode,
            severity,
            confidence: Confidence::Certain,
            attribution: None,
            reason: "test".into(),
            location: mollify_types::Location {
                path: "a.py".into(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![],
        }
    }

    #[test]
    fn nonexistent_path_is_exit_2_not_clean_report() {
        let missing = Utf8PathBuf::from("/no/such/mollify-project");
        assert_eq!(run_audit(&scope(missing.clone())), 2);
        assert_eq!(
            run_metrics(&MetricsArgs {
                path: missing.clone(),
                format: Format::Json
            }),
            2
        );
        assert_eq!(validate_root(&missing), Some(2));
    }

    #[test]
    fn validate_root_rejects_files_and_accepts_dirs() {
        let dir = temp_project("root");
        assert_eq!(validate_root(&dir), None);
        assert_eq!(
            validate_root(&dir.join("lib.py")),
            Some(2),
            "a file is not a root"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_baseline_write_failure_is_error_exit() {
        let dir = temp_project("save-fail");
        let mut s = scope(dir.clone());
        // Writing a baseline *onto a directory* must fail.
        s.save_baseline = Some(dir.clone());
        let mut findings = vec![sample_finding(Severity::Warn)];
        let outcome = handle_baseline(&s, &mut findings);
        assert!(matches!(outcome, BaselineOutcome::SaveFailed));
        assert_eq!(baseline_failure_exit(&outcome), Some(1));
        assert_eq!(run_audit(&s), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_baseline_success_emits_report_and_exits_zero() {
        let dir = temp_project("save-ok");
        let mut s = scope(dir.clone());
        s.save_baseline = Some(dir.join("baseline.json"));
        assert_eq!(run_audit(&s), 0);
        assert!(dir.join("baseline.json").exists());
        // Even with error findings, `--save-baseline` is documented exit 0.
        assert_eq!(
            gated_exit(&s, 3, &BaselineOutcome::Saved(dir.join("baseline.json"))),
            0
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fail_on_regression_with_missing_baseline_is_error_exit() {
        let dir = temp_project("regress");
        let mut s = scope(dir.clone());
        s.baseline = Some(dir.join("absent.json"));
        s.fail_on_regression = true;
        let mut findings = vec![sample_finding(Severity::Warn)];
        let outcome = handle_baseline(&s, &mut findings);
        assert!(matches!(outcome, BaselineOutcome::LoadFailed));
        assert_eq!(run_audit(&s), 1);
        // Without the CI gate the old warn-and-report behavior stands.
        s.fail_on_regression = false;
        let outcome = handle_baseline(&s, &mut findings);
        assert!(matches!(outcome, BaselineOutcome::None));
        assert_eq!(findings.len(), 1, "findings must be untouched");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rescore_audit_recomputes_score_from_filtered_findings() {
        let mut report = mollify_types::AuditReport {
            schema_version: mollify_types::SCHEMA_VERSION.into(),
            quality_score: 40,
            summary: Summary::from_findings(&[sample_finding(Severity::Error)], 1),
            findings: vec![sample_finding(Severity::Error)],
        };
        report.findings.clear(); // as --min-confidence / --baseline would
        rescore_audit(&mut report);
        assert_eq!(
            report.quality_score, 100,
            "score must track emitted findings"
        );
        assert_eq!(report.summary.total, 0);
    }

    #[test]
    fn unsupported_formats_are_usage_errors() {
        let dir = temp_project("format");
        for format in [Format::Sarif, Format::Github, Format::Junit] {
            assert_eq!(
                run_metrics(&MetricsArgs {
                    path: dir.clone(),
                    format
                }),
                2
            );
        }
        assert_eq!(require_human_or_json("trace", Format::Json), None);
        assert_eq!(require_human_or_json("trace", Format::Human), None);
        assert_eq!(require_human_or_json("trace", Format::Sarif), Some(2));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inspect_unknown_file_is_error_exit() {
        let dir = temp_project("inspect");
        let miss = InspectArgs {
            file: "zzz_no_such_file.py".into(),
            path: dir.clone(),
            format: Format::Json,
        };
        assert_eq!(run_inspect(&miss), 1);
        let hit = InspectArgs {
            file: "lib.py".into(),
            path: dir.clone(),
            format: Format::Json,
        };
        assert_eq!(run_inspect(&hit), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn starter_rc_is_valid_and_loads() {
        // The scaffolded rc must parse cleanly through the real loader
        // (including the `_comment` doc keys) and apply its overrides.
        let base = std::env::temp_dir().join(format!("mollify-cli-rc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dir = Utf8PathBuf::from_path_buf(base).unwrap();
        std::fs::write(dir.join(".mollifyrc.json"), STARTER_RC).unwrap();
        let cfg = mollify_core::config::load(&dir);
        assert_eq!(cfg.max_cyclomatic, 10);
        assert_eq!(cfg.max_cognitive, 15);
        assert_eq!(
            cfg.severity.get("type-health"),
            Some(&Severity::Off),
            "starter rc should silence type-health by default"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
