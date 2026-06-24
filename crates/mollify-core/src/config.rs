//! `.mollifyrc.json` configuration: severity overrides (per rule or category),
//! ignore globs, and complexity thresholds. Absent config → sensible defaults.

use camino::Utf8Path;
use mollify_types::{Category, Finding, Severity};
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct Config {
    /// Override severity by rule id (e.g. "unused-export") or category
    /// ("dead-code"). Rule id wins over category.
    pub severity: FxHashMap<String, Severity>,
    /// Path substrings to ignore (simple contains-match; globs later).
    pub ignore: Vec<String>,
    pub max_cyclomatic: u32,
    pub max_cognitive: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            severity: FxHashMap::default(),
            ignore: Vec::new(),
            max_cyclomatic: crate::complexity::DEFAULT_CYCLOMATIC,
            max_cognitive: crate::complexity::DEFAULT_COGNITIVE,
        }
    }
}

/// Load `.mollifyrc.json` from `root` (or defaults if missing/invalid).
pub fn load(root: &Utf8Path) -> Config {
    let mut cfg = Config::default();
    let path = root.join(".mollifyrc.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return cfg;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return cfg;
    };
    if let Some(sev) = v.get("severity").and_then(|s| s.as_object()) {
        for (k, val) in sev {
            if let Some(s) = val.as_str().and_then(parse_severity) {
                cfg.severity.insert(k.clone(), s);
            }
        }
    }
    if let Some(ig) = v.get("ignore").and_then(|i| i.as_array()) {
        cfg.ignore = ig
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
    }
    if let Some(c) = v.get("max_cyclomatic").and_then(|n| n.as_u64()) {
        cfg.max_cyclomatic = c as u32;
    }
    if let Some(c) = v.get("max_cognitive").and_then(|n| n.as_u64()) {
        cfg.max_cognitive = c as u32;
    }
    cfg
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s.to_ascii_lowercase().as_str() {
        "error" => Some(Severity::Error),
        "warn" | "warning" => Some(Severity::Warn),
        "off" | "ignore" => Some(Severity::Off),
        _ => None,
    }
}

fn category_key(c: Category) -> &'static str {
    match c {
        Category::DeadCode => "dead-code",
        Category::Duplication => "duplication",
        Category::CircularDependency => "circular-dependency",
        Category::Complexity => "complexity",
        Category::Architecture => "architecture",
        Category::DependencyHygiene => "dependency-hygiene",
        Category::TypeHealth => "type-health",
    }
}

/// Apply config to findings: drop ignored paths and `off` findings, and override
/// severities (rule id first, then category).
pub fn apply(cfg: &Config, findings: &mut Vec<Finding>) {
    for f in findings.iter_mut() {
        if let Some(s) = cfg
            .severity
            .get(&f.rule)
            .or_else(|| cfg.severity.get(category_key(f.category)))
        {
            f.severity = *s;
        }
    }
    findings.retain(|f| {
        if f.severity == Severity::Off {
            return false;
        }
        let p = f.location.path.as_str();
        !cfg.ignore.iter().any(|ig| p.contains(ig.as_str()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mollify_types::{Category, Location};

    fn finding(rule: &str, path: &str) -> Finding {
        Finding {
            fingerprint: "x".into(),
            rule: rule.into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence: mollify_types::Confidence::Likely,
            attribution: None,
            reason: "r".into(),
            location: Location {
                path: path.into(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![],
        }
    }

    #[test]
    fn severity_override_and_ignore() {
        let mut cfg = Config::default();
        cfg.severity.insert("unused-export".into(), Severity::Error);
        cfg.ignore.push("tests/".into());
        let mut f = vec![
            finding("unused-export", "src/a.py"),
            finding("unused-export", "tests/b.py"),
        ];
        apply(&cfg, &mut f);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].location.path, "src/a.py");
        assert_eq!(f[0].severity, Severity::Error);
    }

    #[test]
    fn off_drops_finding() {
        let mut cfg = Config::default();
        cfg.severity.insert("dead-code".into(), Severity::Off);
        let mut f = vec![finding("unused-export", "a.py")];
        apply(&cfg, &mut f);
        assert!(f.is_empty());
    }
}
