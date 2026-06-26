//! A pragmatic **PEP 440 subset** for matching package versions against
//! advisory constraint ranges. Not a full PEP 440 implementation: it handles
//! release segments (`1.2.3`), an optional pre-release tag (`a`/`b`/`rc`), and
//! the operators `== != < <= > >= ~=`. Epochs, local versions, and `===` are
//! out of scope (documented; we degrade to "no match" rather than guess).

use std::cmp::Ordering;

/// A parsed version: release components plus an optional pre-release rank.
/// Pre-releases sort *before* the same release (`1.0rc1` < `1.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    release: Vec<u64>,
    /// `None` for a final release; `Some((rank, n))` for a pre-release where
    /// rank orders a<b<rc (0,1,2). Final releases sort after all pre-releases.
    pre: Option<(u8, u64)>,
}

impl Version {
    /// Parse a version string. Returns `None` if no leading release segment.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        // Split off a local version (`+...`) — ignored for comparison.
        let s = s.split('+').next().unwrap_or(s);
        // Find where the release segment ends (first non-digit, non-dot char).
        let end = s
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(s.len());
        let (rel_str, rest) = s.split_at(end);
        let release: Vec<u64> = rel_str
            .split('.')
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        if release.is_empty() {
            return None;
        }
        let pre = parse_pre(rest);
        Some(Version { release, pre })
    }

    /// Compare two versions per PEP 440 ordering (release then pre-release).
    pub fn cmp_to(&self, other: &Version) -> Ordering {
        let max = self.release.len().max(other.release.len());
        for i in 0..max {
            let a = self.release.get(i).copied().unwrap_or(0);
            let b = other.release.get(i).copied().unwrap_or(0);
            match a.cmp(&b) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        // Equal release: a pre-release is less than a final release.
        match (self.pre, other.pre) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(&b),
        }
    }
}

/// Parse a pre-release suffix like `rc1`, `b2`, `a`, `.rc1`, `-beta.1`.
fn parse_pre(rest: &str) -> Option<(u8, u64)> {
    let r = rest
        .trim_start_matches(['.', '-', '_'])
        .to_ascii_lowercase();
    let (rank, tail) = if let Some(t) = r.strip_prefix("alpha") {
        (0u8, t)
    } else if let Some(t) = r.strip_prefix('a') {
        (0, t)
    } else if let Some(t) = r.strip_prefix("beta") {
        (1, t)
    } else if let Some(t) = r.strip_prefix('b') {
        (1, t)
    } else if let Some(t) = r.strip_prefix("rc") {
        (2, t)
    } else if let Some(t) = r.strip_prefix("c") {
        (2, t)
    } else {
        return None;
    };
    let n: u64 = tail
        .trim_start_matches(['.', '-', '_'])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    Some((rank, n))
}

/// Does `version` satisfy a single constraint like `>=1.2`, `<2.0`, `==1.0.*`,
/// `~=1.4`, `!=1.5`? Unknown operators / unparseable bounds → `false`.
fn satisfies_one(version: &Version, constraint: &str) -> bool {
    let c = constraint.trim();
    if c.is_empty() {
        return true;
    }
    let (op, rhs) = split_op(c);
    // Wildcard handling for == / != (e.g. `==1.4.*`).
    if (op == "==" || op == "!=") && rhs.ends_with(".*") {
        let prefix = rhs.trim_end_matches(".*");
        let Some(pv) = Version::parse(prefix) else {
            return false;
        };
        let matches_prefix = version.release.len() >= pv.release.len()
            && version.release[..pv.release.len()] == pv.release[..];
        return if op == "==" {
            matches_prefix
        } else {
            !matches_prefix
        };
    }
    let Some(bound) = Version::parse(rhs) else {
        return false;
    };
    let ord = version.cmp_to(&bound);
    match op {
        "==" => ord == Ordering::Equal,
        "!=" => ord != Ordering::Equal,
        "<" => ord == Ordering::Less,
        "<=" => ord != Ordering::Greater,
        ">" => ord == Ordering::Greater,
        ">=" => ord != Ordering::Less,
        "~=" => compatible_release(version, &bound),
        _ => false,
    }
}

/// `~=X.Y` means `>=X.Y, ==X.*`; `~=X.Y.Z` means `>=X.Y.Z, ==X.Y.*`.
fn compatible_release(version: &Version, bound: &Version) -> bool {
    if version.cmp_to(bound) == Ordering::Less {
        return false;
    }
    if bound.release.len() < 2 {
        return true; // `~=1` is invalid PEP 440; be permissive.
    }
    let keep = bound.release.len() - 1;
    version.release.len() >= keep && version.release[..keep] == bound.release[..keep]
}

fn split_op(c: &str) -> (&str, &str) {
    for op in ["==", "!=", "<=", ">=", "~=", "<", ">"] {
        if let Some(rest) = c.strip_prefix(op) {
            return (op, rest.trim());
        }
    }
    // Bare version = exact match.
    ("==", c)
}

/// Do two PEP 440 specifier sets have a **non-empty intersection** — i.e. does
/// any version satisfy both `a` and `b`? Used to decide whether a declared
/// *range* (e.g. `>=2.0`) permits a version that an advisory marks vulnerable
/// (e.g. `<2.11.3`), without needing a concrete pin.
///
/// Sound finite sweep: every constraint's truth value only changes at one of
/// the boundary versions named in `a`/`b`. We test each boundary, a point just
/// above each boundary, and a point below all of them; if any candidate
/// satisfies both specifier sets, they intersect.
pub fn specs_intersect(a: &str, b: &str) -> bool {
    let mut bounds: Vec<String> = Vec::new();
    for spec in [a, b] {
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (_, rhs) = split_op(part);
            let rhs = rhs.trim_end_matches(".*").trim();
            if Version::parse(rhs).is_some() {
                bounds.push(rhs.to_string());
            }
        }
    }
    let mut candidates: Vec<String> = vec!["0".to_string()];
    for bnd in &bounds {
        candidates.push(bnd.clone());
        candidates.push(format!("{bnd}.1")); // strictly just above this boundary
        if let Some(inc) = incr_last(bnd) {
            candidates.push(inc);
        }
    }
    candidates
        .iter()
        .any(|c| matches_spec(c, a) && matches_spec(c, b))
}

/// Increment the last release component of a version string (`1.4` -> `1.5`).
fn incr_last(v: &str) -> Option<String> {
    let parsed = Version::parse(v)?;
    let mut rel = parsed.release;
    let last = rel.last_mut()?;
    *last += 1;
    Some(
        rel.iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// Does `version` satisfy a comma-separated AND of constraints
/// (e.g. `>=1.0,<2.0`)? An empty spec matches everything.
pub fn matches_spec(version: &str, spec: &str) -> bool {
    let Some(v) = Version::parse(version) else {
        return false;
    };
    spec.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .all(|p| satisfies_one(&v, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_releases_and_prereleases() {
        assert_eq!(
            Version::parse("1.2.0")
                .unwrap()
                .cmp_to(&Version::parse("1.10.0").unwrap()),
            Ordering::Less
        );
        // pre-release sorts before final
        assert_eq!(
            Version::parse("1.0rc1")
                .unwrap()
                .cmp_to(&Version::parse("1.0").unwrap()),
            Ordering::Less
        );
        assert_eq!(
            Version::parse("1.0a1")
                .unwrap()
                .cmp_to(&Version::parse("1.0b1").unwrap()),
            Ordering::Less
        );
    }

    #[test]
    fn matches_ranges() {
        assert!(matches_spec("2.4.1", "<2.11.3"));
        assert!(!matches_spec("2.11.3", "<2.11.3"));
        assert!(matches_spec("1.5", ">=1.0,<2.0"));
        assert!(!matches_spec("2.0", ">=1.0,<2.0"));
        assert!(matches_spec("1.4.7", "==1.4.*"));
        assert!(!matches_spec("1.5.0", "==1.4.*"));
        assert!(matches_spec("1.4.9", "~=1.4.2"));
        assert!(!matches_spec("1.5.0", "~=1.4.2"));
        assert!(matches_spec("3.1.2", ">=3.1.0"));
    }

    #[test]
    fn unparseable_is_no_match() {
        assert!(!matches_spec("not-a-version", "<2.0"));
        assert!(!matches_spec("1.0", "≤2.0")); // unknown operator
    }

    #[test]
    fn specifier_set_intersection() {
        // A declared range that permits a vulnerable version intersects.
        assert!(specs_intersect(">=2.0", "<2.11.3"));
        assert!(specs_intersect(">=1.0,<3.0", ">=2.0,<2.5"));
        assert!(specs_intersect("", "<2.0")); // empty (any) intersects anything satisfiable
                                              // A declared range entirely above the vulnerable range does NOT intersect.
        assert!(!specs_intersect(">=2.11.3", "<2.11.3"));
        assert!(!specs_intersect(">=3.0", "<2.0"));
        assert!(!specs_intersect(">=1.0,<2.0", ">=2.0"));
        // Wildcards and compatible-release.
        assert!(specs_intersect("~=1.4", "==1.4.7"));
        assert!(!specs_intersect("~=1.4", "==2.0.0"));
        assert!(specs_intersect(">=1.0", "==1.5.*"));
    }
}
