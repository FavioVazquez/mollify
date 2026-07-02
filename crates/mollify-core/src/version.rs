//! A pragmatic **PEP 440 subset** for matching package versions against
//! advisory constraint ranges. It handles epochs (`2!1.0`), release segments
//! (`1.2.3`), pre-releases (`a`/`b`/`rc`), post-releases (`.post1`), dev
//! releases (`.dev1`), and the operators `== != < <= > >= ~=`. Local versions
//! (`+cpu`) are ignored for comparison; `===` is out of scope (degrades to
//! "no match" rather than guessing).

use std::cmp::Ordering;

/// A parsed version. PEP 440 ordering within one release:
/// `.devN` < pre-release < final < `.postN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    epoch: u64,
    release: Vec<u64>,
    /// `None` for a final release; `Some((rank, n))` for a pre-release where
    /// rank orders a<b<rc (0,1,2).
    pre: Option<(u8, u64)>,
    post: Option<u64>,
    dev: Option<u64>,
}

impl Version {
    /// Parse a version string. Returns `None` if no leading release segment.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        // Split off a local version (`+...`) — ignored for comparison.
        let s = s.split('+').next().unwrap_or(s);
        // Epoch: `N!` prefix.
        let (epoch, s) = match s.split_once('!') {
            Some((e, rest)) => (e.parse::<u64>().ok()?, rest),
            None => (0, s),
        };
        // Find where the release segment ends (first non-digit, non-dot char).
        let end = s
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(s.len());
        let (rel_str, rest) = s.split_at(end);
        // A trailing dot belongs to the suffix (`1.0.post1` → release "1.0").
        let rel_str = rel_str.trim_end_matches('.');
        let release: Vec<u64> = rel_str
            .split('.')
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        if release.is_empty() {
            return None;
        }
        let (pre, post, dev) = parse_suffixes(rest);
        Some(Version {
            epoch,
            release,
            pre,
            post,
            dev,
        })
    }

    /// Phase key implementing `.devN` < pre < final < `.postN` within one
    /// release. (Nested combos like `1.0rc1.post2` order by their outermost
    /// phase, which is enough for advisory ranges.)
    fn phase_key(&self) -> (u8, u8, u64, u64) {
        match (self.pre, self.post, self.dev) {
            (None, None, Some(d)) => (0, 0, d, 0),
            (Some((rank, n)), _, d) => (1, rank, n, d.unwrap_or(u64::MAX)),
            (None, None, None) => (2, 0, 0, 0),
            (None, Some(p), d) => (3, 0, p, d.unwrap_or(u64::MAX)),
        }
    }

    /// Compare two versions per PEP 440 ordering
    /// (epoch, then release, then phase).
    pub fn cmp_to(&self, other: &Version) -> Ordering {
        match self.epoch.cmp(&other.epoch) {
            Ordering::Equal => {}
            ord => return ord,
        }
        let max = self.release.len().max(other.release.len());
        for i in 0..max {
            let a = self.release.get(i).copied().unwrap_or(0);
            let b = other.release.get(i).copied().unwrap_or(0);
            match a.cmp(&b) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        self.phase_key().cmp(&other.phase_key())
    }
}

/// Parse the suffix after the release segment: pre-release (`rc1`, `-beta.1`),
/// post-release (`.post1`, `-r2`, `rev3`), and dev release (`.dev1`), in any
/// of PEP 440's spellings.
fn parse_suffixes(rest: &str) -> (Option<(u8, u64)>, Option<u64>, Option<u64>) {
    let lower = rest.to_ascii_lowercase();
    let mut r: &str = &lower;
    let mut pre = None;
    let mut post = None;
    let mut dev = None;
    loop {
        r = r.trim_start_matches(['.', '-', '_']);
        if r.is_empty() {
            break;
        }
        // Longest-prefix first: `rc` before `r`/`c`, `post` before `p`.
        let table: &[(&str, u8)] = &[
            ("alpha", 0),
            ("beta", 1),
            ("rc", 2),
            ("preview", 2),
            ("pre", 2),
            ("post", 10),
            ("rev", 10),
            ("dev", 20),
            ("a", 0),
            ("b", 1),
            ("c", 2),
            ("r", 10),
        ];
        let Some(&(tag, kind)) = table.iter().find(|(t, _)| r.starts_with(t)) else {
            break; // unknown suffix: ignore the tail
        };
        let tail = &r[tag.len()..];
        let digits: String = tail
            .trim_start_matches(['.', '-', '_'])
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let n: u64 = digits.parse().unwrap_or(0);
        let consumed = tag.len()
            + (tail.len() - tail.trim_start_matches(['.', '-', '_']).len())
            + digits.len();
        match kind {
            10 => post = post.or(Some(n)),
            20 => dev = dev.or(Some(n)),
            rank => pre = pre.or(Some((rank, n))),
        }
        r = &r[consumed..];
    }
    (pre, post, dev)
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
/// Finite candidate sweep (a *heuristic*, biased toward false intersections
/// never being missed at real-world granularity): we test each named
/// boundary, points just above each boundary at two depths, and a point
/// below all of them. A gap narrower than the probed granularity could in
/// principle be missed; advisory ranges use release-segment boundaries, where
/// the sweep is exact.
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
        // Even closer above: lands inside narrow gaps like (>2.0, <2.0.1),
        // where `{bnd}.1` collides with the other spec's own boundary.
        candidates.push(format!("{bnd}.0.1"));
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
    fn epochs_compare_first() {
        // `2!1.0` is a *later* version line than any epoch-0 release.
        assert_eq!(
            Version::parse("2!1.0")
                .unwrap()
                .cmp_to(&Version::parse("3.0").unwrap()),
            Ordering::Greater
        );
        assert!(!matches_spec("2!1.0", "<3.0"));
        assert!(matches_spec("2!1.0", ">=1!0"));
    }

    #[test]
    fn post_and_dev_releases_are_ordered_per_pep440() {
        // dev < pre < final < post within one release.
        let dev = Version::parse("1.0.dev1").unwrap();
        let pre = Version::parse("1.0a1").unwrap();
        let fin = Version::parse("1.0").unwrap();
        let post = Version::parse("1.0.post1").unwrap();
        assert_eq!(dev.cmp_to(&pre), Ordering::Less);
        assert_eq!(pre.cmp_to(&fin), Ordering::Less);
        assert_eq!(fin.cmp_to(&post), Ordering::Less);
        // `==1.0` matches neither the post nor the dev release.
        assert!(!matches_spec("1.0.post1", "==1.0"));
        assert!(!matches_spec("1.0.dev1", "==1.0"));
        // `<`-bounded advisory ranges see dev releases below the bound.
        assert!(matches_spec("1.0.dev1", "<1.0"));
        assert!(!matches_spec("1.0.post1", "<1.0"));
    }

    #[test]
    fn narrow_gaps_between_specs_are_found() {
        // The only versions satisfying both live strictly between the two
        // boundaries (e.g. 2.0.0.1) — the sweep must land inside the gap.
        assert!(specs_intersect(">2.0", "<2.0.1"));
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
