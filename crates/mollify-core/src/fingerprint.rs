//! Stable, deterministic finding fingerprints: `<rule>:<8 hex>`.

use xxhash_rust::xxh3::xxh3_64;

/// Build a fingerprint from the rule id and stable identity components
/// (e.g. path + symbol name). Independent of run order and minor edits.
pub fn fingerprint(rule: &str, parts: &[&str]) -> String {
    let joined = parts.join("\u{1f}");
    let h = xxh3_64(joined.as_bytes());
    format!("{rule}:{:08x}", (h & 0xffff_ffff) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_and_distinct() {
        let a = fingerprint("unused-export", &["lib.py", "foo"]);
        let b = fingerprint("unused-export", &["lib.py", "foo"]);
        let c = fingerprint("unused-export", &["lib.py", "bar"]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("unused-export:"));
    }
}
