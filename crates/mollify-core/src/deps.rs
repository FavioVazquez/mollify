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
use rustc_hash::{FxHashMap, FxHashSet};

/// Analyze dependency hygiene. `root` is the project root. Declared dependencies
/// are gathered from `pyproject.toml` (PEP 621 + Poetry + uv + pdm + PEP 735) and
/// any `requirements*.txt` files, so projects without a pyproject still work.
pub fn analyze(root: &Utf8Path, graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    let pyproject_path = root.join("pyproject.toml");
    let mut declared = FxHashSet::default();
    // Dependencies declared *only* in dev/test groups (deptry DEP004 input).
    let mut dev_only: FxHashSet<String> = FxHashSet::default();
    // Manifest the findings point at (pyproject if present, else a requirements file).
    let mut manifest = pyproject_path.clone();

    let mut has_manifest = false;
    if let Ok(text) = std::fs::read_to_string(&pyproject_path) {
        has_manifest = true;
        if let Ok(table) = text.parse::<toml::Table>() {
            let val = toml::Value::Table(table);
            declared.extend(declared_dependencies(&val));
            let prod = prod_dependencies(&val);
            for d in dev_dependencies(&val) {
                if !prod.contains(&d) {
                    dev_only.insert(d);
                }
            }
        }
    }
    // requirements*.txt (pip / pip-tools) — `name[extras]op version` per line.
    // Sorted by file name: `read_dir` order is filesystem-dependent, and both
    // the declared set's manifest attribution and finding locations must be
    // deterministic across machines.
    let mut req_files: Vec<camino::Utf8PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| camino::Utf8PathBuf::from_path_buf(e.path()).ok())
        .filter(|p| {
            p.file_name()
                .is_some_and(|f| f.starts_with("requirements") && f.ends_with(".txt"))
        })
        .collect();
    req_files.sort();
    for path in req_files {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let before = declared.len();
            for line in text.lines() {
                if let Some(name) = requirement_name(line) {
                    declared.insert(name);
                }
            }
            has_manifest = true;
            if declared.len() > before && !pyproject_path.exists() && manifest == pyproject_path {
                manifest = path;
            }
        }
    }
    let manifest_path = manifest;
    let manifest_name = manifest_path
        .file_name()
        .unwrap_or("pyproject.toml")
        .to_string();
    // No manifest at all → nothing to check (avoid flagging every import as
    // "missing" in a project that simply doesn't declare dependencies here).
    if !has_manifest {
        return findings;
    }

    let known = Known::new();
    let internal_tops = internal_top_levels(graph);
    // Accurate import→dist mapping + installed set from a venv, if present.
    let installed = crate::installed::discover(root);
    let used = used_distributions(graph, &known, &internal_tops, installed.as_ref());

    let confidence = if graph.global_dynamic {
        Confidence::Uncertain
    } else {
        Confidence::Likely
    };

    // Unused: declared but never imported. Dev-group tools (black, mypy,
    // pre-commit, pytest plugins…) are invoked, not imported — deptry exempts
    // dev dependencies from this check for the same reason.
    for dist in &declared {
        if dist == "python" || dev_only.contains(dist) {
            continue;
        }
        if !used.candidates.contains(dist) {
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
                    path: manifest_path.clone(),
                    line: 1,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "remove-dependency".into(),
                    description: format!("Remove unused dependency `{dist}` from {manifest_name}"),
                    auto_fixable: false,
                    suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                }],
            });
        }
    }

    // Imported but not declared (under ANY plausible providing dist). If we
    // can see the installed env, split into `transitive-dependency` (installed
    // as someone else's sub-dep) vs `missing-dependency` (not installed).
    for u in &used.imports {
        if u.candidates.iter().any(|c| declared.contains(c)) {
            continue;
        }
        if u.unresolvable_namespace {
            // `google`/`azure`/… are claimed by many unrelated dists; without
            // an installed env we can't name the right one — stay silent
            // rather than guess wrong.
            continue;
        }
        let dist = &u.primary;
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
                path: manifest_path.clone(),
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

    // Misplaced dev dependency (deptry DEP004): a dependency declared only in a
    // dev/test group but imported from production (non-test) code. Reported once
    // per distribution, pointing at the manifest.
    if !dev_only.is_empty() {
        let mut seen: FxHashSet<String> = FxHashSet::default();
        for m in &graph.modules {
            if is_test_module(&m.path) {
                continue;
            }
            for dist in module_imported_dists(m, &known, &internal_tops, installed.as_ref()) {
                if !dev_only.contains(&dist) || !seen.insert(dist.clone()) {
                    continue;
                }
                let rule = "misplaced-dev-dependency";
                findings.push(Finding {
                    fingerprint: fingerprint(rule, &[&dist]),
                    rule: rule.into(),
                    category: Category::DependencyHygiene,
                    severity: Severity::Warn,
                    confidence,
                    attribution: None,
                    reason: format!(
                        "`{dist}` is declared only as a dev dependency but is imported by production module `{}`",
                        m.dotted
                    ),
                    location: Location {
                        path: manifest_path.clone(),
                        line: 1,
                        column: 0,
                        end_line: None,
                    },
                    actions: vec![Action {
                        kind: "move-dependency".into(),
                        description: format!(
                            "Move `{dist}` from the dev group to runtime dependencies"
                        ),
                        auto_fixable: false,
                        suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                    }],
                });
            }
        }
    }

    findings
}

/// Flag imports that look first-party or relative but resolve to no module in
/// the project (typo / broken refactor) — distinct from `missing-dependency`,
/// which is third-party. Both tiers are `likely`, not `certain`: a relative
/// import must be internal, but it may resolve to something the `.py` walk
/// can't see — an in-tree C/Cython extension (`._speedups`) or a
/// build-generated module (`._version`).
/// Independent of any manifest, so it runs even with no `pyproject.toml`.
pub fn unresolved(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut occ = crate::fingerprint::Occurrences::default();
    for u in graph.unresolved_imports() {
        let rule = "unresolved-import";
        let confidence = Confidence::Likely;
        let kind = if u.relative {
            "relative"
        } else {
            "first-party"
        };
        let occ_key = format!("{}\u{1f}{}", u.importer_rel, u.display);
        findings.push(Finding {
            fingerprint: fingerprint(
                rule,
                &[u.importer_rel.as_str(), &u.display, &occ.next(&occ_key)],
            ),
            rule: rule.into(),
            category: Category::DependencyHygiene,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason: format!(
                "{kind} import `{}` does not resolve to any module in the project",
                u.display
            ),
            location: Location {
                path: u.importer.clone(),
                line: u.line,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "fix-import".into(),
                description: format!(
                    "Fix or remove the broken import `{}` (check the module path / refactor)",
                    u.display
                ),
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
    // Legacy Poetry (pre-1.2): [tool.poetry.dev-dependencies] — a table keyed by
    // name, the old home for dev deps before group syntax. Still common in the
    // wild; without it, declared dev tools look "missing" when imported.
    if let Some(tbl) = value
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dev-dependencies"))
        .and_then(|d| d.as_table())
    {
        for name in tbl.keys() {
            set.insert(normalize_dist(name));
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
/// `@` terminates the name too (direct references: `pkg @ https://…`).
fn spec_name(spec: &str) -> Option<String> {
    let end = spec
        .find(|c: char| " <>=!~;[(@".contains(c))
        .unwrap_or(spec.len());
    let name = spec[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(normalize_dist(name))
    }
}

/// Extract a declared name from one requirements.txt line: comments (a `#`
/// at line start or preceded by whitespace, per pip), pip options, and
/// URL/VCS requirements handled. A VCS/URL line names its dist only via
/// `#egg=`; without it the line declares nothing we can name — better silent
/// than a mangled `git+https-…` finding.
fn requirement_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }
    // URL/VCS requirement: the `#egg=name` fragment IS the name — read it
    // before any comment stripping (the fragment starts with `#`).
    let lower = trimmed.to_ascii_lowercase();
    if [
        "git+", "hg+", "svn+", "bzr+", "http://", "https://", "file:",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
    {
        return trimmed.split_once("#egg=").and_then(|(_, rest)| {
            let name = rest.split(|c: char| c.is_whitespace() || c == '&').next()?;
            spec_name(name)
        });
    }
    // Strip an end-of-line comment: pip requires whitespace before `#`.
    let code = match trimmed.find(" #") {
        Some(i) => &trimmed[..i],
        None => trimmed,
    };
    spec_name(code.trim())
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
        // pytest puts test dirs on sys.path, so sibling helpers (conftest.py,
        // reference.py, …) are imported by bare leaf name. Register those leaves
        // as first-party so they aren't mistaken for external distributions.
        if is_test_module(&m.path) {
            if let Some(leaf) = m.dotted.rsplit('.').next() {
                if !leaf.is_empty() {
                    set.insert(leaf.to_string());
                }
            }
        }
    }
    set
}

/// Module names referenced by `[project.scripts]` / `[project.gui-scripts]` /
/// `[tool.poetry.scripts]` console-script entry points (the `pkg.mod` half of a
/// `pkg.mod:func` target). These are reachability roots even with no in-repo
/// caller. Returns dotted module names.
pub fn entry_point_modules(root: &Utf8Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("pyproject.toml")) else {
        return Vec::new();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let val = toml::Value::Table(table);
    let mut modules = FxHashSet::default();
    let mut harvest = |tbl: Option<&toml::Value>| {
        if let Some(t) = tbl.and_then(|t| t.as_table()) {
            for target in t.values().filter_map(|v| v.as_str()) {
                // `pkg.mod:func` → `pkg.mod`; a bare `pkg.mod` counts too.
                let module = target.split(':').next().unwrap_or(target).trim();
                if !module.is_empty() {
                    modules.insert(module.to_string());
                }
            }
        }
    };
    harvest(val.get("project").and_then(|p| p.get("scripts")));
    harvest(val.get("project").and_then(|p| p.get("gui-scripts")));
    harvest(
        val.get("tool")
            .and_then(|t| t.get("poetry"))
            .and_then(|p| p.get("scripts")),
    );
    let mut out: Vec<String> = modules.into_iter().collect();
    out.sort();
    out
}

/// The `(module, function)` pairs named by console-script entry points
/// (`pkg.mod:func`). The function is invoked by the installed script, so it is a
/// reachability root and must not be reported `unused-export`.
pub fn entry_point_symbols(root: &Utf8Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join("pyproject.toml")) else {
        return Vec::new();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let val = toml::Value::Table(table);
    let mut pairs = FxHashSet::default();
    let mut harvest = |tbl: Option<&toml::Value>| {
        if let Some(t) = tbl.and_then(|t| t.as_table()) {
            for target in t.values().filter_map(|v| v.as_str()) {
                if let Some((module, func)) = target.split_once(':') {
                    let module = module.trim();
                    // `pkg.mod:obj.method` → take the first attribute as the root.
                    let func = func.trim().split('.').next().unwrap_or("").trim();
                    if !module.is_empty() && !func.is_empty() {
                        pairs.insert((module.to_string(), func.to_string()));
                    }
                }
            }
        }
    };
    harvest(val.get("project").and_then(|p| p.get("scripts")));
    harvest(val.get("project").and_then(|p| p.get("gui-scripts")));
    harvest(
        val.get("tool")
            .and_then(|t| t.get("poetry"))
            .and_then(|p| p.get("scripts")),
    );
    let mut out: Vec<(String, String)> = pairs.into_iter().collect();
    out.sort();
    out
}

/// One distinct external import, with every distribution that plausibly
/// provides it.
struct UsedImport {
    /// Preferred dist name for messages/fingerprints.
    primary: String,
    /// A declared dep matching ANY of these means the import is declared.
    candidates: Vec<String>,
    /// Namespace top (`google`, `azure`, …) with no installed env to name the
    /// real dist — `missing-dependency` stays silent for these.
    unresolvable_namespace: bool,
}

struct UsedDistributions {
    /// Union of all candidates: the "is this declared dep imported?" set.
    candidates: FxHashSet<String>,
    /// Distinct imports keyed by primary, sorted for deterministic output.
    imports: Vec<UsedImport>,
}

/// Distributions imported by the project (external, non-stdlib, non-internal).
/// Prefers the installed env's accurate import→dist map when available.
fn used_distributions(
    graph: &ModuleGraph,
    known: &Known,
    internal: &FxHashSet<String>,
    installed: Option<&crate::installed::Installed>,
) -> UsedDistributions {
    let mut candidates = FxHashSet::default();
    let mut by_primary: FxHashMap<String, UsedImport> = FxHashMap::default();
    for m in &graph.modules {
        // Lazy/deferred imports inside functions count as usage too (a dep
        // imported only inside `main()` is not unused).
        for imp in m.parsed.imports.iter().chain(&m.parsed.nested_imports) {
            if imp.relative_dots > 0 {
                continue; // relative = internal
            }
            let Some(top) = imp.module.split('.').next() else {
                continue;
            };
            if top.is_empty() || internal.contains(top) || known.is_stdlib(top) {
                continue;
            }
            // The installed env names the exact providing dist; otherwise
            // every plausible provider counts.
            let (cands, namespace) = match installed.and_then(|i| i.import_to_dist.get(top)) {
                Some(d) => (vec![d.clone()], false),
                None => (
                    known.dists_for_import(&imp.module),
                    known.is_namespace_top(top),
                ),
            };
            candidates.extend(cands.iter().cloned());
            let primary = cands[0].clone();
            by_primary
                .entry(primary.clone())
                .and_modify(|u| {
                    // Merge candidate lists from different dotted imports of
                    // the same top level.
                    for c in &cands {
                        if !u.candidates.contains(c) {
                            u.candidates.push(c.clone());
                        }
                    }
                })
                .or_insert(UsedImport {
                    primary,
                    candidates: cands,
                    unresolvable_namespace: namespace,
                });
        }
    }
    let mut imports: Vec<UsedImport> = by_primary.into_values().collect();
    imports.sort_by(|a, b| a.primary.cmp(&b.primary));
    for u in &mut imports {
        u.candidates.sort();
    }
    UsedDistributions {
        candidates,
        imports,
    }
}

/// Distributions declared in **dev/test/lint/docs/typing** groups only (PEP 735
/// `dependency-groups`, Poetry dev groups + legacy dev-dependencies, uv/pdm dev
/// dependencies). Runtime `optional-dependencies` extras are intentionally
/// excluded (they are shipped extras, not dev tooling).
fn dev_dependencies(value: &toml::Value) -> FxHashSet<String> {
    let mut set = FxHashSet::default();
    let add_spec_array = |arr: &toml::Value, set: &mut FxHashSet<String>| {
        if let Some(arr) = arr.as_array() {
            for item in arr {
                if let Some(name) = item.as_str().and_then(spec_name) {
                    set.insert(name);
                }
            }
        }
    };
    // PEP 735 dependency-groups (dev/test/docs/...).
    if let Some(tbl) = value.get("dependency-groups").and_then(|t| t.as_table()) {
        for (_g, arr) in tbl {
            add_spec_array(arr, &mut set);
        }
    }
    // Poetry named groups (dev/test/lint/docs/typing).
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
    // Legacy Poetry dev-dependencies (table keyed by name).
    if let Some(tbl) = value
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dev-dependencies"))
        .and_then(|d| d.as_table())
    {
        for name in tbl.keys() {
            set.insert(normalize_dist(name));
        }
    }
    // uv dev-dependencies (array).
    if let Some(arr) = value
        .get("tool")
        .and_then(|t| t.get("uv"))
        .and_then(|u| u.get("dev-dependencies"))
    {
        add_spec_array(arr, &mut set);
    }
    // pdm dev-dependencies (table of group → array).
    if let Some(tbl) = value
        .get("tool")
        .and_then(|t| t.get("pdm"))
        .and_then(|p| p.get("dev-dependencies"))
        .and_then(|d| d.as_table())
    {
        for (_g, arr) in tbl {
            add_spec_array(arr, &mut set);
        }
    }
    set
}

/// Distributions declared as **runtime** dependencies (`[project].dependencies`
/// and `[tool.poetry.dependencies]`).
fn prod_dependencies(value: &toml::Value) -> FxHashSet<String> {
    let mut set = FxHashSet::default();
    if let Some(arr) = value
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
    {
        for item in arr {
            if let Some(name) = item.as_str().and_then(spec_name) {
                set.insert(name);
            }
        }
    }
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
    set
}

/// Distributions imported by a single module (external, non-stdlib, non-internal).
fn module_imported_dists(
    m: &mollify_graph::ModuleInfo,
    known: &Known,
    internal: &FxHashSet<String>,
    installed: Option<&crate::installed::Installed>,
) -> FxHashSet<String> {
    let mut set = FxHashSet::default();
    for imp in m.parsed.imports.iter().chain(&m.parsed.nested_imports) {
        if imp.relative_dots > 0 {
            continue;
        }
        let Some(top) = imp.module.split('.').next() else {
            continue;
        };
        if top.is_empty() || internal.contains(top) || known.is_stdlib(top) {
            continue;
        }
        // All plausible providers: a dev-only `psycopg2` is "imported" whether
        // the import maps to `psycopg2` or `psycopg2-binary`.
        match installed.and_then(|i| i.import_to_dist.get(top)) {
            Some(d) => {
                set.insert(d.clone());
            }
            None => set.extend(known.dists_for_import(&imp.module)),
        }
    }
    set
}

/// True if a module path is test/dev code (so importing dev deps there is fine).
fn is_test_module(path: &Utf8Path) -> bool {
    crate::paths::is_test_module(path, &[])
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
    fn flags_misplaced_dev_dependency_used_in_prod() {
        let d = temp("devdep");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = [\"requests\"]\n\n\
             [dependency-groups]\ndev = [\"pytest\"]\n",
        )
        .unwrap();
        // Production module imports the dev-only `pytest`.
        std::fs::write(d.join("app.py"), "import requests\nimport pytest\n").unwrap();
        // A test module importing pytest is fine.
        std::fs::create_dir_all(d.join("tests")).unwrap();
        std::fs::write(d.join("tests/test_app.py"), "import pytest\n").unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        let mis: Vec<_> = f
            .iter()
            .filter(|x| x.rule == "misplaced-dev-dependency")
            .collect();
        assert_eq!(mis.len(), 1, "expected one misplaced dep, got {f:?}");
        assert!(mis[0].reason.contains("pytest") && mis[0].reason.contains("app"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn flags_unresolved_relative_and_firstparty_imports() {
        let d = temp("unresolved");
        // First-party package `app` with a broken relative + absolute import.
        std::fs::write(d.join("__init__.py"), "").unwrap();
        std::fs::write(
            d.join("app.py"),
            "from .missing_mod import thing\nimport app.nope\nimport os\nfrom .real import x\n",
        )
        .unwrap();
        std::fs::write(d.join("real.py"), "x = 1\n").unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = unresolved(&g);
        // Both tiers are `likely`: a relative import must be internal, but the
        // target may be a C extension or build-generated module the .py walk
        // can't see — never `certain`.
        let rel = f
            .iter()
            .find(|x| x.reason.contains("missing_mod"))
            .expect("relative unresolved");
        assert_eq!(rel.confidence, Confidence::Likely);
        assert!(f
            .iter()
            .any(|x| x.reason.contains("app.nope") && x.confidence == Confidence::Likely));
        // `os` (stdlib) and the resolvable `.real` must not be flagged.
        assert!(!f.iter().any(|x| x.reason.contains("`os`")));
        assert!(!f.iter().any(|x| x.reason.contains(".real")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn dev_group_tools_are_not_unused() {
        // black/pytest-cov/pre-commit are invoked, not imported — dev groups
        // are exempt from unused-dependency (deptry DEP002 parity).
        let d = temp("devgroup");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = [\"requests\"]\n\n[dependency-groups]\ndev = [\"black\", \"pytest-cov\", \"pre-commit\"]\n",
        )
        .unwrap();
        std::fs::write(d.join("app.py"), "import requests\nrequests.get('x')\n").unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(
            !f.iter().any(|x| x.rule == "unused-dependency"),
            "dev tools flagged unused: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn psycopg2_declared_and_imported_is_clean() {
        // `psycopg2` is a real dist; the psycopg2-binary alias must not
        // produce a paired unused+missing false positive.
        let d = temp("psycopg2");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = [\"psycopg2\"]\n",
        )
        .unwrap();
        std::fs::write(d.join("app.py"), "import psycopg2\npsycopg2.connect('')\n").unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(
            !f.iter()
                .any(|x| x.rule == "unused-dependency" || x.rule == "missing-dependency"),
            "psycopg2 pairing false positive: {f:?}"
        );
        // Declaring the -binary dist instead is equally fine.
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = [\"psycopg2-binary\"]\n",
        )
        .unwrap();
        let f = analyze(&d, &g);
        assert!(
            !f.iter()
                .any(|x| x.rule == "unused-dependency" || x.rule == "missing-dependency"),
            "psycopg2-binary pairing false positive: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn requirement_names_handle_urls_and_comments() {
        // VCS/URL lines: only `#egg=` names a dist; otherwise stay silent.
        assert_eq!(
            requirement_name("git+https://github.com/user/repo.git@v1.0#egg=pkg"),
            Some("pkg".to_string())
        );
        assert_eq!(
            requirement_name("git+https://github.com/user/repo.git@v1.0"),
            None
        );
        // PEP 508 direct reference.
        assert_eq!(
            requirement_name("pkg @ https://example.com/pkg-1.0.tar.gz"),
            Some("pkg".to_string())
        );
        // End-of-line comments need preceding whitespace (pip rules).
        assert_eq!(
            requirement_name("requests>=2  # pinned for CVE-xxxx"),
            Some("requests".to_string())
        );
        assert_eq!(requirement_name("# a comment line"), None);
        assert_eq!(requirement_name("-r other.txt"), None);
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
    fn lazy_in_function_import_counts_as_used() {
        // A dependency imported only inside a function body (deferred/lazy) must
        // not be reported `unused-dependency`.
        let d = temp("lazy");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = [\"uvicorn\"]\n",
        )
        .unwrap();
        std::fs::write(
            d.join("app.py"),
            "def main():\n    import uvicorn\n    uvicorn.run()\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(
            !f.iter()
                .any(|x| x.rule == "unused-dependency" && x.reason.contains("uvicorn")),
            "lazy import wrongly flagged unused: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn firstparty_test_helpers_not_missing_dependency() {
        // pytest sibling imports by bare leaf name (conftest, reference) are
        // first-party, not external `missing-dependency`.
        let d = temp("helpers");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = []\n",
        )
        .unwrap();
        std::fs::create_dir_all(d.join("tests")).unwrap();
        std::fs::write(d.join("tests/conftest.py"), "import pytest\n").unwrap();
        std::fs::write(d.join("tests/reference.py"), "VALUE = 1\n").unwrap();
        std::fs::write(
            d.join("tests/test_it.py"),
            "import conftest\nfrom reference import VALUE\n\ndef test_x():\n    assert VALUE == 1\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        assert!(
            !f.iter().any(|x| x.rule == "missing-dependency"
                && (x.reason.contains("conftest") || x.reason.contains("reference"))),
            "first-party helper wrongly flagged missing: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn parses_console_script_entry_points() {
        let d = temp("scripts");
        std::fs::write(
            d.join("pyproject.toml"),
            "[project]\nname = \"x\"\n\n[project.scripts]\nserve = \"myapp.cli:main\"\n\
             [project.gui-scripts]\ngui = \"myapp.ui\"\n",
        )
        .unwrap();
        let mods = entry_point_modules(&d);
        assert!(mods.contains(&"myapp.cli".to_string()), "got {mods:?}");
        assert!(mods.contains(&"myapp.ui".to_string()), "got {mods:?}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn legacy_poetry_dev_dependencies_count_as_declared() {
        // Pre-1.2 Poetry put dev deps in [tool.poetry.dev-dependencies]. A tool
        // declared there and imported in code must NOT be reported as missing.
        let d = temp("poetry-legacy");
        std::fs::write(
            d.join("pyproject.toml"),
            "[tool.poetry]\nname = \"x\"\n\n\
             [tool.poetry.dependencies]\npython = \"^3.10\"\nrequests = \"2.31.0\"\n\n\
             [tool.poetry.dev-dependencies]\nblack = \"24.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            d.join("app.py"),
            "import black\nimport requests\nblack.format_str('x')\nrequests.get('y')\n",
        )
        .unwrap();
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&d, &g);
        // black is *declared* (legacy dev-deps), so never `missing`/`unused`.
        assert!(
            !f.iter().any(|x| matches!(
                x.rule.as_str(),
                "missing-dependency" | "unused-dependency"
            ) && x.reason.contains("black")),
            "black is declared (legacy dev-deps) → not missing/unused, got {f:?}"
        );
        // But it IS a dev-only dep imported by production code (DEP004).
        assert!(
            f.iter()
                .any(|x| x.rule == "misplaced-dev-dependency" && x.reason.contains("black")),
            "black (dev-only) imported in prod → misplaced, got {f:?}"
        );
        // requests is a runtime dep, declared + used → clean.
        assert!(!f.iter().any(|x| x.reason.contains("requests")));
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
