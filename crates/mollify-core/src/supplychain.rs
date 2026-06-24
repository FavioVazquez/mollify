//! Supply-chain analysis: cross-reference **pinned/locked dependency versions**
//! against a local **advisory database** and flag versions that fall in a known
//! vulnerable range (`vulnerable-dependency`).
//!
//! Determinism is preserved by design: the advisory DB is an *input file*, never
//! a live network call. Same `(lockfile, advisory-db)` → byte-identical output.
//! Refresh the DB out-of-band with `scripts/fetch-advisories.py` (which pulls
//! from OSV / safety-db). Mollify itself never reaches the network.

use crate::fingerprint::fingerprint;
use crate::known::normalize_dist;
use crate::version::matches_spec;
use camino::Utf8Path;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use serde::Deserialize;

/// One advisory in the normalized `mollify-advisories/1` schema.
#[derive(Debug, Clone, Deserialize)]
pub struct Advisory {
    pub id: String,
    pub package: String,
    /// Affected version specs; ANY matching spec ⇒ vulnerable (OR).
    #[serde(default)]
    pub specs: Vec<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AdvisoryDb {
    #[serde(default)]
    advisories: Vec<Advisory>,
}

/// A concrete, resolved dependency version found in a lockfile / pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedDep {
    pub name: String,
    pub version: String,
    pub source: camino::Utf8PathBuf,
    pub line: u32,
}

/// Load and index the advisory DB at `db_path` (or `None` if unreadable/invalid).
pub fn load_db(db_path: &Utf8Path) -> Option<Vec<Advisory>> {
    let text = std::fs::read_to_string(db_path).ok()?;
    let db: AdvisoryDb = serde_json::from_str(&text).ok()?;
    Some(db.advisories)
}

/// Analyze a project: collect pinned versions, match against `advisories`.
pub fn analyze(root: &Utf8Path, advisories: &[Advisory]) -> Vec<Finding> {
    let pins = collect_pins(root);
    analyze_pins(&pins, advisories)
}

/// Match a set of pinned deps against advisories (pure; testable).
pub fn analyze_pins(pins: &[PinnedDep], advisories: &[Advisory]) -> Vec<Finding> {
    let mut findings = Vec::new();
    // The same CVE is often published under several advisory ids (GHSA + PYSEC);
    // collapse them so each (package, version, CVE) is reported once.
    let mut seen: rustc_hash::FxHashSet<(String, String, String)> =
        rustc_hash::FxHashSet::default();
    for pin in pins {
        for adv in advisories {
            if normalize_dist(&adv.package) != pin.name {
                continue;
            }
            // ANY affected spec matching the pinned version ⇒ vulnerable.
            // An advisory with no specs is treated as "all versions affected".
            let hit =
                adv.specs.is_empty() || adv.specs.iter().any(|s| matches_spec(&pin.version, s));
            if !hit {
                continue;
            }
            let rule = "vulnerable-dependency";
            let alias = adv
                .aliases
                .iter()
                .find(|a| a.starts_with("CVE-"))
                .cloned()
                .unwrap_or_else(|| adv.id.clone());
            if !seen.insert((pin.name.clone(), pin.version.clone(), alias.clone())) {
                continue; // same CVE already reported for this pin
            }
            let summary = if adv.summary.is_empty() {
                String::new()
            } else {
                format!(" — {}", adv.summary)
            };
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[&pin.name, &pin.version, &adv.id]),
                rule: rule.into(),
                category: Category::Security,
                severity: Severity::Warn,
                confidence: Confidence::Certain,
                attribution: None,
                reason: format!(
                    "`{}` {} is affected by {alias}{summary}",
                    pin.name, pin.version
                ),
                location: Location {
                    path: pin.source.clone(),
                    line: pin.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "upgrade-dependency".into(),
                    description: format!(
                        "Upgrade `{}` out of the affected range for {} ({alias}).",
                        pin.name, adv.id
                    ),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[vulnerable-dependency]".into()),
                }],
            });
        }
    }
    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.reason.cmp(&b.reason))
    });
    findings.dedup_by(|a, b| a.fingerprint == b.fingerprint);
    findings
}

/// Collect concrete (name, version) pins from common lock/pin files.
pub fn collect_pins(root: &Utf8Path) -> Vec<PinnedDep> {
    let mut pins = Vec::new();
    // requirements*.txt — `name==version` lines.
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("requirements") && name.ends_with(".txt") {
            if let Ok(p) = camino::Utf8PathBuf::from_path_buf(entry.path()) {
                parse_requirements(&p, &mut pins);
            }
        }
    }
    // poetry.lock / uv.lock — TOML [[package]] tables.
    for lock in ["poetry.lock", "uv.lock"] {
        let p = root.join(lock);
        if p.exists() {
            parse_toml_lock(&p, &mut pins);
        }
    }
    pins.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
    pins.dedup();
    pins
}

fn parse_requirements(path: &Utf8Path, out: &mut Vec<PinnedDep>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('-') {
            continue;
        }
        // Only exact pins are unambiguous: `name==1.2.3` (drop extras/markers).
        let Some((name_part, rest)) = line.split_once("==") else {
            continue;
        };
        let name = normalize_dist(name_part.split('[').next().unwrap_or(name_part).trim());
        let version = rest
            .split([';', ' ', ','])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !name.is_empty() && !version.is_empty() {
            out.push(PinnedDep {
                name,
                version,
                source: path.to_owned(),
                line: i as u32 + 1,
            });
        }
    }
}

fn parse_toml_lock(path: &Utf8Path, out: &mut Vec<PinnedDep>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return;
    };
    let Some(pkgs) = table.get("package").and_then(|p| p.as_array()) else {
        return;
    };
    for pkg in pkgs {
        let (Some(name), Some(version)) = (
            pkg.get("name").and_then(|v| v.as_str()),
            pkg.get("version").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        out.push(PinnedDep {
            name: normalize_dist(name),
            version: version.to_string(),
            source: path.to_owned(),
            line: 1,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-sc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    fn adv(id: &str, pkg: &str, specs: &[&str]) -> Advisory {
        Advisory {
            id: id.into(),
            package: pkg.into(),
            specs: specs.iter().map(|s| s.to_string()).collect(),
            summary: "test advisory".into(),
            aliases: vec!["CVE-2020-00000".into()],
            severity: Some("high".into()),
        }
    }

    #[test]
    fn flags_pinned_vulnerable_version() {
        let pins = vec![
            PinnedDep {
                name: "jinja2".into(),
                version: "2.4.1".into(),
                source: "requirements.txt".into(),
                line: 3,
            },
            PinnedDep {
                name: "jinja2".into(),
                version: "3.1.5".into(),
                source: "requirements.txt".into(),
                line: 4,
            },
        ];
        let advisories = vec![adv("PYSEC-1", "Jinja2", &["<2.11.3"])];
        let f = analyze_pins(&pins, &advisories);
        assert_eq!(f.len(), 1, "got {f:?}");
        assert!(f[0].reason.contains("2.4.1"));
        assert!(f[0].reason.contains("CVE-2020-00000"));
    }

    #[test]
    fn parses_requirements_and_lock() {
        let d = temp("pins");
        std::fs::write(
            d.join("requirements.txt"),
            "# comment\nDjango==3.2.0\nrequests>=2.0  # range, skipped\nflask==2.0.1 ; python_version>='3.7'\n",
        )
        .unwrap();
        std::fs::write(
            d.join("poetry.lock"),
            "[[package]]\nname = \"urllib3\"\nversion = \"1.26.4\"\n",
        )
        .unwrap();
        let pins = collect_pins(&d);
        assert!(pins
            .iter()
            .any(|p| p.name == "django" && p.version == "3.2.0"));
        assert!(pins
            .iter()
            .any(|p| p.name == "flask" && p.version == "2.0.1"));
        assert!(pins
            .iter()
            .any(|p| p.name == "urllib3" && p.version == "1.26.4"));
        assert!(
            !pins.iter().any(|p| p.name == "requests"),
            "ranges not pinned"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn loads_db_from_disk() {
        let d = temp("db");
        let db = d.join("adv.json");
        std::fs::write(
            &db,
            r#"{"schema":"mollify-advisories/1","advisories":[{"id":"PYSEC-9","package":"flask","specs":["<2.0.0"],"summary":"x","aliases":["CVE-1"]}]}"#,
        )
        .unwrap();
        let advisories = load_db(&db).unwrap();
        assert_eq!(advisories.len(), 1);
        assert_eq!(advisories[0].package, "flask");
        std::fs::remove_dir_all(&d).ok();
    }
}
