//! Supply-chain analysis: cross-reference **pinned/locked dependency versions**
//! against a local **advisory database** and flag versions that fall in a known
//! vulnerable range (`vulnerable-dependency`).
//!
//! Determinism is preserved by design: the advisory DB is an *input file*, never
//! a live network call. Same `(lockfile, advisory-db)` → byte-identical output.
//! Refresh the DB out-of-band with `scripts/fetch-advisories.py` (which pulls
//! from OSV / safety-db). Mollify itself never reaches the network.

use crate::fingerprint::fingerprint;
use crate::installed::Installed;
use crate::known::normalize_dist;
use crate::version::{matches_spec, specs_intersect};
use camino::Utf8Path;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use serde::{Deserialize, Serialize};

/// One advisory in the normalized `mollify-advisories/1` schema.
#[derive(Debug, Clone, Deserialize, Serialize)]
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

/// A declared dependency *constraint* (range), e.g. `requests>=2.0,<3` — as
/// opposed to a concrete pin. Empty `spec` means an unconstrained dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredRange {
    pub name: String,
    pub spec: String,
    pub source: camino::Utf8PathBuf,
    pub line: u32,
}

/// Analyze a project: match pinned versions, installed versions, and declared
/// ranges against `advisories`.
pub fn analyze(root: &Utf8Path, advisories: &[Advisory]) -> Vec<Finding> {
    let pins = collect_pins(root);
    let pinned: rustc_hash::FxHashSet<String> = pins.iter().map(|p| p.name.clone()).collect();
    let ranges = collect_declared_ranges(root);
    let installed = crate::installed::discover(root);

    let mut findings = analyze_pins(&pins, advisories);
    findings.extend(analyze_declared(
        &ranges,
        advisories,
        installed.as_ref(),
        &pinned,
    ));
    findings.sort_by(|a, b| {
        a.location
            .path
            .cmp(&b.location.path)
            .then(a.location.line.cmp(&b.location.line))
            .then(a.reason.cmp(&b.reason))
    });
    findings.dedup_by(|a, b| a.fingerprint == b.fingerprint);
    findings
}

/// Match declared *ranges* against advisories. A range is resolved precisely
/// when the package is installed (concrete version → `Certain`); otherwise we
/// flag when the declared range **intersects** a vulnerable range (`Uncertain`:
/// the range permits a vulnerable version, though it may resolve to a safe one).
pub fn analyze_declared(
    ranges: &[DeclaredRange],
    advisories: &[Advisory],
    installed: Option<&Installed>,
    pinned: &rustc_hash::FxHashSet<String>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut seen: rustc_hash::FxHashSet<(String, String, String)> =
        rustc_hash::FxHashSet::default();
    for dep in ranges {
        if pinned.contains(&dep.name) {
            continue; // an exact pin already covers this package, precisely
        }
        let installed_ver = installed.and_then(|i| i.versions.get(&dep.name).cloned());
        for adv in advisories {
            if normalize_dist(&adv.package) != dep.name {
                continue;
            }
            let alias = adv
                .aliases
                .iter()
                .find(|a| a.starts_with("CVE-"))
                .cloned()
                .unwrap_or_else(|| adv.id.clone());
            let summary = if adv.summary.is_empty() {
                String::new()
            } else {
                format!(" — {}", adv.summary)
            };

            let (matched, version_key, confidence, reason) = if let Some(ver) = &installed_ver {
                // Resolve the range to the concrete installed version.
                let hit = adv.specs.is_empty() || adv.specs.iter().any(|s| matches_spec(ver, s));
                (
                    hit,
                    ver.clone(),
                    Confidence::Certain,
                    format!(
                        "`{}` {ver} (installed, declared `{}`) is affected by {alias}{summary}",
                        dep.name, dep.spec
                    ),
                )
            } else if dep.spec.is_empty() {
                (false, String::new(), Confidence::Uncertain, String::new())
            } else {
                // No concrete version — does the declared range permit a vulnerable one?
                let hit = adv.specs.iter().any(|s| specs_intersect(&dep.spec, s));
                (
                    hit,
                    dep.spec.clone(),
                    Confidence::Uncertain,
                    format!(
                        "declared range `{} {}` permits a version affected by {alias}{summary}; pin or constrain above the fix",
                        dep.name, dep.spec
                    ),
                )
            };
            if !matched {
                continue;
            }
            if !seen.insert((dep.name.clone(), version_key.clone(), alias.clone())) {
                continue;
            }
            let rule = "vulnerable-dependency";
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[&dep.name, &version_key, &adv.id]),
                rule: rule.into(),
                category: Category::Security,
                severity: Severity::Warn,
                confidence,
                attribution: None,
                reason,
                location: Location {
                    path: dep.source.clone(),
                    line: dep.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "upgrade-dependency".into(),
                    description: format!(
                        "Constrain `{}` out of the affected range for {} ({alias}).",
                        dep.name, adv.id
                    ),
                    auto_fixable: false,
                    suppression_comment: Some("# mollify: ignore[vulnerable-dependency]".into()),
                }],
            });
        }
    }
    findings
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

/// Collect declared dependency *constraints* (ranges) from `requirements*.txt`
/// and `pyproject.toml` (PEP 621 `[project].dependencies` + Poetry).
pub fn collect_declared_ranges(root: &Utf8Path) -> Vec<DeclaredRange> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("requirements") && name.ends_with(".txt") {
            if let Ok(p) = camino::Utf8PathBuf::from_path_buf(entry.path()) {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    for (i, raw) in text.lines().enumerate() {
                        let line = raw.split('#').next().unwrap_or("").trim();
                        if line.is_empty() || line.starts_with('-') {
                            continue;
                        }
                        if let Some((name, spec)) = split_requirement(line) {
                            out.push(DeclaredRange {
                                name,
                                spec,
                                source: p.clone(),
                                line: i as u32 + 1,
                            });
                        }
                    }
                }
            }
        }
    }
    let pp = root.join("pyproject.toml");
    if pp.exists() {
        parse_pyproject_ranges(&pp, &mut out);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.spec.cmp(&b.spec)));
    out.dedup();
    out
}

/// Split a requirement string into `(normalized_name, pep440_spec)`. Markers
/// and extras are dropped; a bare dependency yields an empty spec.
fn split_requirement(line: &str) -> Option<(String, String)> {
    let line = line.split(';').next().unwrap_or("").trim();
    if line.is_empty() {
        return None;
    }
    match line.find(['<', '>', '=', '!', '~']) {
        Some(pos) => {
            let name = line[..pos].split('[').next().unwrap_or("").trim();
            let spec = line[pos..].trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some((normalize_dist(name), spec))
            }
        }
        None => {
            let name = line.split('[').next().unwrap_or("").trim();
            if name.is_empty() {
                None
            } else {
                Some((normalize_dist(name), String::new()))
            }
        }
    }
}

fn parse_pyproject_ranges(path: &Utf8Path, out: &mut Vec<DeclaredRange>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return;
    };
    // PEP 621: [project].dependencies = ["name>=1.0", ...]
    if let Some(deps) = table
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
    {
        for d in deps {
            if let Some(s) = d.as_str() {
                if let Some((name, spec)) = split_requirement(s) {
                    out.push(DeclaredRange {
                        name,
                        spec,
                        source: path.to_owned(),
                        line: 1,
                    });
                }
            }
        }
    }
    // Poetry: [tool.poetry.dependencies] name = "^1.2" / ">=1.0" / { version = ".." }
    if let Some(deps) = table
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        for (name, val) in deps {
            if name.eq_ignore_ascii_case("python") {
                continue;
            }
            let raw = val.as_str().map(|s| s.to_string()).or_else(|| {
                val.get("version")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });
            if let Some(raw) = raw {
                out.push(DeclaredRange {
                    name: normalize_dist(name),
                    spec: poetry_to_pep440(&raw),
                    source: path.to_owned(),
                    line: 1,
                });
            }
        }
    }
}

/// Convert a Poetry caret/tilde constraint to a PEP 440 range. Plain PEP 440
/// specs pass through; `*` / unrecognized → empty (any).
fn poetry_to_pep440(spec: &str) -> String {
    let s = spec.trim();
    if s == "*" || s.is_empty() {
        return String::new();
    }
    if let Some(rest) = s.strip_prefix('^') {
        // ^X.Y.Z → >=X.Y.Z,<next-significant. Leading zeros tighten the bound.
        let parts: Vec<u64> = rest
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect();
        if parts.is_empty() {
            return String::new();
        }
        let upper = caret_upper(&parts);
        return format!(">={rest},<{upper}");
    }
    if let Some(rest) = s.strip_prefix('~') {
        // ~X.Y → >=X.Y,<X.(Y+1); ~X → >=X,<(X+1)
        let parts: Vec<u64> = rest
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect();
        let upper = match parts.len() {
            0 => return String::new(),
            1 => format!("{}", parts[0] + 1),
            _ => format!("{}.{}", parts[0], parts[1] + 1),
        };
        return format!(">={rest},<{upper}");
    }
    // Already a PEP 440 specifier (or bare version).
    s.to_string()
}

/// Caret upper bound: bump the first non-zero component (Poetry/SemVer rule).
fn caret_upper(parts: &[u64]) -> String {
    for (i, &p) in parts.iter().enumerate() {
        if p != 0 {
            let mut bumped = parts[..=i].to_vec();
            bumped[i] += 1;
            for b in bumped.iter_mut().skip(i + 1) {
                *b = 0;
            }
            return bumped
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(".");
        }
    }
    // All zeros (`^0.0.0`) → next patch.
    let mut v = parts.to_vec();
    if let Some(last) = v.last_mut() {
        *last += 1;
    }
    v.iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(".")
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

    #[test]
    fn flags_declared_range_that_permits_vulnerable() {
        let ranges = vec![DeclaredRange {
            name: "jinja2".into(),
            spec: ">=2.0".into(),
            source: "requirements.txt".into(),
            line: 2,
        }];
        let advisories = vec![adv("PYSEC-1", "Jinja2", &["<2.11.3"])];
        let f = analyze_declared(&ranges, &advisories, None, &Default::default());
        assert_eq!(f.len(), 1, "got {f:?}");
        assert!(matches!(f[0].confidence, Confidence::Uncertain));
        assert!(f[0].reason.contains("permits"), "{}", f[0].reason);
    }

    #[test]
    fn declared_range_above_fix_is_clean() {
        let ranges = vec![DeclaredRange {
            name: "jinja2".into(),
            spec: ">=2.11.3".into(),
            source: "requirements.txt".into(),
            line: 2,
        }];
        let advisories = vec![adv("PYSEC-1", "Jinja2", &["<2.11.3"])];
        let f = analyze_declared(&ranges, &advisories, None, &Default::default());
        assert!(
            f.is_empty(),
            "range entirely above the fix should be clean: {f:?}"
        );
    }

    #[test]
    fn installed_version_resolves_range_precisely() {
        let mut versions = rustc_hash::FxHashMap::default();
        versions.insert("jinja2".to_string(), "2.4.1".to_string());
        let inst = Installed {
            versions,
            ..Default::default()
        };
        let ranges = vec![DeclaredRange {
            name: "jinja2".into(),
            spec: ">=2.0".into(),
            source: "pyproject.toml".into(),
            line: 1,
        }];
        let advisories = vec![adv("PYSEC-1", "Jinja2", &["<2.11.3"])];
        let f = analyze_declared(&ranges, &advisories, Some(&inst), &Default::default());
        assert_eq!(f.len(), 1, "got {f:?}");
        assert!(matches!(f[0].confidence, Confidence::Certain));
        assert!(f[0].reason.contains("2.4.1") && f[0].reason.contains("installed"));
    }

    #[test]
    fn collects_declared_ranges_from_requirements_and_pyproject() {
        let d = temp("ranges");
        std::fs::write(d.join("requirements.txt"), "requests>=2.0,<3\nflask\n").unwrap();
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\ndependencies = [\"urllib3>=1.0\"]\n[tool.poetry.dependencies]\npython = \"^3.9\"\nclick = \"^8.1\"\n",
        )
        .unwrap();
        let r = collect_declared_ranges(&d);
        assert!(r
            .iter()
            .any(|x| x.name == "requests" && x.spec == ">=2.0,<3"));
        assert!(r.iter().any(|x| x.name == "flask" && x.spec.is_empty()));
        assert!(r.iter().any(|x| x.name == "urllib3" && x.spec == ">=1.0"));
        // Poetry caret converted to a PEP 440 range; `python` excluded.
        assert!(r.iter().any(|x| x.name == "click" && x.spec == ">=8.1,<9"));
        assert!(!r.iter().any(|x| x.name == "python"));
        std::fs::remove_dir_all(&d).ok();
    }
}
