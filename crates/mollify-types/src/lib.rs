//! # mollify-types
//!
//! The shared, versioned **data contract** for Mollify. Every command emits a
//! JSON envelope with a discriminating top-level `kind`; downstream agents and
//! CI depend on this JSON shape, not on Mollify's internal Rust types.
//!
//! Invariants (ported from fallow's design — see `RESEARCH.md` §2.11):
//! - **Determinism:** identical input → byte-identical output. All collections
//!   that reach output are sorted deterministically before serialization.
//! - **Evidence, not decisions:** every [`Finding`] carries a stable
//!   [`Finding::fingerprint`], a [`Confidence`] tier, and a human `reason`.
//! - **Candidate/verifier separation:** [`Action`]s are *proposed*; only
//!   `auto_fixable` + `Confidence::Certain` may be applied without a human.

use serde::{Deserialize, Serialize};

/// Current schema version of the JSON contract. Bump the minor on additive
/// changes, the major on breaking ones. Agent skills pin to this.
pub const SCHEMA_VERSION: &str = "0.1";

/// Confidence tier attached to every finding. This is the core honesty
/// mechanism: Python dead-code detection is undecidable in general, so Mollify
/// never claims boolean certainty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Syntactically provable (e.g. code after `return`, unused local with no
    /// dynamic sink in scope). Safe to auto-fix.
    Certain,
    /// Strong static signal but a residual dynamic risk. Suggest, don't apply.
    Likely,
    /// Public surface, near `getattr`/`eval`, or framework-adjacent. Report only.
    Uncertain,
}

/// Severity controls CI exit behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Fails CI (non-zero exit) by default.
    Error,
    /// Reported, exit 0.
    Warn,
    /// Suppressed.
    Off,
}

/// Whether a finding was introduced by the current change or inherited from the
/// base. The PR gate (`--gate new-only`) keys on [`Attribution::Introduced`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    Introduced,
    Inherited,
}

/// The five co-equal analysis areas (plus dependency hygiene), mirroring
/// fallow's "never reduce it to a dead-code tool" rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    DeadCode,
    Duplication,
    CircularDependency,
    Complexity,
    Architecture,
    DependencyHygiene,
    /// Type-annotation health (Python-specific; no fallow analog).
    TypeHealth,
    /// Security candidates (syntactic; never confirmed vulnerabilities).
    Security,
}

/// A source location, 1-based line/column, workspace-relative path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    pub path: camino::Utf8PathBuf,
    pub line: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub column: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A proposed, machine-actionable remediation for a finding. The agent decides
/// whether to apply it; Mollify never auto-applies non-`Certain` findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// e.g. `remove-symbol`, `remove-import`, `remove-dependency`.
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    /// True only when Mollify can apply this deterministically and safely.
    pub auto_fixable: bool,
    /// The inline comment that would suppress this finding instead of fixing it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression_comment: Option<String>,
}

/// A single piece of deterministic evidence. The atom of every report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable cross-run id, `<rule>:<hex>` — survives reordering and minor edits
    /// so it can be referenced in commits and baselines.
    pub fingerprint: String,
    /// Machine rule id, e.g. `unused-export`, `unused-dependency`, `cycle`.
    pub rule: String,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
    /// Human-readable explanation — the "why" of the evidence.
    pub reason: String,
    pub location: Location,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
}

/// The kind-discriminated output envelope. `kind` lets clients switch on the
/// result type and iterate `findings`.
// `Eq` is intentionally omitted: `MetricsReport` carries `f64` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Report {
    /// Full unified report across all analysis areas.
    Audit(AuditReport),
    /// Dead-code-only report.
    DeadCode(FindingsReport),
    /// Dependency-hygiene-only report.
    Deps(FindingsReport),
    /// Architecture (circular dependencies, boundaries).
    Arch(FindingsReport),
    /// Complexity hotspots.
    Complexity(FindingsReport),
    /// Duplication / clone families.
    Dupes(FindingsReport),
    /// Type-annotation health.
    Types(FindingsReport),
    /// Security candidates.
    Security(FindingsReport),
    /// Runtime-coverage cold-path analysis.
    Coverage(FindingsReport),
    /// Code-metrics report (Maintainability Index, Halstead, raw LOC).
    Metrics(MetricsReport),
}

/// Per-file code metrics (radon/wily-style), plus project totals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsReport {
    pub schema_version: String,
    pub files: Vec<FileMetrics>,
    pub totals: MetricsTotals,
}

/// Maintainability and size metrics for one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMetrics {
    pub path: camino::Utf8PathBuf,
    /// Physical lines of code.
    pub loc: u32,
    /// Source lines (non-blank, non-comment).
    pub sloc: u32,
    pub comment_lines: u32,
    pub blank_lines: u32,
    pub functions: u32,
    /// Sum of per-function cyclomatic complexity.
    pub total_cyclomatic: u32,
    pub max_cyclomatic: u32,
    /// Maintainability Index, normalized to 0–100 (higher is better).
    pub maintainability_index: f64,
    /// MI rank: `A` (20–100), `B` (10–<20), `C` (<10) — radon's mapping.
    pub mi_rank: char,
}

/// Project-wide metric totals.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MetricsTotals {
    pub files: usize,
    pub loc: u32,
    pub sloc: u32,
    pub functions: u32,
    /// Mean Maintainability Index across files.
    pub mean_maintainability_index: f64,
}

/// A report that is just a sorted list of findings plus a summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingsReport {
    pub schema_version: String,
    pub summary: Summary,
    pub findings: Vec<Finding>,
}

/// The full audit envelope: a quality score plus the findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditReport {
    pub schema_version: String,
    /// 0–100 health score (higher is better).
    pub quality_score: u8,
    pub summary: Summary,
    pub findings: Vec<Finding>,
}

/// Aggregate counts, always present so CI can gate without scanning findings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
    pub files_analyzed: usize,
    #[serde(default, skip_serializing_if = "is_usize_zero")]
    pub introduced: usize,
}

fn is_usize_zero(n: &usize) -> bool {
    *n == 0
}

impl Summary {
    /// Build a summary from a finding slice (counts errors/warnings/introduced).
    pub fn from_findings(findings: &[Finding], files_analyzed: usize) -> Self {
        let mut s = Summary {
            total: findings.len(),
            files_analyzed,
            ..Default::default()
        };
        for f in findings {
            match f.severity {
                Severity::Error => s.errors += 1,
                Severity::Warn => s.warnings += 1,
                Severity::Off => {}
            }
            if f.attribution == Some(Attribution::Introduced) {
                s.introduced += 1;
            }
        }
        s
    }
}

/// Deterministic ordering for findings: by path, then line, then rule, then
/// fingerprint. Call before serializing any report.
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.location.line.cmp(&b.location.line))
            .then(a.rule.cmp(&b.rule))
            .then(a.fingerprint.cmp(&b.fingerprint))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_finding(path: &str, line: u32, rule: &str) -> Finding {
        Finding {
            fingerprint: format!("{rule}:0000"),
            rule: rule.to_string(),
            category: Category::DeadCode,
            severity: Severity::Error,
            confidence: Confidence::Certain,
            attribution: None,
            reason: "test".into(),
            location: Location {
                path: path.into(),
                line,
                column: 0,
                end_line: None,
            },
            actions: vec![],
        }
    }

    #[test]
    fn envelope_has_kind_discriminator() {
        let report = Report::DeadCode(FindingsReport {
            schema_version: SCHEMA_VERSION.into(),
            summary: Summary::default(),
            findings: vec![],
        });
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"kind\":\"dead-code\""));
    }

    #[test]
    fn confidence_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&Confidence::Uncertain).unwrap(),
            "\"uncertain\""
        );
    }

    #[test]
    fn sort_is_deterministic() {
        let mut a = vec![
            sample_finding("b.py", 1, "x"),
            sample_finding("a.py", 9, "x"),
            sample_finding("a.py", 2, "y"),
        ];
        sort_findings(&mut a);
        assert_eq!(a[0].location.path, "a.py");
        assert_eq!(a[0].location.line, 2);
        assert_eq!(a[2].location.path, "b.py");
    }

    #[test]
    fn summary_counts_severities() {
        let mut f = sample_finding("a.py", 1, "x");
        f.severity = Severity::Warn;
        let s = Summary::from_findings(&[sample_finding("a.py", 1, "x"), f], 1);
        assert_eq!(s.total, 2);
        assert_eq!(s.errors, 1);
        assert_eq!(s.warnings, 1);
    }
}
