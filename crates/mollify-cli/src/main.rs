//! The `mollify` command-line interface.
//!
//! Analysis: `audit`, `dead-code`, `deps`, `arch`, `complexity`, `dupes`,
//! `types`, `security`, `coverage`, `supply-chain`. Workflow: `fix`, `explain`,
//! `trace`, `inspect`, `list`, `watch`, `init`, `mcp`. Analysis commands support
//! `--format human|json|sarif`, `--path`, `--gate`, and regression baselines
//! (`--save-baseline`/`--baseline`/`--fail-on-regression`/`--brief`). JSON is the
//! kind-discriminated contract from `mollify-types`.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use mollify_types::{Confidence, Finding, Report, Severity, Summary};

mod osv;

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
    /// Scaffold a .mollifyrc and report detected layout.
    Init(Scope),
    /// Run the Model Context Protocol server over stdio (for coding agents).
    Mcp,
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
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    Human,
    Json,
    Sarif,
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
    for f in findings.iter_mut() {
        let introduced =
            mollify_core::git::path_is_changed(&scope.path, &f.location.path, &changed);
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
            mollify_core::dead_code_report,
            Report::DeadCode,
            "dead-code",
        ),
        Command::Deps(s) => run_findings(&s, mollify_core::deps_report, Report::Deps, "deps"),
        Command::Arch(s) => run_findings(&s, mollify_core::arch_report, Report::Arch, "arch"),
        Command::Complexity(s) => run_findings(
            &s,
            mollify_core::complexity_report,
            Report::Complexity,
            "complexity",
        ),
        Command::Dupes(s) => run_findings(&s, mollify_core::dupes_report, Report::Dupes, "dupes"),
        Command::Types(s) => run_findings(&s, mollify_core::types_report, Report::Types, "types"),
        Command::Security(s) => run_findings(
            &s,
            mollify_core::security_report,
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
        Command::Init(s) => run_init(&s),
        Command::Mcp => match mollify_mcp::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("mollify mcp: {e}");
                1
            }
        },
    };
    std::process::exit(code);
}

/// Outcome of applying baseline options to a findings set.
enum BaselineOutcome {
    /// `--save-baseline` wrote a snapshot; the caller should print + exit 0.
    Saved(Utf8PathBuf),
    /// `--baseline` filtered to findings new since the snapshot; `usize` is how many.
    Filtered(usize),
    /// No baseline options in effect.
    None,
}

/// Apply `--save-baseline` / `--baseline` to `findings`. With `--baseline`,
/// retains only findings that are new relative to the snapshot.
fn handle_baseline(s: &Scope, findings: &mut Vec<mollify_types::Finding>) -> BaselineOutcome {
    use mollify_core::baseline::{split_new, Baseline};
    if let Some(path) = &s.save_baseline {
        let b = Baseline::from_findings(findings);
        if let Err(e) = b.save(path) {
            eprintln!("error: could not write baseline {path}: {e}");
        }
        return BaselineOutcome::Saved(path.clone());
    }
    if let Some(path) = &s.baseline {
        let Some(b) = Baseline::load(path) else {
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
    if s.fail_on_regression {
        if let BaselineOutcome::Filtered(n) = outcome {
            if *n > 0 {
                return 1;
            }
        }
    }
    exit_code(errors)
}

fn run_audit(s: &Scope) -> i32 {
    let mut report = mollify_core::audit_report(&s.path);
    apply_gate(s, &mut report.findings);
    let outcome = handle_baseline(s, &mut report.findings);
    if let BaselineOutcome::Saved(p) = &outcome {
        println!(
            "Wrote baseline with {} fingerprint(s) to {p}",
            report.findings.len()
        );
        return 0;
    }
    report.summary =
        mollify_types::Summary::from_findings(&report.findings, report.summary.files_analyzed);
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
        Format::Human => {
            println!("Mollify audit — {}", s.path);
            println!("Quality score: {}/100", report.quality_score);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    gated_exit(s, errors, &outcome)
}

fn run_findings(
    s: &Scope,
    f: fn(&camino::Utf8Path) -> mollify_types::FindingsReport,
    wrap: fn(mollify_types::FindingsReport) -> Report,
    label: &str,
) -> i32 {
    let mut report = f(&s.path);
    apply_gate(s, &mut report.findings);
    let outcome = handle_baseline(s, &mut report.findings);
    if let BaselineOutcome::Saved(p) = &outcome {
        println!(
            "Wrote baseline with {} fingerprint(s) to {p}",
            report.findings.len()
        );
        return 0;
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
        Format::Human => {
            println!("Mollify {label} — {}", s.path);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    gated_exit(s, errors, &outcome)
}

fn run_coverage(a: &CoverageArgs) -> i32 {
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
        Format::Human => {
            println!("Mollify coverage — {}", a.path);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    exit_code(errors)
}

fn run_supply_chain(a: &SupplyChainArgs) -> i32 {
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
        Format::Human => {
            println!("Mollify supply-chain — {} (db: {db})", a.path);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    exit_code(errors)
}

fn run_fix(a: &FixArgs) -> i32 {
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
    match mollify_core::fix::apply(&edits) {
        Ok(n) => {
            println!("Applied {n} fix(es). Re-run `mollify audit` to confirm.");
            0
        }
        Err(e) => {
            eprintln!("error: applying fixes: {e}");
            1
        }
    }
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
    let scope = Scope {
        path: a.path.clone(),
        format: Format::Human,
        gate: Gate::All,
        base: None,
        save_baseline: None,
        baseline: None,
        fail_on_regression: false,
        brief: false,
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
    // All findings for the project, filtered to the requested file.
    let report = mollify_core::audit_report(&a.path);
    let matches: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|f| {
            let p = f.location.path.as_str();
            p == a.file || p.ends_with(&a.file) || p.ends_with(&format!("/{}", a.file))
        })
        .collect();
    // Import neighborhood (best-effort): match the file's module by path stem.
    let graph = mollify_core::build_graph(&a.path);
    let module = graph
        .modules
        .iter()
        .find(|m| {
            let p = m.path.as_str();
            p == a.file || p.ends_with(&a.file) || p.ends_with(&format!("/{}", a.file))
        })
        .map(|m| m.dotted.clone());
    let trace = module
        .as_deref()
        .and_then(|d| mollify_core::trace::module(&graph, d));

    match a.format {
        Format::Json => {
            let body = serde_json::json!({
                "kind": "inspect",
                "file": a.file,
                "module": module,
                "findings": matches,
                "imports": trace.as_ref().map(|t| &t.imports),
                "imported_by": trace.as_ref().map(|t| &t.imported_by),
            });
            println!("{}", serde_json::to_string_pretty(&body).unwrap());
        }
        _ => {
            println!("Mollify inspect — {}", a.file);
            if let Some(m) = &module {
                println!("module: {m}");
            }
            if let Some(t) = &trace {
                println!(
                    "imports {} module(s); imported by {} module(s)",
                    t.imports.len(),
                    t.imported_by.len()
                );
            }
            println!("{} finding(s):", matches.len());
            print_findings_refs(&matches);
        }
    }
    0
}

fn run_list(a: &ListArgs) -> i32 {
    let graph = mollify_core::build_graph(&a.path);
    let mut rows: Vec<String> = match a.kind {
        ListKind::EntryPoints => graph
            .modules
            .iter()
            .filter(|m| m.is_entry)
            .map(|m| format!("{}  ({})", m.dotted, m.path))
            .collect(),
        ListKind::Files => graph
            .modules
            .iter()
            .map(|m| format!("{}  ({})", m.dotted, m.path))
            .collect(),
        ListKind::Frameworks => {
            let mut fw: std::collections::BTreeSet<String> = Default::default();
            for m in &graph.modules {
                for d in &m.parsed.definitions {
                    if mollify_core::plugins::is_framework_entry(d) {
                        for dec in &d.decorators {
                            fw.insert(dec.split('.').next().unwrap_or(dec).to_string());
                        }
                    }
                }
            }
            fw.into_iter().collect()
        }
    };
    rows.sort();
    let label = match a.kind {
        ListKind::EntryPoints => "entry-points",
        ListKind::Files => "files",
        ListKind::Frameworks => "frameworks",
    };
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
                println!("  {r}");
            }
        }
    }
    0
}

fn run_init(s: &Scope) -> i32 {
    let cfg = s.path.join(".mollifyrc.json");
    if cfg.exists() {
        println!("{cfg} already exists; leaving it untouched.");
        return 0;
    }
    let default = "{\n  \"source_roots\": [\".\", \"src\"],\n  \"severity\": { \"dead-code\": \"warn\", \"dependency-hygiene\": \"warn\" }\n}\n";
    match std::fs::write(&cfg, default) {
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
            Severity::Off => "off",
        };
        let conf = match f.confidence {
            Confidence::Certain => "certain",
            Confidence::Likely => "likely",
            Confidence::Uncertain => "uncertain",
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
