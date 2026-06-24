//! The `mollify` command-line interface.
//!
//! Mirrors fallow's surface incrementally. Implemented today: `audit`,
//! `dead-code`, `deps`, `init`, `version`. Each supports `--format human|json`
//! and `--path`. JSON is the kind-discriminated contract from `mollify-types`.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use mollify_types::{Confidence, Finding, Report, Severity, Summary};

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
    /// Apply safe auto-fixes (certain, auto-fixable unused symbols). Dry-run unless --apply.
    Fix(FixArgs),
    /// Scaffold a .mollifyrc and report detected layout.
    Init(Scope),
    /// Run the Model Context Protocol server over stdio (for coding agents).
    Mcp,
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
        Command::Fix(a) => run_fix(&a),
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

fn run_audit(s: &Scope) -> i32 {
    let mut report = mollify_core::audit_report(&s.path);
    apply_gate(s, &mut report.findings);
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
    exit_code(errors)
}

fn run_findings(
    s: &Scope,
    f: fn(&camino::Utf8Path) -> mollify_types::FindingsReport,
    wrap: fn(mollify_types::FindingsReport) -> Report,
    label: &str,
) -> i32 {
    let mut report = f(&s.path);
    apply_gate(s, &mut report.findings);
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
