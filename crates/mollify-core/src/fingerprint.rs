//! Stable, deterministic finding fingerprints: `<rule>:<16 hex>`.
//!
//! Identity components MUST be stable across machines and unrelated edits:
//! use the module's **root-relative** path (`ModuleInfo::rel`), symbol names
//! or line *content* — never the invocation-spelled path and never bare line
//! numbers, which shift under any edit above the finding. Duplicate
//! identities within a file are disambiguated with an occurrence index.

use xxhash_rust::xxh3::xxh3_64;

/// Build a fingerprint from the rule id and stable identity components
/// (e.g. relative path + symbol name). Independent of run order, checkout
/// location, root spelling, and edits elsewhere in the file.
pub fn fingerprint(rule: &str, parts: &[&str]) -> String {
    let joined = parts.join("\u{1f}");
    let h = xxh3_64(joined.as_bytes());
    format!("{rule}:{h:016x}")
}

/// Occurrence counter for disambiguating findings that share every other
/// identity component (e.g. two defs of the same name in one file). Returns
/// "0", "1", … in source order, which is deterministic because engines
/// iterate findings in line order.
#[derive(Default)]
pub struct Occurrences(rustc_hash::FxHashMap<String, u32>);

impl Occurrences {
    pub fn next(&mut self, key: &str) -> String {
        let n = self.0.entry(key.to_string()).or_insert(0);
        let s = n.to_string();
        *n += 1;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn stable_and_distinct() {
        let a = fingerprint("unused-export", &["lib.py", "foo"]);
        let b = fingerprint("unused-export", &["lib.py", "foo"]);
        let c = fingerprint("unused-export", &["lib.py", "bar"]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("unused-export:"));
    }

    #[test]
    fn occurrences_count_per_key_in_order() {
        let mut occ = Occurrences::default();
        assert_eq!(occ.next("a"), "0");
        assert_eq!(occ.next("b"), "0");
        assert_eq!(occ.next("a"), "1");
        assert_eq!(occ.next("a"), "2");
    }

    fn write_project(dir: &Utf8PathBuf) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("__main__.py"), "from lib import used\nused()\n").unwrap();
        std::fs::write(
            dir.join("lib.py"),
            "import os\n\ndef used():\n    return 1\n\ndef helper():\n    return 2\n",
        )
        .unwrap();
    }

    fn fingerprints_of(dir: &Utf8PathBuf) -> Vec<String> {
        let mut v: Vec<String> = crate::dead_code_report(dir)
            .findings
            .into_iter()
            .map(|f| f.fingerprint)
            .collect();
        v.sort();
        v
    }

    /// Baselines must transfer across machines and checkouts: byte-identical
    /// projects at two different roots produce identical fingerprints.
    #[test]
    fn fingerprints_are_checkout_location_independent() {
        let base = std::env::temp_dir().join(format!("mollify-fp-roots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let a = Utf8PathBuf::from_path_buf(base.join("checkout-a")).unwrap();
        let b = Utf8PathBuf::from_path_buf(base.join("some/deeper/checkout-b")).unwrap();
        write_project(&a);
        write_project(&b);
        let fa = fingerprints_of(&a);
        assert!(!fa.is_empty());
        assert_eq!(fa, fingerprints_of(&b));
        std::fs::remove_dir_all(&base).ok();
    }

    /// Fingerprints must survive edits elsewhere in the file: inserting a
    /// comment line above a finding must not churn its identity.
    #[test]
    fn fingerprints_survive_unrelated_line_shifts() {
        let base = std::env::temp_dir().join(format!("mollify-fp-shift-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let d = Utf8PathBuf::from_path_buf(base.clone()).unwrap();
        write_project(&d);
        let before = fingerprints_of(&d);
        let shifted = format!(
            "# a new leading comment\n{}",
            std::fs::read_to_string(d.join("lib.py")).unwrap()
        );
        std::fs::write(d.join("lib.py"), shifted).unwrap();
        assert_eq!(before, fingerprints_of(&d));
        std::fs::remove_dir_all(&base).ok();
    }

    /// Two defs of the same name must not share a fingerprint (occurrence
    /// disambiguation) — fingerprint-keyed consumers would merge them.
    #[test]
    fn duplicate_names_get_distinct_fingerprints() {
        let base = std::env::temp_dir().join(format!("mollify-fp-dup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let d = Utf8PathBuf::from_path_buf(base.clone()).unwrap();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        std::fs::write(
            d.join("lib.py"),
            "def dead():\n    return 1\n\ndef dead():\n    return 2\n",
        )
        .unwrap();
        let report = crate::dead_code_report(&d);
        let fps: Vec<&str> = report
            .findings
            .iter()
            .filter(|f| f.rule == "unused-export")
            .map(|f| f.fingerprint.as_str())
            .collect();
        assert_eq!(fps.len(), 2, "expected two unused-export findings");
        assert_ne!(fps[0], fps[1], "duplicate defs share a fingerprint");
        std::fs::remove_dir_all(&base).ok();
    }
}
