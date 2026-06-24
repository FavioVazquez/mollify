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
    /// Full unified report (dead-code + dependency hygiene today).
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
    /// Scaffold a .mollifyrc and report detected layout.
    Init(Scope),
    /// Run the Model Context Protocol server over stdio (for coding agents).
    Mcp,
}

#[derive(clap::Args)]
struct Scope {
    /// Project root to analyze.
    #[arg(long, default_value = ".")]
    path: Utf8PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    Human,
    Json,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Audit(s) => run_audit(&s),
        Command::DeadCode(s) => {
            run_findings(&s, mollify_core::dead_code_report, Report::DeadCode, "dead-code")
        }
        Command::Deps(s) => run_findings(&s, mollify_core::deps_report, Report::Deps, "deps"),
        Command::Arch(s) => run_findings(&s, mollify_core::arch_report, Report::Arch, "arch"),
        Command::Complexity(s) => {
            run_findings(&s, mollify_core::complexity_report, Report::Complexity, "complexity")
        }
        Command::Dupes(s) => run_findings(&s, mollify_core::dupes_report, Report::Dupes, "dupes"),
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
    let report = mollify_core::audit_report(&s.path);
    match s.format {
        Format::Json => {
            let env = Report::Audit(report.clone());
            println!("{}", serde_json::to_string_pretty(&env).unwrap());
        }
        Format::Human => {
            println!("Mollify audit — {}", s.path);
            println!("Quality score: {}/100", report.quality_score);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    exit_code(report.summary.errors)
}

fn run_findings(
    s: &Scope,
    f: fn(&camino::Utf8Path) -> mollify_types::FindingsReport,
    wrap: fn(mollify_types::FindingsReport) -> Report,
    label: &str,
) -> i32 {
    let report = f(&s.path);
    let errors = report.summary.errors;
    match s.format {
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(&wrap(report)).unwrap());
        }
        Format::Human => {
            println!("Mollify {label} — {}", s.path);
            print_summary(&report.summary);
            print_findings(&report.findings);
        }
    }
    exit_code(errors)
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
