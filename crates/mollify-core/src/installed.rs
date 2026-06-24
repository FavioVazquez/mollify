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
}

/// Discover and parse the project's virtualenv `site-packages`, if any.
pub fn discover(root: &Utf8Path) -> Option<Installed> {
    let sp = find_site_packages(root)?;
    let mut inst = Installed::default();
    for entry in std::fs::read_dir(&sp).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".dist-info") {
            continue;
        }
        let dir = entry.path();
        // Distribution name from METADATA `Name:`, else the dir prefix.
        let dist = std::fs::read_to_string(dir.join("METADATA"))
            .ok()
            .and_then(|m| {
                m.lines()
                    .find_map(|l| l.strip_prefix("Name:").map(|n| n.trim().to_string()))
            })
            .unwrap_or_else(|| name.split('-').next().unwrap_or(&name).to_string());
        let dist = normalize_dist(&dist);
        inst.dists.insert(dist.clone());

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
            inst.import_to_dist
                .entry(top)
                .or_insert_with(|| dist.clone());
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
