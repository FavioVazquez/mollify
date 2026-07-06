//! Code-metrics engine (radon / wily parity): raw size counts, per-function
//! complexity rollups, and the **Maintainability Index**. Unlike the other
//! engines this emits *measurements*, not findings — a `MetricsReport`.
//!
//! MI uses radon's normalized formula:
//! `MI = max(0, (171 - 5.2*ln(V) - 0.23*CC - 16.2*ln(SLOC)) * 100/171)`
//! where V = Halstead volume, CC = summed cyclomatic, SLOC = source lines.

use camino::Utf8Path;
use mollify_types::{FileMetrics, MetricsReport, MetricsTotals, SCHEMA_VERSION};

/// Compute per-file + project metrics for `root`.
pub fn report(root: &Utf8Path) -> MetricsReport {
    let graph = crate::build_graph(root);
    let mut files: Vec<FileMetrics> = Vec::new();
    for m in &graph.modules {
        let (loc, blank, comment_lines) = line_counts(&m.path);
        let sloc = loc
            .saturating_sub(blank)
            .saturating_sub(comment_lines)
            .max(1);
        let total_cyclomatic: u32 = m.parsed.functions.iter().map(|f| f.cyclomatic).sum();
        let max_cyclomatic = m
            .parsed
            .functions
            .iter()
            .map(|f| f.cyclomatic)
            .max()
            .unwrap_or(0);
        let cc = total_cyclomatic.max(1) as f64;
        let volume = m.parsed.halstead_volume.max(1.0);
        let mi_raw = 171.0 - 5.2 * volume.ln() - 0.23 * cc - 16.2 * (sloc as f64).ln();
        let mi = (mi_raw * 100.0 / 171.0).clamp(0.0, 100.0);
        files.push(FileMetrics {
            path: m.rel.clone(),
            loc,
            sloc,
            comment_lines,
            blank_lines: blank,
            functions: m.parsed.functions.len() as u32,
            total_cyclomatic,
            max_cyclomatic,
            maintainability_index: (mi * 100.0).round() / 100.0,
            mi_rank: rank(mi),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let n = files.len().max(1) as f64;
    let totals = MetricsTotals {
        files: files.len(),
        loc: files.iter().map(|f| f.loc).sum(),
        sloc: files.iter().map(|f| f.sloc).sum(),
        functions: files.iter().map(|f| f.functions).sum(),
        mean_maintainability_index: (files.iter().map(|f| f.maintainability_index).sum::<f64>()
            / n
            * 100.0)
            .round()
            / 100.0,
    };
    MetricsReport {
        schema_version: SCHEMA_VERSION.into(),
        files,
        totals,
    }
}

/// radon's MI ranks on the 0–100 scale: A 20–100, B 10–<20, C <10.
fn rank(mi: f64) -> char {
    if mi >= 20.0 {
        'A'
    } else if mi >= 10.0 {
        'B'
    } else {
        'C'
    }
}

/// (physical lines, blank lines, comment lines) for a file. Notebook cells are
/// already concatenated by `read_source`.
fn line_counts(path: &Utf8Path) -> (u32, u32, u32) {
    let Some(src) = mollify_graph::read_source(path) else {
        return (0, 0, 0);
    };
    let (mut loc, mut blank, mut comment) = (0u32, 0u32, 0u32);
    for line in src.lines() {
        loc += 1;
        let t = line.trim_start();
        if t.is_empty() {
            blank += 1;
        } else if t.starts_with('#') {
            comment += 1;
        }
    }
    (loc, blank, comment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-metrics-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn computes_metrics_and_mi() {
        let d = temp("m");
        std::fs::write(
            d.join("a.py"),
            "# a comment\n\ndef f(x):\n    if x:\n        return 1\n    return 0\n",
        )
        .unwrap();
        let r = report(&d);
        assert_eq!(r.totals.files, 1);
        let fm = &r.files[0];
        assert_eq!(fm.functions, 1);
        assert!(fm.comment_lines >= 1 && fm.blank_lines >= 1);
        assert!(fm.maintainability_index > 0.0 && fm.maintainability_index <= 100.0);
        assert!(['A', 'B', 'C'].contains(&fm.mi_rank));
        std::fs::remove_dir_all(&d).ok();
    }
}
