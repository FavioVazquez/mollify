//! Dependency-hygiene engine: declared-but-unused and imported-but-undeclared
//! distributions. Parses `pyproject.toml` (PEP 621 + Poetry + PEP 735 groups).
//!
//! import→distribution mapping uses the installed env's `*.dist-info` metadata
//! when a virtualenv is present (accurate), falling back to a stdlib set + alias
//! table otherwise. With the installed set known, an imported-but-undeclared
//! package is split into `transitive-dependency` (installed) vs
//! `missing-dependency` (not installed). Findings stay `Likely`/`Uncertain`.

use crate::fingerprint::fingerprint;
use crate::known::{normalize_dist, Known};
use camino::Utf8Path;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use rustc_hash::FxHashSet;

/// Analyze dependency hygiene. `root` is the project root. Declared dependencies
/// are gathered from `pyproject.toml` (PEP 621 + Poetry + uv + pdm + PEP 735) and
/// any `requirements*.txt` files, so projects without a pyproject still work.
pub fn analyze(root: &Utf8Path, graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    let pyproject_path = root.join("pyproject.toml");
    let mut declared = FxHashSet::default();
    // Manifest the findings point at (pyproject if present, else a requirements file).
    let mut manifest = pyproject_path.clone();

    let mut has_manifest = false;
    if let Ok(text) = std::fs::read_to_string(&pyproject_path) {
        has_manifest = true;
        if let Ok(table) = text.parse::<toml::Table>() {
            declared.extend(declared_dependencies(&toml::Value::Table(table)));
        }
    }
    // requirements*.txt (pip / pip-tools) — `name[extras]op version` per line.
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let fname = entry.file_name();
        let fname = fname.to_string_lossy();
        if fname.starts_with("requirements") && fname.ends_with(".txt") {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                let before = declared.len();
                for line in text.lines() {
                    let line = line.split('#').next().unwrap_or("").trim();
                    if line.is_empty() || line.starts_with('-') {
                        continue;
                    }
                    if let Some(name) = spec_name(line) {
                        declared.insert(name);
                    }
                }
                has_manifest = true;
                if declared.len() > before && !pyproject_path.exists() {
                    if let Ok(p) = camino::Utf8PathBuf::from_path_buf(entry.path()) {
                        manifest = p;
                    }
                }
            }
        }
    }
    let pyproject_path = manifest;
    // No manifest at all → nothing to check (avoid flagging every import as
    // "missing" in a project that simply doesn't declare dependencies here).
    if !has_manifest {
        return findings;
    }

    let known = Known::new();
    let internal_tops = internal_top_levels(graph);
    // Accurate import→dist mapping + installed set from a venv, if present.
    let installed = crate::installed::discover(root);
    let used_dists = used_distributions(graph, &known, &internal_tops, installed.as_ref());

    let confidence = if graph.global_dynamic {
        Confidence::Uncertain
    } else {
        Confidence::Likely
    };

    // Unused: declared but never imported.
    for dist in &declared {
        if dist == "python" {
            continue;
        }
        if !used_dists.contains(dist) {
            let rule = "unused-dependency";
            findings.push(Finding {
                fingerprint: fingerprint(rule, &[dist]),
                rule: rule.into(),
                category: Category::DependencyHygiene,
                severity: Severity::Warn,
                confidence,
                attribution: None,
                reason: format!("declared dependency `{dist}` is never imported"),
                location: Location {
                    path: pyproject_path.clone(),
                    line: 1,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "remove-dependency".into(),
                    description: format!("Remove unused dependency `{dist}` from pyproject.toml"),
                    auto_fixable: false,
                    suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                }],
            });
        }
    }

    // Imported but not declared. If we can see the installed env, split into
    // `transitive-dependency` (installed as someone else's sub-dep) vs
    // `missing-dependency` (not installed at all).
    for dist in &used_dists {
        if declared.contains(dist) {
            continue;
        }
        let is_transitive = installed.as_ref().is_some_and(|i| i.dists.contains(dist));
        let (rule, reason, action) = if is_transitive {
            (
                "transitive-dependency",
                format!("`{dist}` is imported and installed, but only as a transitive dependency — declare it directly"),
                format!("Add `{dist}` to your direct dependencies (currently transitive)"),
            )
        } else {
            (
                "missing-dependency",
                format!("`{dist}` is imported but not declared in the project manifest"),
                format!("Add `{dist}` to project dependencies"),
            )
        };
        findings.push(Finding {
            fingerprint: fingerprint(rule, &[dist]),
            rule: rule.into(),
            category: Category::DependencyHygiene,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason,
            location: Location {
                path: pyproject_path.clone(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "add-dependency".into(),
                description: action,
                auto_fixable: false,
                suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
            }],
        });
    }

    findings
}

/// Collect declared distribution names (normalized) from the manifest.
fn declared_dependencies(value: &toml::Value) -> FxHashSet<String> {
    let mut set = FxHashSet::default();

    // PEP 621: [project].dependencies = ["requests>=2", ...]
    if let Some(arr) = value
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
    {
        for item in arr {
            if let Some(s) = item.as_str() {
                if let Some(name) = spec_name(s) {
                    set.insert(name);
                }
            }
        }
    }
    // PEP 621 optional + PEP 735 groups: tables of arrays of specs.
    for key in ["optional-dependencies"] {
        if let Some(tbl) = value
            .get("project")
            .and_then(|p| p.get(key))
            .and_then(|t| t.as_table())
        {
            for (_group, arr) in tbl {
                if let Some(arr) = arr.as_array() {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            if let Some(name) = spec_name(s) {
                                set.insert(name);
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(tbl) = value.get("dependency-groups").and_then(|t| t.as_table()) {
        for (_g, arr) in tbl {
            if let Some(arr) = arr.as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if let Some(name) = spec_name(s) {
                            set.insert(name);
                        }
                    }
                }
            }
        }
    }
    // Poetry: [tool.poetry.dependencies] is a table keyed by name.
    if let Some(tbl) = value
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        for name in tbl.keys() {
            set.insert(normalize_dist(name));
        }
    }
    // Poetry groups: [tool.poetry.group.<g>.dependencies].
    if let Some(groups) = value
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("group"))
        .and_then(|g| g.as_table())
    {
        for (_g, gv) in groups {
            if let Some(tbl) = gv.get("dependencies").and_then(|d| d.as_table()) {
                for name in tbl.keys() {
                    set.insert(normalize_dist(name));
                }
            }
        }
    }
    // uv: [tool.uv] dev-dependencies (array of specs).
    if let Some(arr) = value
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.get("dev-dependencies"))
        .and_then(|d| d.as_array())
    {
        for item in arr {
            if let Some(name) = item.as_str().and_then(spec_name) {
                set.insert(name);
            }
        }
    }
    // pdm: [tool.pdm.dev-dependencies] = { group = [specs...] }.
    if let Some(tbl) = value
        .get("tool")
        .and_then(|t| t.get("pdm"))
        .and_then(|p| p.get("dev-dependencies"))
        .and_then(|d| d.as_table())
    {
        for (_g, arr) in tbl {
            if let Some(arr) = arr.as_array() {
                for item in arr {
                    if let Some(name) = item.as_str().and_then(spec_name) {
                        set.insert(name);
                    }
                }
            }
        }
    }
    set
}

/// Extract the distribution name from a PEP 508 requirement spec.
fn spec_name(spec: &str) -> Option<String> {
    let end = spec
        .find(|c: char| " <>=!~;[(".contains(c))
        .unwrap_or(spec.len());
    let name = spec[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(normalize_dist(name))
    }
}

/// Internal top-level package names (first dotted segment of each module).
fn internal_top_levels(graph: &ModuleGraph) -> FxHashSet<String> {
    let mut set = FxHashSet::default();
    for m in &graph.modules {
        if let Some(first) = m.dotted.split('.').next() {
            if !first.is_empty() {
                set.insert(first.to_string());
            }
        }
    }
    set
}

/// Distributions imported by the project (external, non-stdlib, non-internal).
/// Prefers the installed env's accurate import→dist map when available.
fn used_distributions(
    graph: &ModuleGraph,
    known: &Known,
    internal: &FxHashSet<String>,
    installed: Option<&crate::installed::Installed>,
) -> FxHashSet<String> {
    let mut set = FxHashSet::default();
    for m in &graph.modules {
        for imp in &m.parsed.imports {
            if imp.relative_dots > 0 {
                continue; // relative = internal
            }
            let Some(top) = imp.module.split('.').next() else {
                continue;
            };
            if top.is_empty() || internal.contains(top) || known.is_stdlib(top) {
                continue;
            }
            let dist = installed
                .and_then(|i| i.import_to_dist.get(top).cloned())
                .unwrap_or_else(|| known.dist_for_import(top));
            set.insert(dist);
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-deps-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn detects_unused_and_missing() {
        let d = temp("mix");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = [\"requests>=2\", \"unused-lib\"]\n",
        )
        .unwrap();
        std::fs::write(
            d.join("app.py"),
            "import requests\nimport numpy\nimport os\nrequests.get('x')\nnumpy.array([])\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(
            f.iter()
                .any(|x| x.rule == "unused-dependency" && x.reason.contains("unused-lib")),
            "expected unused-lib, got {f:?}"
        );
        assert!(
            f.iter()
                .any(|x| x.rule == "missing-dependency" && x.reason.contains("numpy")),
            "expected missing numpy, got {f:?}"
        );
        // requests is declared and used → no finding; os is stdlib → ignored.
        assert!(!f.iter().any(|x| x.reason.contains("requests")));
        assert!(!f.iter().any(|x| x.reason.contains("`os`")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn transitive_when_installed_but_undeclared() {
        let d = temp("trans");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::write(d.join("app.py"), "import requests\nrequests.get('x')\n").unwrap();
        // Synthetic venv with requests installed (as if pulled in transitively).
        let sp = d.join(".venv/lib/python3.11/site-packages/requests-2.31.0.dist-info");
        std::fs::create_dir_all(&sp).unwrap();
        std::fs::write(sp.join("METADATA"), "Name: requests\n").unwrap();
        std::fs::write(sp.join("top_level.txt"), "requests\n").unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(
            f.iter()
                .any(|x| x.rule == "transitive-dependency" && x.reason.contains("requests")),
            "got {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn reads_requirements_txt_when_no_pyproject() {
        let d = temp("req");
        std::fs::write(
            d.join("requirements.txt"),
            "requests==2.0\nunused-lib==1.0\n",
        )
        .unwrap();
        std::fs::write(
            d.join("app.py"),
            "import requests\nimport numpy\nrequests.get('x')\nnumpy.array([])\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(
            f.iter()
                .any(|x| x.rule == "unused-dependency" && x.reason.contains("unused-lib")),
            "got {f:?}"
        );
        assert!(
            f.iter()
                .any(|x| x.rule == "missing-dependency" && x.reason.contains("numpy")),
            "got {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn spec_name_strips_versions_and_extras() {
        assert_eq!(
            spec_name("uvicorn[standard]>=0.20").as_deref(),
            Some("uvicorn")
        );
        assert_eq!(spec_name("Flask_Login").as_deref(), Some("flask-login"));
    }
}
