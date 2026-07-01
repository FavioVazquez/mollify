//! Safe auto-fix: removes only `confidence: certain`, `auto_fixable` unused
//! symbols and unused imports (never files, never lower-confidence findings).
//! Dry-run by default at the CLI; this module computes a plan and can apply it.

use crate::dead_code_report;
use camino::{Utf8Path, Utf8PathBuf};
use mollify_types::Confidence;
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct FixEdit {
    pub path: Utf8PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub description: String,
}

/// Compute the set of safe edits (deleting unused-symbol line ranges).
pub fn plan(root: &Utf8Path) -> Vec<FixEdit> {
    let report = dead_code_report(root);
    let mut edits: Vec<FixEdit> = report
        .findings
        .into_iter()
        .filter(|f| {
            (f.rule == "unused-export" || f.rule == "unused-import")
                && f.confidence == Confidence::Certain
                && f.actions.first().is_some_and(|a| a.auto_fixable)
                // Defense in depth: only plain .py files can be line-edited
                // safely (notebook findings carry cell-relative lines).
                && f.location.path.extension() == Some("py")
        })
        .map(|f| FixEdit {
            start_line: f.location.line,
            end_line: f.location.end_line.unwrap_or(f.location.line),
            path: f.location.path,
            description: f
                .actions
                .into_iter()
                .next()
                .map(|a| a.description)
                .unwrap_or_default(),
        })
        .collect();
    edits.sort_by(|a, b| a.path.cmp(&b.path).then(a.start_line.cmp(&b.start_line)));
    edits
}

/// Apply edits in place. Deletes the inclusive line ranges, bottom-up per file
/// so earlier line numbers stay valid. Returns the number of edits applied.
pub fn apply(edits: &[FixEdit]) -> std::io::Result<usize> {
    let mut by_file: FxHashMap<&Utf8Path, Vec<&FixEdit>> = FxHashMap::default();
    for e in edits {
        by_file.entry(e.path.as_path()).or_default().push(e);
    }
    let mut applied = 0;
    for (path, mut file_edits) in by_file {
        // Bottom-up; skip overlaps defensively.
        file_edits.sort_by_key(|e| std::cmp::Reverse(e.start_line));
        let content = std::fs::read_to_string(path)?;
        let mut lines: Vec<&str> = content.lines().collect();
        let mut last_removed_start = u32::MAX;
        for e in file_edits {
            let start = e.start_line.saturating_sub(1) as usize;
            let end = (e.end_line as usize).min(lines.len());
            if start >= lines.len() || e.end_line >= last_removed_start {
                continue; // out of range or overlapping a prior removal
            }
            lines.drain(start..end);
            last_removed_start = e.start_line;
            applied += 1;
        }
        let mut out = lines.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        std::fs::write(path, out)?;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-fix-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn plan_targets_only_certain_unused() {
        let d = temp("plan");
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        // _priv is private+unused => certain+autofixable; pub is likely (not in plan).
        std::fs::write(
            d.join("lib.py"),
            "def _priv():\n    return 1\n\ndef pub():\n    return 2\n",
        )
        .unwrap();
        let edits = plan(&d);
        assert_eq!(edits.len(), 1, "got {edits:?}");
        assert!(edits[0].path.as_str().ends_with("lib.py"));
        assert_eq!(edits[0].start_line, 1);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn plan_never_touches_notebooks() {
        // Notebook finding lines are relative to the concatenated code cells;
        // editing the raw JSON by those line numbers destroys the file.
        let d = temp("notebook");
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        let nb = r#"{
 "cells": [
  {
   "cell_type": "code",
   "metadata": {},
   "source": ["import os\n", "x = 1\n", "print(x)\n"],
   "outputs": [],
   "execution_count": null
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}"#;
        std::fs::write(d.join("analysis.ipynb"), nb).unwrap();
        let report = dead_code_report(&d);
        // The unused-import finding still exists (evidence is preserved)…
        let nb_finding = report
            .findings
            .iter()
            .find(|f| f.rule == "unused-import" && f.location.path.extension() == Some("ipynb"))
            .expect("notebook unused-import finding");
        // …but it is never auto-fixable.
        assert!(!nb_finding.actions[0].auto_fixable);
        let edits = plan(&d);
        assert!(
            edits.iter().all(|e| e.path.extension() == Some("py")),
            "plan contains a non-.py edit: {edits:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn cross_module_dynamic_dispatch_blocks_auto_fix() {
        // `getattr(lib, "_handler_" + name)()` in another module means the
        // "unused" private symbol may be live — never Certain, never planned.
        let d = temp("dynsink");
        std::fs::write(
            d.join("__main__.py"),
            "import lib\nname = 'a'\nfn = getattr(lib, '_handler_' + name)\nfn()\n",
        )
        .unwrap();
        std::fs::write(d.join("lib.py"), "def _handler_a():\n    return 'a'\n").unwrap();
        let report = dead_code_report(&d);
        for f in report.findings.iter().filter(|f| f.rule == "unused-export") {
            assert_ne!(
                f.confidence,
                Confidence::Certain,
                "dynamic-dispatch project must not grade unused-export Certain: {f:?}"
            );
        }
        assert!(plan(&d).is_empty(), "fix plan must be empty");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn apply_removes_the_symbol() {
        let d = temp("apply");
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        let lib = d.join("lib.py");
        std::fs::write(
            &lib,
            "def _priv():\n    return 1\n\ndef keep():\n    return 2\n",
        )
        .unwrap();
        let edits = plan(&d);
        let n = apply(&edits).unwrap();
        assert_eq!(n, 1);
        let after = std::fs::read_to_string(&lib).unwrap();
        assert!(!after.contains("_priv"), "after: {after:?}");
        assert!(after.contains("keep"));
        std::fs::remove_dir_all(&d).ok();
    }
}
