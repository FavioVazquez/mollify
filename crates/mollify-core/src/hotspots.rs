//! Churn × complexity hotspot ranking — a refactor-priority signal that is
//! genuinely unfilled in FOSS Python tooling (RESEARCH.md §8.3). A file that is
//! both **complex** and **frequently changed** is where bugs cluster.
//!
//! Churn = commits touching the file (from `git log`). File complexity = sum of
//! per-function cyclomatic complexity. Score = churn × complexity. Files above
//! both thresholds are flagged, highest score first.

use crate::fingerprint::fingerprint;
use crate::git;
use camino::Utf8Path;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

const MIN_CHURN: u32 = 3;
const MIN_COMPLEXITY: u32 = 15;

pub fn analyze(root: &Utf8Path, graph: &ModuleGraph) -> Vec<Finding> {
    let Some(churn) = git::file_churn(root) else {
        return Vec::new(); // not a git repo → no churn signal
    };
    let mut scored: Vec<(f64, u32, u32, &str, &mollify_graph::ModuleInfo)> = Vec::new();
    for m in &graph.modules {
        let complexity: u32 = m.parsed.functions.iter().map(|f| f.cyclomatic).sum();
        let rel = m
            .path
            .strip_prefix(root)
            .unwrap_or(&m.path)
            .as_str()
            .trim_start_matches("./");
        let c = churn
            .get(rel)
            .copied()
            .or_else(|| {
                // Fallback: match by file name, anchored at a path-separator
                // boundary so `app.py` never claims `myapp.py`'s churn. Take
                // the smallest matching key for a deterministic winner.
                m.path.file_name().and_then(|n| {
                    let suffix = format!("/{n}");
                    churn
                        .iter()
                        .filter(|(k, _)| k.as_str() == n || k.ends_with(&suffix))
                        .min_by(|a, b| a.0.cmp(b.0))
                        .map(|(_, v)| *v)
                })
            })
            .unwrap_or(0);
        if c >= MIN_CHURN && complexity >= MIN_COMPLEXITY {
            scored.push((
                c as f64 * complexity as f64,
                c,
                complexity,
                m.dotted.as_str(),
                m,
            ));
        }
    }
    // Highest score first, deterministic tie-break by path.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.4.path.cmp(&b.4.path))
    });

    scored
        .into_iter()
        .map(|(score, churn, complexity, dotted, m)| {
            let path = &m.path;
            let rule = "hotspot";
            Finding {
                fingerprint: fingerprint(rule, &[m.rel.as_str()]),
                rule: rule.into(),
                category: Category::Complexity,
                severity: Severity::Warn,
                confidence: Confidence::Likely,
                attribution: None,
                reason: format!(
                    "refactor-priority hotspot `{dotted}`: churn {churn} commits × complexity {complexity} = score {score:.0}"
                ),
                location: Location {
                    path: path.to_owned(),
                    line: 1,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "prioritize-refactor".into(),
                    description: "High-churn, high-complexity file — prioritize for refactoring/tests".into(),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[hotspot]".into()),
                }],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use mollify_graph::discover_python_files;
    use std::process::Command;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-hot-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    fn git(root: &Utf8Path, args: &[&str]) {
        Command::new("git")
            .arg("-C")
            .arg(root.as_str())
            .args(args)
            .output()
            .unwrap();
    }

    #[test]
    fn flags_high_churn_high_complexity() {
        let d = temp("hot");
        git(&d, &["init"]);
        git(&d, &["config", "user.email", "t@t.co"]);
        git(&d, &["config", "user.name", "t"]);
        // A complex file, committed several times.
        let mut body = String::from("def big(x):\n");
        for i in 0..20 {
            body.push_str(&format!("    if x == {i}:\n        x += {i}\n"));
        }
        body.push_str("    return x\n");
        for n in 0..4 {
            std::fs::write(d.join("hot.py"), format!("{body}# rev {n}\n")).unwrap();
            git(&d, &["add", "-A"]);
            git(&d, &["commit", "-m", &format!("c{n}")]);
        }
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(f.iter().any(|x| x.rule == "hotspot"), "got {f:?}");
        std::fs::remove_dir_all(&d).ok();
    }
}
