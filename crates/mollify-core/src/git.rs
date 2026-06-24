//! Minimal git integration for the PR gate. Computes the set of changed files
//! (working tree + staged + optionally vs a base ref) so findings can be
//! attributed introduced-vs-inherited.
//!
//! Simplification vs fallow (documented in STATUS.md): attribution is
//! **file-level** — a finding in a changed file is "introduced". fallow's
//! base-worktree snapshot gives line-level introduced-vs-inherited; that's a
//! planned upgrade.

use camino::Utf8Path;
use rustc_hash::FxHashSet;
use std::process::Command;

/// Return the set of changed file paths (relative to `root`), or `None` if this
/// isn't a git repo / git is unavailable. Includes unstaged, staged, untracked,
/// and (if `base` is given) everything changed since the merge-base with `base`.
pub fn changed_files(root: &Utf8Path, base: Option<&str>) -> Option<FxHashSet<String>> {
    // Quick check: is this a git work tree?
    let ok = Command::new("git")
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

/// Whether a finding path (possibly absolute or `./`-prefixed) is in the changed
/// set (which holds paths relative to `root`).
pub fn path_is_changed(root: &Utf8Path, finding_path: &Utf8Path, changed: &FxHashSet<String>) -> bool {
    let rel = finding_path
        .strip_prefix(root)
        .unwrap_or(finding_path)
        .as_str()
        .trim_start_matches("./");
    if changed.contains(rel) {
        return true;
    }
    // Fallback: match by file name (handles path-normalization edge cases).
    if let Some(name) = finding_path.file_name() {
        return changed.iter().any(|c| c.ends_with(name));
    }
    false
}
