//! Installed-environment introspection. When a virtualenv is present, reads
//! `*.dist-info` metadata from `site-packages` to (a) map import names to
//! distributions accurately (beyond the static alias table) and (b) know which
//! distributions are actually installed — which lets `deps` distinguish a
//! **transitive** dependency (installed but undeclared) from a genuinely
//! **missing** one (not installed at all). Best-effort: absent venv → `None`.

use crate::known::normalize_dist;
use camino::Utf8Path;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Default)]
pub struct Installed {
    /// import top-level name → normalized distribution name.
    pub import_to_dist: FxHashMap<String, String>,
    /// All installed (normalized) distribution names.
    pub dists: FxHashSet<String>,
    /// normalized distribution name → installed version (from dist-info METADATA).
    /// Lets supply-chain resolve a declared *range* to the concrete version that
    /// is actually installed, for precise advisory matching.
    pub versions: FxHashMap<String, String>,
}

/// Discover and parse the project's virtualenv `site-packages`, if any.
pub fn discover(root: &Utf8Path) -> Option<Installed> {
    let sp = find_site_packages(root)?;
    let mut inst = Installed::default();
    // Sorted: `read_dir` order is filesystem-dependent, and when several
    // dists claim the same top-level import (namespace packages), which one
    // wins must not vary across machines (invariant: byte-identical output).
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&sp)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().ends_with(".dist-info"))
                .unwrap_or(false)
        })
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        let name = name.to_string();
        // Distribution name from METADATA `Name:`, else the dir prefix.
        let meta = std::fs::read_to_string(dir.join("METADATA")).ok();
        let dist = meta
            .as_ref()
            .and_then(|m| {
                m.lines()
                    .find_map(|l| l.strip_prefix("Name:").map(|n| n.trim().to_string()))
            })
            .unwrap_or_else(|| name.split('-').next().unwrap_or(&name).to_string());
        let dist = normalize_dist(&dist);
        inst.dists.insert(dist.clone());
        if let Some(ver) = meta.as_ref().and_then(|m| {
            m.lines()
                .find_map(|l| l.strip_prefix("Version:").map(|v| v.trim().to_string()))
        }) {
            inst.versions.insert(dist.clone(), ver);
        }

        // Import names from top_level.txt; fall back to the dist name.
        let tops = std::fs::read_to_string(dir.join("top_level.txt"))
            .ok()
            .map(|t| {
                t.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec![dist.replace('-', "_")]);
        for top in tops {
            // Only the package's top segment matters for import resolution.
            let top = top.split('/').next().unwrap_or(&top).to_string();
            // On a claim collision, a dist named after the import wins
            // (`requests` over an alphabetically-earlier claimant); otherwise
            // first-in-sorted-order stays — deterministic either way.
            let self_named = normalize_dist(&top) == dist;
            match inst.import_to_dist.entry(top) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    if self_named && normalize_dist(e.key()) != *e.get() {
                        e.insert(dist.clone());
                    }
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(dist.clone());
                }
            }
        }
    }
    if inst.dists.is_empty() {
        None
    } else {
        Some(inst)
    }
}

/// Locate a `site-packages` directory for the project (common venv layouts and
/// `$VIRTUAL_ENV`). Returns the first that exists.
fn find_site_packages(root: &Utf8Path) -> Option<camino::Utf8PathBuf> {
    let mut roots: Vec<camino::Utf8PathBuf> = vec![
        root.join(".venv"),
        root.join("venv"),
        root.join("env"),
        root.join(".env"),
    ];
    if let Ok(v) = std::env::var("VIRTUAL_ENV") {
        roots.insert(0, camino::Utf8PathBuf::from(v));
    }
    for venv in roots {
        // POSIX: <venv>/lib/pythonX.Y/site-packages ; Windows: <venv>/Lib/site-packages
        for libdir in ["lib", "Lib"] {
            let base = venv.join(libdir);
            let Ok(rd) = std::fs::read_dir(&base) else {
                continue;
            };
            // Windows layout: Lib/site-packages directly.
            let direct = base.join("site-packages");
            if direct.is_dir() {
                return Some(direct);
            }
            // POSIX: lib/pythonX.Y/site-packages.
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if let Ok(p) = camino::Utf8PathBuf::from_path_buf(p) {
                        let sp = p.join("site-packages");
                        if sp.is_dir() {
                            return Some(sp);
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn parses_dist_info_from_a_synthetic_venv() {
        let d = std::env::temp_dir().join(format!("mollify-installed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let sp = d.join(".venv/lib/python3.11/site-packages/requests-2.31.0.dist-info");
        std::fs::create_dir_all(&sp).unwrap();
        std::fs::write(sp.join("METADATA"), "Name: requests\nVersion: 2.31.0\n").unwrap();
        std::fs::write(sp.join("top_level.txt"), "requests\n").unwrap();
        let root = Utf8PathBuf::from_path_buf(d.clone()).unwrap();
        let inst = discover(&root).unwrap();
        assert!(inst.dists.contains("requests"));
        assert_eq!(
            inst.import_to_dist.get("requests").map(|s| s.as_str()),
            Some("requests")
        );
        std::fs::remove_dir_all(&d).ok();
    }
}
