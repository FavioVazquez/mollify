//! Duplication engine — token-based clone detection.
//!
//! Algorithm (v1): a lightweight Python tokenizer produces a normalized token
//! stream per file (string/number literals blinded → Type-1.5); equal-length
//! windows are Rabin-Karp hashed; colliding windows are content-verified and
//! **extended to maximal length**, then grouped into clone families. This is the
//! jscpd/CPD family of detector.
//!
//! Planned upgrade (ADR/STATUS): a **SA-IS suffix array + LCP** engine for exact
//! maximal-match detection in O(n), and identifier-blinding for full Type-2.

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use rustc_hash::FxHashMap;
use xxhash_rust::xxh3::xxh3_64;

/// Minimum window length (tokens) for a clone.
pub const MIN_TOKENS: usize = 40;
/// Minimum line span for a clone to be reported.
pub const MIN_LINES: u32 = 5;

#[derive(Clone)]
struct Tok {
    norm: String,
    line: u32,
}

pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    // Tokenize each module from disk (deterministic order via sorted modules).
    let mut files: Vec<(usize, Vec<Tok>)> = Vec::new();
    for (i, m) in graph.modules.iter().enumerate() {
        if let Ok(src) = std::fs::read_to_string(&m.path) {
            let toks = tokenize(&src);
            if toks.len() >= MIN_TOKENS {
                files.push((i, toks));
            }
        }
    }

    // Window hash -> occurrences (file-list-index, token-start).
    let mut map: FxHashMap<u64, Vec<(usize, usize)>> = FxHashMap::default();
    for (fi, (_m, toks)) in files.iter().enumerate() {
        for start in 0..=toks.len() - MIN_TOKENS {
            let h = window_hash(&toks[start..start + MIN_TOKENS]);
            map.entry(h).or_default().push((fi, start));
        }
    }

    // Deterministic iteration over candidate hashes.
    let mut hashes: Vec<u64> = map.keys().copied().collect();
    hashes.sort_unstable();

    let mut covered: FxHashMap<usize, Vec<(usize, usize)>> = FxHashMap::default(); // fi -> covered [start,end) token ranges
    let mut findings = Vec::new();

    for h in hashes {
        let occs = &map[&h];
        if occs.len() < 2 {
            continue;
        }
        // Group occurrences whose MIN_TOKENS window content actually matches the
        // first (guards against hash collisions).
        let mut sorted = occs.clone();
        sorted.sort_unstable();
        let (f0, s0) = sorted[0];
        if is_covered(&covered, f0, s0) {
            continue;
        }
        let ref_win: Vec<&str> = files[f0].1[s0..s0 + MIN_TOKENS]
            .iter()
            .map(|t| t.norm.as_str())
            .collect();
        let group: Vec<(usize, usize)> = sorted
            .iter()
            .copied()
            .filter(|&(fi, st)| {
                !is_covered(&covered, fi, st)
                    && files[fi].1[st..st + MIN_TOKENS]
                        .iter()
                        .map(|t| t.norm.as_str())
                        .eq(ref_win.iter().copied())
            })
            .collect();
        if group.len() < 2 {
            continue;
        }
        // Extend to maximal common length across all group members.
        let mut len = MIN_TOKENS;
        loop {
            let next: Option<&str> = group
                .first()
                .and_then(|&(fi, st)| files[fi].1.get(st + len).map(|t| t.norm.as_str()));
            let Some(tok) = next else { break };
            let all_match = group
                .iter()
                .all(|&(fi, st)| files[fi].1.get(st + len).map(|t| t.norm.as_str()) == Some(tok));
            if all_match {
                len += 1;
            } else {
                break;
            }
        }

        // Mark covered and build instance locations.
        let mut instances = Vec::new();
        for &(fi, st) in &group {
            covered.entry(fi).or_default().push((st, st + len));
            let toks = &files[fi].1;
            let start_line = toks[st].line;
            let end_line = toks[st + len - 1].line;
            instances.push((files[fi].0, start_line, end_line));
        }
        // Dedup identical (module,line) instances; require min line span.
        instances.sort();
        instances.dedup();
        let span = instances
            .iter()
            .map(|(_, s, e)| e.saturating_sub(*s) + 1)
            .max()
            .unwrap_or(0);
        if instances.len() < 2 || span < MIN_LINES {
            continue;
        }

        let locs: Vec<String> = instances
            .iter()
            .map(|(mi, s, _e)| format!("{}:{}", graph.modules[*mi].path, s))
            .collect();
        let rule = "duplication";
        let (first_mi, first_s, first_e) = instances[0];
        findings.push(Finding {
            fingerprint: fingerprint(rule, &[&format!("{h:016x}"), &len.to_string()]),
            rule: rule.into(),
            category: Category::Duplication,
            severity: Severity::Warn,
            confidence: Confidence::Likely,
            attribution: None,
            reason: format!(
                "duplicated block (~{len} tokens) appears in {} locations: {}",
                instances.len(),
                locs.join(", ")
            ),
            location: Location {
                path: graph.modules[first_mi].path.clone(),
                line: first_s,
                column: 0,
                end_line: Some(first_e),
            },
            actions: vec![Action {
                kind: "extract-shared".into(),
                description: "Extract the duplicated logic into a shared function/module".into(),
                auto_fixable: false,
                suppression_comment: Some("# mollify: ignore[duplication]".into()),
            }],
        });
    }

    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.location.line.cmp(&b.location.line))
    });
    findings
}

fn is_covered(covered: &FxHashMap<usize, Vec<(usize, usize)>>, fi: usize, start: usize) -> bool {
    covered
        .get(&fi)
        .is_some_and(|ranges| ranges.iter().any(|&(s, e)| start >= s && start < e))
}

fn window_hash(toks: &[Tok]) -> u64 {
    let mut buf = String::new();
    for t in toks {
        buf.push_str(&t.norm);
        buf.push('\u{1f}');
    }
    xxh3_64(buf.as_bytes())
}

/// A minimal Python tokenizer for clone detection. Skips comments/whitespace;
/// blinds string and number literals (`STR`/`NUM`); keeps identifiers, keywords,
/// operators, and punctuation verbatim. Tracks 1-based line numbers.
fn tokenize(src: &str) -> Vec<Tok> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut line = 1u32;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Strings (incl. triple-quoted). Ignore prefixes like r, b, f handled as
        // identifiers immediately followed by a quote → treat the quote here.
        if c == '"' || c == '\'' {
            let (consumed, lines) = consume_string(&b[i..], c);
            i += consumed;
            line += lines;
            out.push(Tok {
                norm: "STR".into(),
                line: line.saturating_sub(lines),
            });
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_' || b[i] == b'x')
            {
                i += 1;
            }
            let _ = start;
            out.push(Tok {
                norm: "NUM".into(),
                line,
            });
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            // Identifier possibly a string prefix (r"...", f"..."): if next is a
            // quote, fold it into a STR token.
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i] as char;
                let (consumed, lines) = consume_string(&b[i..], q);
                i += consumed;
                line += lines;
                out.push(Tok {
                    norm: "STR".into(),
                    line: line.saturating_sub(lines),
                });
                continue;
            }
            out.push(Tok {
                norm: src[start..i].to_string(),
                line,
            });
            continue;
        }
        // Operator / punctuation — single char token.
        out.push(Tok {
            norm: c.to_string(),
            line,
        });
        i += 1;
    }
    out
}

/// Consume a (possibly triple-quoted) string starting at `b[0] == quote`.
/// Returns (bytes consumed, newlines encountered).
fn consume_string(b: &[u8], quote: char) -> (usize, u32) {
    let q = quote as u8;
    let triple = b.len() >= 3 && b[1] == q && b[2] == q;
    let mut i = if triple { 3 } else { 1 };
    let mut lines = 0u32;
    while i < b.len() {
        if b[i] == b'\\' {
            if i + 1 < b.len() && b[i + 1] == b'\n' {
                lines += 1;
            }
            i += 2;
            continue;
        }
        if b[i] == b'\n' {
            lines += 1;
            if !triple {
                // unterminated single-line string; stop at newline
                return (i, lines);
            }
            i += 1;
            continue;
        }
        if b[i] == q {
            if triple {
                if i + 2 < b.len() && b[i + 1] == q && b[i + 2] == q {
                    return (i + 3, lines);
                }
                i += 1;
            } else {
                return (i + 1, lines);
            }
        } else {
            i += 1;
        }
    }
    (b.len(), lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-dup-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }
    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    fn block(name: &str) -> String {
        // ~10 lines of identical-structure code.
        format!(
            "def {name}(items):\n    total = 0\n    for it in items:\n        if it > 0:\n            total += it\n        else:\n            total -= it\n    result = total * 2\n    print(result)\n    return result\n"
        )
    }

    #[test]
    fn detects_cross_file_duplicate() {
        let d = temp("dup");
        write(&d, "a.py", &block("compute_a"));
        write(&d, "b.py", &block("compute_a"));
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(f.iter().any(|x| x.rule == "duplication"), "got {f:?}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn no_false_positive_on_distinct_code() {
        let d = temp("nodup");
        write(&d, "a.py", "def a():\n    return 1\n");
        write(&d, "b.py", "class Z:\n    x = 5\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        assert!(analyze(&g).is_empty());
        std::fs::remove_dir_all(&d).ok();
    }
}
