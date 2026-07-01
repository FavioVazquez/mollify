//! Commented-out-code detection (eradicate / flake8-eradicate E800). Flags
//! comment lines whose stripped text parses as Python code (`import`, `def`,
//! `return`, assignments, control flow) rather than prose. Tool directives
//! (`noqa`, `type:`, `mypy:`, `TODO`, `mollify:`, shebangs) are never flagged.
//! Orthogonal to reachability — it's about dead *text*, not dead symbols.

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

/// Directive prefixes that are legitimate comments, never commented-out code.
const DIRECTIVES: &[&str] = &[
    "noqa", "type:", "mypy", "pylint", "pyright", "ruff", "flake8", "isort", "todo", "fixme",
    "xxx", "hack", "note", "mollify", "nosec", "pragma", "!",
];

/// Heuristic: does a comment's stripped body look like Python code?
fn looks_like_code(body: &str) -> bool {
    let b = body.trim();
    if b.len() < 3 {
        return false;
    }
    let lower = b.to_ascii_lowercase();
    if DIRECTIVES.iter().any(|d| lower.starts_with(d)) {
        return false;
    }
    // Prose guards, applied even to keyword-opening lines: a trailing period is
    // a sentence, and a long wordy line is explanatory English. Words like
    // "from"/"for"/"with"/"if" open both code and prose, so a bare keyword match
    // is not enough (e.g. "from zero (proportion of draws ...), doubled.").
    if b.ends_with('.') || b.split_whitespace().count() > 12 {
        return false;
    }
    // Statement keywords at the start.
    let starters = [
        "import ", "from ", "def ", "class ", "return", "if ", "elif ", "else:", "for ", "while ",
        "try:", "except", "finally:", "with ", "raise ", "assert ", "print(", "del ", "yield ",
        "async ", "await ", "lambda ",
    ];
    if starters.iter().any(|s| b.starts_with(s)) {
        // `from X import Y` is code; "from a distance ..." is not.
        if b.starts_with("from ") {
            return b.contains(" import ");
        }
        return true;
    }
    // `name = value` / `name(...)` / `obj.method(...)` shaped lines (with a
    // trailing colon or paren/operator), excluding prose-like sentences.
    (b.contains(" = ") || b.contains("=="))
        || (b.ends_with(':') && !b.contains(' '))
        || (b.ends_with(')') && b.contains('('))
        || b.ends_with('\\')
}

/// Emit a `commented-code` finding per comment line that looks like code.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in &graph.modules {
        if let Some(src) = mollify_graph::read_source(&m.path) {
            findings.extend(analyze_source_ids(&m.path, &m.rel, &src));
        }
    }
    findings
}

/// Commented-code findings from a file's source text (also the live LSP path,
/// where the display path doubles as the fingerprint identity).
pub fn analyze_source(path: &camino::Utf8Path, src: &str) -> Vec<Finding> {
    analyze_source_ids(path, path, src)
}

/// `path` is what findings display; `rel` (root-relative) is the stable
/// fingerprint identity. The comment's own content anchors the fingerprint,
/// so unrelated edits above it don't churn baselines.
fn analyze_source_ids(path: &camino::Utf8Path, rel: &camino::Utf8Path, src: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut occ = crate::fingerprint::Occurrences::default();
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(body) = trimmed.strip_prefix('#') else {
            continue;
        };
        if !looks_like_code(body) {
            continue;
        }
        let rule = "commented-code";
        let line_no = i as u32 + 1;
        let content = body.trim();
        findings.push(Finding {
            fingerprint: fingerprint(rule, &[rel.as_str(), content, &occ.next(content)]),
            rule: rule.into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence: Confidence::Likely,
            attribution: None,
            reason: format!("commented-out code: `{}`", body.trim()),
            location: Location {
                path: path.to_owned(),
                line: line_no,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "remove-commented-code".into(),
                description: "Delete the commented-out code (version control remembers it)".into(),
                auto_fixable: false,
                suppression_comment: Some("# mollify: ignore[commented-code]".into()),
            }],
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_code_not_prose_or_directives() {
        assert!(looks_like_code(" import os"));
        assert!(looks_like_code(" return x + 1"));
        assert!(looks_like_code(" x = compute()"));
        assert!(looks_like_code(" def helper():"));
        assert!(!looks_like_code(" this explains why we do the thing."));
        assert!(!looks_like_code(" noqa: F401"));
        assert!(!looks_like_code(" type: ignore"));
        assert!(!looks_like_code(" TODO: fix this later"));
    }

    #[test]
    fn prose_opening_with_keywords_is_not_code() {
        // Real-world false positives: English prose that happens to open
        // with a Python keyword.
        assert!(!looks_like_code(
            " from zero (proportion of draws on the wrong side of 0, doubled)."
        ));
        assert!(!looks_like_code(
            " for each row we compute the running mean."
        ));
        assert!(!looks_like_code(
            " with these settings the model converges."
        ));
        assert!(!looks_like_code(" from a distance the curve looks linear"));
        // Genuine commented-out imports are still caught.
        assert!(looks_like_code(" from a import b"));
        assert!(looks_like_code(" import os"));
    }
}
