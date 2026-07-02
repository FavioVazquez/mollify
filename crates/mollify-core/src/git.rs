//! Git integration for the PR gate. Computes changed files (working tree +
//! staged + optionally vs a base ref) and **changed line ranges** so findings
//! can be attributed introduced-vs-inherited at line granularity (parsed from
//! `git diff --unified=0`), with file-level as the fallback.

use camino::Utf8Path;
use rustc_hash::FxHashSet;
use std::process::Command;

/// Return the set of changed file paths (relative to `root`), or `None` if this
/// isn't a git repo / git is unavailable. Includes unstaged, staged, untracked,
/// and (if `base` is given) everything changed since the merge-base with `base`.
pub fn changed_files(root: &Utf8Path, base: Option<&str>) -> Option<FxHashSet<String>> {
    // Quick check: is this a git work tree?
    let ok = Command::new("git")
        .args(["-c", "core.quotepath=off"])
        .arg("-C")
        .arg(root.as_str())
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()?;
    if !ok.status.success() {
        return None;
    }

    let mut set = FxHashSet::default();
    let mut add = |args: &[&str]| {
        if let Ok(out) = Command::new("git")
            .args(["-c", "core.quotepath=off"])
            .arg("-C")
            .arg(root.as_str())
            .args(args)
            .output()
        {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let l = line.trim();
                    if !l.is_empty() {
                        set.insert(l.to_string());
                    }
                }
            }
        }
    };

    add(&["diff", "--name-only"]); // unstaged
    add(&["diff", "--name-only", "--cached"]); // staged
    add(&["ls-files", "--others", "--exclude-standard"]); // untracked
    if let Some(base) = base {
        let range = format!("{base}...HEAD");
        add(&["diff", "--name-only", &range]);
    }
    Some(set)
}

/// Added/modified line ranges per file (relative paths) from `git diff
/// --unified=0`, combining unstaged + staged + (if `base`) the base range, plus
/// whole-file ranges for untracked files. `None` if not a git repo. Enables
/// **line-level** introduced-vs-inherited attribution.
pub fn changed_lines(
    root: &Utf8Path,
    base: Option<&str>,
) -> Option<rustc_hash::FxHashMap<String, Vec<(u32, u32)>>> {
    let ok = Command::new("git")
        .args(["-c", "core.quotepath=off"])
        .arg("-C")
        .arg(root.as_str())
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()?;
    if !ok.status.success() {
        return None;
    }
    let mut map: rustc_hash::FxHashMap<String, Vec<(u32, u32)>> = rustc_hash::FxHashMap::default();
    let mut add_diff = |args: &[&str]| {
        if let Ok(out) = Command::new("git")
            .args(["-c", "core.quotepath=off"])
            .arg("-C")
            .arg(root.as_str())
            .args(args)
            .output()
        {
            if out.status.success() {
                parse_unified0(&String::from_utf8_lossy(&out.stdout), &mut map);
            }
        }
    };
    add_diff(&["diff", "--unified=0"]);
    add_diff(&["diff", "--unified=0", "--cached"]);
    if let Some(base) = base {
        let range = format!("{base}...HEAD");
        add_diff(&["diff", "--unified=0", &range]);
    }
    // Untracked files: the whole file is "introduced".
    if let Ok(out) = Command::new("git")
        .args(["-c", "core.quotepath=off"])
        .arg("-C")
        .arg(root.as_str())
        .args(["ls-files", "--others", "--exclude-standard"])
        .output()
    {
        if out.status.success() {
            for f in String::from_utf8_lossy(&out.stdout).lines() {
                let f = f.trim();
                if !f.is_empty() {
                    map.entry(f.to_string()).or_default().push((1, u32::MAX));
                }
            }
        }
    }
    Some(map)
}

/// Parse `git diff --unified=0` output, recording added-line ranges per `+++`
/// file from each `@@ … +start[,len] @@` hunk header.
fn parse_unified0(diff: &str, map: &mut rustc_hash::FxHashMap<String, Vec<(u32, u32)>>) {
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            // `+++ b/path` (or `+++ /dev/null` for deletions).
            current = rest
                .strip_prefix("b/")
                .or(Some(rest))
                .filter(|p| *p != "/dev/null")
                .map(|p| p.to_string());
        } else if line.starts_with("@@ ") {
            // @@ -a,b +c,d @@
            if let Some(plus) = line.split('+').nth(1) {
                let spec = plus.split([' ', '@']).next().unwrap_or("");
                let mut it = spec.split(',');
                let start: u32 = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                let len: u32 = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(1);
                if start > 0 && len > 0 {
                    if let Some(f) = &current {
                        map.entry(f.clone())
                            .or_default()
                            .push((start, start + len - 1));
                    }
                }
            }
        }
    }
}

/// Whether `line` of `finding_path` falls in a changed range from [`changed_lines`].
pub fn line_is_changed(
    root: &Utf8Path,
    finding_path: &Utf8Path,
    line: u32,
    changed: &rustc_hash::FxHashMap<String, Vec<(u32, u32)>>,
) -> Option<bool> {
    let rel = finding_path
        .strip_prefix(root)
        .unwrap_or(finding_path)
        .as_str()
        .trim_start_matches("./");
    let ranges = changed.get(rel).or_else(|| {
        // Fallback by file name, anchored at a path-separator boundary so
        // `app.py` never inherits `myapp.py`'s hunks; smallest key wins for
        // determinism.
        finding_path.file_name().and_then(|name| {
            let suffix = format!("/{name}");
            changed
                .iter()
                .filter(|(k, _)| k.as_str() == name || k.ends_with(&suffix))
                .min_by(|a, b| a.0.cmp(b.0))
                .map(|(_, v)| v)
        })
    })?;
    Some(ranges.iter().any(|&(s, e)| line >= s && line <= e))
}

/// Per-file churn = number of commits that touched each file (relative paths).
/// `None` if not a git repo. Used for churn×complexity hotspot ranking.
pub fn file_churn(root: &Utf8Path) -> Option<rustc_hash::FxHashMap<String, u32>> {
    let out = Command::new("git")
        .args(["-c", "core.quotepath=off"])
        .arg("-C")
        .arg(root.as_str())
        .args(["log", "--no-merges", "--pretty=format:", "--name-only"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut counts: rustc_hash::FxHashMap<String, u32> = rustc_hash::FxHashMap::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let l = line.trim();
        if !l.is_empty() {
            *counts.entry(l.to_string()).or_insert(0) += 1;
        }
    }
    Some(counts)
}

/// Whether a finding path (possibly absolute or `./`-prefixed) is in the changed
/// set (which holds paths relative to `root`).
pub fn path_is_changed(
    root: &Utf8Path,
    finding_path: &Utf8Path,
    changed: &FxHashSet<String>,
) -> bool {
    let rel = finding_path
        .strip_prefix(root)
        .unwrap_or(finding_path)
        .as_str()
        .trim_start_matches("./");
    if changed.contains(rel) {
        return true;
    }
    // Fallback: match by file name at a path-separator boundary (handles
    // path-normalization edge cases without letting `app.py` match
    // `myapp.py`).
    if let Some(name) = finding_path.file_name() {
        let suffix = format!("/{name}");
        return changed.iter().any(|c| c.as_str() == name || c.ends_with(&suffix));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unified0_hunks() {
        let diff = "\
diff --git a/app.py b/app.py
--- a/app.py
+++ b/app.py
@@ -10,0 +11,3 @@ def f():
+x = 1
+y = 2
+z = 3
@@ -20 +24 @@
-old
+new
";
        let mut map = rustc_hash::FxHashMap::default();
        parse_unified0(diff, &mut map);
        let ranges = map.get("app.py").unwrap();
        assert!(ranges.contains(&(11, 13)), "got {ranges:?}");
        assert!(ranges.contains(&(24, 24)), "got {ranges:?}");
    }
}
