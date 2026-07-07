//! Regression baselines: snapshot the set of finding fingerprints, then on a
//! later run report only what's **new** relative to that snapshot. This is the
//! "no new issues" CI gate (complementary to git-attribution `--gate new-only`):
//! it works without git and survives file moves, because fingerprints are
//! content-derived (the evidence-preserving invariant).

use camino::Utf8Path;
use mollify_types::Finding;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema: String,
    /// Sorted, de-duplicated finding fingerprints captured at snapshot time.
    pub fingerprints: Vec<String>,
}

const SCHEMA: &str = "mollify-baseline/1";

impl Baseline {
    /// Build a baseline from the current findings.
    pub fn from_findings(findings: &[Finding]) -> Baseline {
        let mut fingerprints: Vec<String> =
            findings.iter().map(|f| f.fingerprint.clone()).collect();
        fingerprints.sort();
        fingerprints.dedup();
        Baseline {
            schema: SCHEMA.into(),
            fingerprints,
        }
    }

    /// Write the baseline to `path` as pretty JSON.
    pub fn save(&self, path: &Utf8Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let json = serde_json::to_string_pretty(self).unwrap();
        std::fs::write(path, json)
    }

    /// Load a baseline from `path` (None if missing/invalid).
    pub fn load(path: &Utf8Path) -> Option<Baseline> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// Partition `findings` into (new, known) relative to a baseline's fingerprints.
pub fn split_new<'a>(
    findings: &'a [Finding],
    baseline: &Baseline,
) -> (Vec<&'a Finding>, Vec<&'a Finding>) {
    let known: rustc_hash::FxHashSet<&str> =
        baseline.fingerprints.iter().map(|s| s.as_str()).collect();
    findings
        .iter()
        .partition(|f| !known.contains(f.fingerprint.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mollify_types::{Category, Confidence, Location, Severity};

    fn finding(fp: &str) -> Finding {
        Finding {
            fingerprint: fp.into(),
            rule: "r".into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence: Confidence::Likely,
            attribution: None,
            reason: "x".into(),
            location: Location {
                path: "a.py".into(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![],
        }
    }

    #[test]
    fn new_findings_are_those_not_in_baseline() {
        let base = Baseline::from_findings(&[finding("a:1"), finding("b:2")]);
        let current = vec![finding("a:1"), finding("c:3")];
        let (new, known) = split_new(&current, &base);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].fingerprint, "c:3");
        assert_eq!(known.len(), 1);
    }

    #[test]
    fn roundtrips_through_disk() {
        let dir = std::env::temp_dir().join(format!("mollify-baseline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = camino::Utf8PathBuf::from_path_buf(dir.join("bl.json")).unwrap();
        let b = Baseline::from_findings(&[finding("a:1")]);
        b.save(&p).unwrap();
        let loaded = Baseline::load(&p).unwrap();
        assert_eq!(loaded.fingerprints, vec!["a:1".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
