//! Runtime-coverage merge — the "cold path" signal. Cross-references the static
//! function map against a `coverage.py` JSON report (`coverage json`): a function
//! that is statically reachable but has **zero executed lines** is a strong
//! delete/triage candidate. This is fallow's paid differentiator, here free
//! — Python makes it cheap (PEP 669 / SlipCover).

use crate::fingerprint::fingerprint;
use camino::Utf8Path;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use rustc_hash::{FxHashMap, FxHashSet};

/// Analyze cold code given a `coverage.py` JSON report at `coverage_path`.
pub fn analyze(root: &Utf8Path, graph: &ModuleGraph, coverage_path: &Utf8Path) -> Vec<Finding> {
    let Ok(text) = std::fs::read_to_string(coverage_path) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(files) = json.get("files").and_then(|f| f.as_object()) else {
        return Vec::new();
    };

    // Map coverage entries by both full key and trailing file name.
    let mut by_key: FxHashMap<String, FxHashSet<u32>> = FxHashMap::default();
    for (key, val) in files {
        let mut set = FxHashSet::default();
        if let Some(lines) = val.get("executed_lines").and_then(|l| l.as_array()) {
            for l in lines {
                if let Some(n) = l.as_u64() {
                    set.insert(n as u32);
                }
            }
        }
        by_key.insert(key.clone(), set);
    }

    let mut findings = Vec::new();
    for m in &graph.modules {
        let executed = match_coverage(root, &m.path, &by_key);
        let Some(executed) = executed else {
            continue; // no coverage data for this file → no claim
        };
        let mut occ = crate::fingerprint::Occurrences::default();
        for f in &m.parsed.functions {
            let occurrence = occ.next(&f.name);
            // Importing a module executes every `def` statement, so the def
            // line alone proves nothing about the body. A function "ran" only
            // if a line strictly inside its body executed; one-line defs (body
            // on the def line) stay conservative and count as ran.
            let ran = if f.line >= f.end_line {
                executed.contains(&f.line)
            } else {
                (f.line + 1..=f.end_line).any(|ln| executed.contains(&ln))
            };
            if ran {
                continue;
            }
            let rule = "cold-code";
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[m.rel.as_str(), &f.name, &occurrence]),
                rule: rule.into(),
                category: Category::DeadCode,
                severity: Severity::Warn,
                confidence: Confidence::Likely,
                attribution: None,
                reason: format!(
                    "function `{}` is reachable but never executed in the provided coverage (cold path)",
                    f.name
                ),
                location: Location {
                    path: m.path.clone(),
                    line: f.line,
                    column: 0,
                    end_line: Some(f.end_line),
                },
                actions: vec![Action {
                    kind: "review-cold-code".into(),
                    description: format!(
                        "`{}` ran zero times in this coverage — verify it's dead before removing",
                        f.name
                    ),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[cold-code]".into()),
                }],
            });
        }
    }
    findings
}

/// Find the executed-line set for a module by exact key, then rel-path, then
/// trailing file-name match.
fn match_coverage<'a>(
    root: &Utf8Path,
    path: &Utf8Path,
    by_key: &'a FxHashMap<String, FxHashSet<u32>>,
) -> Option<&'a FxHashSet<u32>> {
    if let Some(s) = by_key.get(path.as_str()) {
        return Some(s);
    }
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .as_str()
        .trim_start_matches("./");
    if let Some(s) = by_key.get(rel) {
        return Some(s);
    }
    // Fallback by file name, anchored at a path-separator boundary so
    // `app.py` never inherits `myapp.py`'s coverage; smallest key wins for
    // determinism.
    let name = path.file_name()?;
    let suffix = format!("/{name}");
    by_key
        .iter()
        .filter(|(k, _)| k.as_str() == name || k.ends_with(&suffix))
        .min_by(|a, b| a.0.cmp(b.0))
        .map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-cov-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn flags_cold_function() {
        let d = temp("cov");
        // hot() on lines 1-2, cold() on lines 4-5.
        std::fs::write(
            d.join("app.py"),
            "def hot():\n    return 1\n\ndef cold():\n    return 2\n",
        )
        .unwrap();
        // coverage report: only line 2 executed.
        let cov = d.join("coverage.json");
        std::fs::write(&cov, r#"{"files":{"app.py":{"executed_lines":[1,2]}}}"#).unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g, &cov);
        assert!(
            f.iter()
                .any(|x| x.rule == "cold-code" && x.reason.contains("cold")),
            "got {f:?}"
        );
        assert!(!f.iter().any(|x| x.reason.contains("`hot`")));
        std::fs::remove_dir_all(&d).ok();
    }
}
