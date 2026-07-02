//! Duplication engine — exact token-clone detection via suffix array + LCP.
//!
//! Algorithm: a lightweight Python tokenizer produces a normalized token stream
//! per file (string/number literals blinded → Type-1.5). Every file's tokens are
//! concatenated into one global symbol sequence, separated by **unique
//! sentinels** so no clone can straddle a file boundary. A linear-time **SA-IS
//! suffix array** + **Kasai LCP** array is built over that sequence
//! (`crate::suffix`); maximal runs of `LCP ≥ min_tokens` are exact maximal
//! repeats — the clone classes. Longer clones are emitted first and cover the
//! shorter shifted sub-windows they contain, so each duplicated region is
//! reported once. O(n) construction, exact matches (no hash collisions).

use crate::fingerprint::fingerprint;
use crate::suffix::{lcp_kasai, suffix_array};
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

/// Duplication analysis with the default thresholds.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    analyze_with(graph, MIN_TOKENS, MIN_LINES)
}

/// Duplication analysis with a configurable `min_tokens` clone window and
/// minimum clone line `min_lines` span.
pub fn analyze_with(graph: &ModuleGraph, min_tokens: usize, min_lines: u32) -> Vec<Finding> {
    let min_tokens = min_tokens.max(8) as u32;

    // Tokenize each module (deterministic order via sorted modules).
    // read_source, not read_to_string: notebooks must contribute their code
    // cells, not their JSON scaffolding (which is near-identical across
    // notebooks and produces bogus clone families).
    let mut files: Vec<(usize, Vec<Tok>)> = Vec::new();
    for (i, m) in graph.modules.iter().enumerate() {
        if let Some(src) = mollify_graph::read_source(&m.path) {
            let toks = tokenize(&src);
            if toks.len() as u32 >= min_tokens {
                files.push((i, toks));
            }
        }
    }
    if files.is_empty() {
        return Vec::new();
    }

    // Intern token strings to compact ids in 1..=D (0 reserved for the sentinel).
    let mut dict: FxHashMap<&str, u32> = FxHashMap::default();
    for (_m, toks) in &files {
        for t in toks {
            let next = dict.len() as u32 + 1;
            dict.entry(t.norm.as_str()).or_insert(next);
        }
    }
    let d = dict.len() as u32;

    // Build the global sequence: file tokens, a unique separator after each file,
    // and a single terminating 0. Separators get ids `d+1 ..= d+files.len()` so
    // they are unique and larger than any token — no clone can cross them.
    let mut seq: Vec<u32> = Vec::new();
    let mut pos_map: Vec<Option<(usize, usize)>> = Vec::new();
    for (fi, (_m, toks)) in files.iter().enumerate() {
        for (ti, t) in toks.iter().enumerate() {
            seq.push(dict[t.norm.as_str()]);
            pos_map.push(Some((fi, ti)));
        }
        seq.push(d + 1 + fi as u32); // unique separator
        pos_map.push(None);
    }
    seq.push(0); // unique smallest terminator
    pos_map.push(None);

    let alphabet_size = (d + files.len() as u32 + 1) as usize + 1;
    let sa = suffix_array(&seq, alphabet_size);
    let lcp = lcp_kasai(&seq, &sa);
    let n = seq.len();

    // Collect maximal runs of LCP ≥ min_tokens. Each run groups the suffixes
    // `sa[lo..=hi]`; their common prefix length is the minimum LCP in the run.
    let mut blocks: Vec<(usize, usize, u32)> = Vec::new();
    let mut k = 1;
    while k < n {
        if lcp[k] >= min_tokens {
            let lo = k - 1;
            let mut minl = u32::MAX;
            while k < n && lcp[k] >= min_tokens {
                minl = minl.min(lcp[k]);
                k += 1;
            }
            blocks.push((lo, k - 1, minl));
        } else {
            k += 1;
        }
    }

    // Emit longer clones first so they cover the shorter shifted sub-windows.
    blocks.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));

    let mut covered: FxHashMap<usize, Vec<(usize, usize)>> = FxHashMap::default();
    let mut findings = Vec::new();

    for (lo, hi, len) in blocks {
        let len = len as usize;
        // Gather this class's occurrences (skip separators and already-covered).
        let mut occ: Vec<(usize, usize)> = Vec::new();
        for &si in &sa[lo..=hi] {
            if let Some((fi, ti)) = pos_map[si as usize] {
                if !is_covered(&covered, fi, ti) {
                    occ.push((fi, ti));
                }
            }
        }
        occ.sort_unstable();
        occ.dedup();
        if occ.len() < 2 {
            continue;
        }

        // Mark covered and build instance line ranges.
        let mut instances: Vec<(usize, u32, u32)> = Vec::new();
        for &(fi, ti) in &occ {
            covered.entry(fi).or_default().push((ti, ti + len));
            let toks = &files[fi].1;
            let end_idx = (ti + len - 1).min(toks.len() - 1);
            instances.push((files[fi].0, toks[ti].line, toks[end_idx].line));
        }
        instances.sort();
        instances.dedup();
        let span = instances
            .iter()
            .map(|(_, s, e)| e.saturating_sub(*s) + 1)
            .max()
            .unwrap_or(0);
        if instances.len() < 2 || span < min_lines {
            continue;
        }

        // Stable fingerprint from the clone's normalized content + length.
        let (cfi, cti) = occ[0];
        let content_hash = clone_hash(&files[cfi].1[cti..cti + len]);

        let locs: Vec<String> = instances
            .iter()
            .map(|(mi, s, _e)| format!("{}:{}", graph.modules[*mi].path, s))
            .collect();
        let rule = "duplication";
        let (first_mi, first_s, first_e) = instances[0];
        findings.push(Finding {
            fingerprint: fingerprint(rule, &[&format!("{content_hash:016x}"), &len.to_string()]),
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

fn clone_hash(toks: &[Tok]) -> u64 {
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
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
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
            // String prefix (r"...", f"..."): fold into a STR token.
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
            if !triple {
                // Unterminated single-quoted string: stop *before* the
                // newline and don't count it — the caller's main loop will
                // see it and count it exactly once.
                return (i, lines);
            }
            lines += 1;
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
    fn notebooks_compare_code_cells_not_json_scaffolding() {
        // Two notebooks with completely different code share near-identical
        // JSON scaffolding; tokenizing the raw file produced bogus clones.
        let d = temp("nbdup");
        let nb = |code: &str| {
            format!(
                r#"{{
 "cells": [
  {{
   "cell_type": "code",
   "metadata": {{"collapsed": false, "scrolled": true, "tags": ["x"]}},
   "source": [{code}],
   "outputs": [],
   "execution_count": null
  }}
 ],
 "metadata": {{"kernelspec": {{"display_name": "Python 3", "language": "python", "name": "python3"}}}},
 "nbformat": 4,
 "nbformat_minor": 5
}}"#
            )
        };
        write(&d, "a.ipynb", &nb(r#""x = 1\n", "print(x)\n""#));
        write(
            &d,
            "b.ipynb",
            &nb(r#""def f(name):\n", "    return name.upper()\n""#),
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            f.is_empty(),
            "notebook JSON scaffolding reported as clone: {f:?}"
        );
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

    #[test]
    fn reports_each_duplicated_region_once() {
        // Three identical copies of a long block → a single clone family with
        // three locations, not a cascade of overlapping sub-window findings.
        let d = temp("triple");
        write(&d, "a.py", &block("f"));
        write(&d, "b.py", &block("f"));
        write(&d, "c.py", &block("f"));
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let dups: Vec<_> = f.iter().filter(|x| x.rule == "duplication").collect();
        assert_eq!(dups.len(), 1, "expected one clone family, got {dups:?}");
        assert!(
            dups[0].reason.contains("3 locations"),
            "reason: {}",
            dups[0].reason
        );
        std::fs::remove_dir_all(&d).ok();
    }
}
