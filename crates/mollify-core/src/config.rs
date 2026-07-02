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
    /// Extra directory names pruned from discovery, in addition to the
    /// builtin denylist (VCS metadata, virtualenvs, build/cache output —
    /// see `mollify_graph::discover_python_files`).
    pub exclude_dirs: Vec<String>,
    pub max_cyclomatic: u32,
    pub max_cognitive: u32,
    /// Minimum normalized-token window for a duplication clone (default 40).
    pub dup_min_tokens: usize,
    /// Minimum line span for a duplication clone (default 5).
    pub dup_min_lines: u32,
    /// Architecture preset name (informational): layered | hexagonal | feature-sliced | bulletproof.
    pub arch_preset: Option<String>,
    /// Ordered layer names, top (most dependent) → bottom. A layer may import
    /// same/lower layers; importing a higher layer is a `layer-violation`.
    pub arch_layers: Vec<String>,
    /// Declarative rule packs: banned imports / calls, optionally path-scoped.
    pub policies: Vec<Policy>,
    /// Declarative import contracts (import-linter / tach style).
    pub contracts: Contracts,
}

/// Module-boundary contracts checked against the import graph.
#[derive(Debug, Clone, Default)]
pub struct Contracts {
    /// `from` module(s) must not import any `to` module (by dotted prefix).
    pub forbidden: Vec<ForbiddenContract>,
    /// Each group is a set of modules that must not import one another.
    pub independent: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ForbiddenContract {
    pub from: String,
    pub to: Vec<String>,
}

/// One declarative policy ("rule pack" entry): forbid an import and/or a call,
/// optionally only within certain path substrings.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Stable rule id surfaced on findings (e.g. `no-requests-in-domain`).
    pub id: String,
    /// Forbidden import module prefix (e.g. `requests`, `django.db`).
    pub forbid_import: Option<String>,
    /// Forbidden call callee (e.g. `print`, `os.system`, `subprocess`).
    pub forbid_call: Option<String>,
    /// Path substrings this policy applies to; empty = whole project.
    pub in_paths: Vec<String>,
    /// Human explanation shown in the finding reason.
    pub message: Option<String>,
    pub severity: Severity,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            severity: FxHashMap::default(),
            ignore: Vec::new(),
            exclude_dirs: Vec::new(),
            max_cyclomatic: crate::complexity::DEFAULT_CYCLOMATIC,
            max_cognitive: crate::complexity::DEFAULT_COGNITIVE,
            dup_min_tokens: crate::dupes::MIN_TOKENS,
            dup_min_lines: crate::dupes::MIN_LINES,
            arch_preset: None,
            arch_layers: Vec::new(),
            policies: Vec::new(),
            contracts: Contracts::default(),
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
    if let Some(ex) = v.get("exclude_dirs").and_then(|i| i.as_array()) {
        cfg.exclude_dirs = ex
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
    if let Some(dup) = v.get("duplication").and_then(|d| d.as_object()) {
        if let Some(n) = dup.get("min_tokens").and_then(|x| x.as_u64()) {
            cfg.dup_min_tokens = n as usize;
        }
        if let Some(n) = dup.get("min_lines").and_then(|x| x.as_u64()) {
            cfg.dup_min_lines = n as u32;
        }
    }
    if let Some(arch) = v.get("architecture").and_then(|a| a.as_object()) {
        cfg.arch_preset = arch
            .get("preset")
            .and_then(|p| p.as_str())
            .map(String::from);
        if let Some(layers) = arch.get("layers").and_then(|l| l.as_array()) {
            cfg.arch_layers = layers
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect();
        }
        // An explicit `layers` list always wins; otherwise a known `preset`
        // expands to a conventional ordering so users can opt in with one key.
        if cfg.arch_layers.is_empty() {
            if let Some(preset) = cfg.arch_preset.as_deref() {
                cfg.arch_layers = preset_layers(preset);
            }
        }
    }
    if let Some(contracts) = v.get("contracts").and_then(|c| c.as_object()) {
        if let Some(arr) = contracts.get("forbidden").and_then(|f| f.as_array()) {
            for c in arr {
                let Some(from) = c.get("from").and_then(|x| x.as_str()) else {
                    continue;
                };
                let to: Vec<String> = c
                    .get("to")
                    .and_then(|t| t.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if !to.is_empty() {
                    cfg.contracts.forbidden.push(ForbiddenContract {
                        from: from.to_string(),
                        to,
                    });
                }
            }
        }
        if let Some(arr) = contracts.get("independent").and_then(|i| i.as_array()) {
            for group in arr {
                if let Some(members) = group.as_array() {
                    let g: Vec<String> = members
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect();
                    if g.len() >= 2 {
                        cfg.contracts.independent.push(g);
                    }
                }
            }
        }
    }
    if let Some(pols) = v.get("policies").and_then(|p| p.as_array()) {
        for (i, p) in pols.iter().enumerate() {
            let Some(obj) = p.as_object() else { continue };
            let forbid_import = obj.get("forbid_import").and_then(|x| x.as_str());
            let forbid_call = obj.get("forbid_call").and_then(|x| x.as_str());
            // A policy with neither lever is inert; skip it.
            if forbid_import.is_none() && forbid_call.is_none() {
                continue;
            }
            let id = obj
                .get("id")
                .and_then(|x| x.as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("policy-{i}"));
            let in_paths = obj
                .get("in_paths")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let severity = obj
                .get("severity")
                .and_then(|s| s.as_str())
                .and_then(parse_severity)
                .unwrap_or(Severity::Warn);
            cfg.policies.push(Policy {
                id,
                forbid_import: forbid_import.map(String::from),
                forbid_call: forbid_call.map(String::from),
                in_paths,
                message: obj
                    .get("message")
                    .and_then(|x| x.as_str())
                    .map(String::from),
                severity,
            });
        }
    }
    cfg
}

/// Default ordered layer names (top/most-dependent → bottom) for a named preset.
/// Unknown presets yield an empty list (the layer engine then does nothing).
fn preset_layers(preset: &str) -> Vec<String> {
    let names: &[&str] = match preset.to_ascii_lowercase().as_str() {
        // Classic n-tier: presentation depends on application depends on domain…
        "layered" => &["presentation", "application", "domain", "infrastructure"],
        // Ports-and-adapters: adapters/app may depend on domain, never the reverse.
        "hexagonal" => &["adapters", "application", "domain"],
        // Bulletproof-style: features → entities → shared.
        "feature-sliced" | "bulletproof" => &["app", "features", "entities", "shared"],
        _ => &[],
    };
    names.iter().map(|s| s.to_string()).collect()
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
        Category::Security => "security",
        // Future contract categories (the enum is #[non_exhaustive]) have no
        // config key yet; they fall through severity overrides untouched.
        _ => "unknown",
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
    fn preset_expands_to_default_layers() {
        assert_eq!(preset_layers("hexagonal").len(), 3);
        assert_eq!(preset_layers("layered")[0], "presentation");
        assert!(preset_layers("nonsense").is_empty());
    }

    #[test]
    fn off_drops_finding() {
        let mut cfg = Config::default();
        cfg.severity.insert("dead-code".into(), Severity::Off);
        let mut f = vec![finding("unused-export", "a.py")];
        apply(&cfg, &mut f);
        assert!(f.is_empty());
    }

    #[test]
    fn load_parses_exclude_dirs() {
        let base = std::env::temp_dir().join(format!(
            "mollify-config-test-{}-exclude-dirs",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(base.clone()).unwrap();
        std::fs::write(
            root.join(".mollifyrc.json"),
            r#"{"exclude_dirs": ["vendor", "third_party"]}"#,
        )
        .unwrap();
        let cfg = load(&root);
        assert_eq!(cfg.exclude_dirs, vec!["vendor", "third_party"]);
        std::fs::remove_dir_all(&base).ok();
    }
}
