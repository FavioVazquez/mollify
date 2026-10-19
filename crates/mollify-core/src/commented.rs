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
    // Simple statement keywords at the start.
    let simple_starters = [
        "import ", "from ", "return ", "raise ", "assert ", "print(", "del ", "yield ", "await ",
        "lambda ",
    ];
    if b == "return" || simple_starters.iter().any(|s| b.starts_with(s)) {
        // `from X import Y` is code; "from a distance ..." is not.
        if b.starts_with("from ") {
            return b.contains(" import ");
        }
        return true;
    }
    if matches!(b, "else:" | "try:" | "finally:" | "except:") {
        return true;
    }
    // Compound-statement headers must end with `:` (avoids flagging prose like
    // `# if host is empty or not set` or `# for backward compatibility`).
    let header_starters = [
        "def ", "class ", "if ", "elif ", "for ", "while ", "except ", "with ", "async ",
    ];
    if b.ends_with(':') && header_starters.iter().any(|s| b.starts_with(s)) {
        if b.starts_with("for ") {
            return b.contains(" in ");
        }
        return true;
    }
    // `name = value` / `name(...)` / `obj.method(...)` shaped lines,
    // excluding prose with parenthetical remarks (`# allowed in RFC 822 (see 4.1)`).
    looks_like_assignment(b) || b.contains("==") || looks_like_call(b) || b.ends_with('\\')
}

/// `target = expr` where `target` is a compact identifier/attribute/subscript,
/// not multi-word English prose.
fn looks_like_assignment(b: &str) -> bool {
    let Some((lhs, rhs)) = b.split_once(" = ") else {
        return false;
    };
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    if lhs.is_empty() || rhs.is_empty() {
        return false;
    }
    let mut chars = lhs.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']' | ',' | ' ' | ':')
        })
        && lhs.split_whitespace().count() <= 3
}

/// `callee(args)` where `callee` is a dotted Python identifier (`foo` or
/// `obj.method`) with no spaces before `(` — distinguishes real calls from
/// prose ending in a parenthetical note `# something (which obsoletes RFC 2822)`.
fn looks_like_call(b: &str) -> bool {
    if !b.ends_with(')') {
        return false;
    }
    let Some((callee, _)) = b.split_once('(') else {
        return false;
    };
    !callee.is_empty()
        && callee.split('.').all(|seg| {
            let mut chars = seg.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Emit a `commented-code` finding per comment line that looks like code.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in &graph.modules {
        findings.extend(analyze_parsed_comments(&m.path, &m.rel, &m.parsed.comments));
    }
    findings
}

/// Commented-code findings from a file's source text (also the live LSP path,
/// where the display path doubles as the fingerprint identity).
pub fn analyze_source(path: &camino::Utf8Path, src: &str) -> Vec<Finding> {
    if let Ok(mut parser) = mollify_parse::PyParser::new() {
        if let Ok(parsed) = parser.parse(path, src) {
            return analyze_parsed_comments(path, path, &parsed.comments);
        }
    }
    Vec::new()
}

/// `path` is what findings display; `rel` (root-relative) is the stable
/// fingerprint identity. Uses lexer-extracted `TokenKind::Comment` lines so
/// `#` lines inside multiline docstrings are never mistaken for comments.
pub fn analyze_parsed_comments(
    path: &camino::Utf8Path,
    rel: &camino::Utf8Path,
    comments: &[(u32, String)],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut occ = crate::fingerprint::Occurrences::default();
    for (line_no, raw) in comments {
        let trimmed = raw.trim_start();
        let Some(body) = trimmed.strip_prefix('#') else {
            continue;
        };
        if !looks_like_code(body) {
            continue;
        }
        let rule = "commented-code";
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
                line: *line_no,
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
        assert!(!looks_like_code(
            " returns either a Terminal or a TokenList representing the RFC object"
        ));
        assert!(!looks_like_code(
            " allowed in RFC 822 (which obsoletes RFC 2822)"
        ));
        assert!(!looks_like_code(" then:"));
        // Genuine commented-out imports are still caught.
        assert!(looks_like_code(" from a import b"));
        assert!(looks_like_code(" import os"));
        // Docstring lines starting with `#` are not comments at all.
        let hits = analyze_source(
            camino::Utf8Path::new("a.py"),
            "def f():\n    \"\"\"\n    # import os\n    # x = compute()\n    \"\"\"\n    return 1\n",
        );
        assert!(hits.is_empty(), "docstring flagged: {hits:?}");
    }
}
