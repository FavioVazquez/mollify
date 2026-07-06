//! Dead-code engine: reachability-based unused files and unused top-level
//! symbols, with confidence tiers (RESEARCH.md §4 / PLAN.md §4).

use crate::fingerprint::{fingerprint, Occurrences};
use mollify_graph::ModuleGraph;
use mollify_parse::DefKind;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use rustc_hash::{FxHashMap, FxHashSet};

/// Run dead-code analysis over the graph (convention-only test detection).
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    analyze_with(graph, &[], &[])
}

/// Like [`analyze`], honoring extra `test_dirs` (a project's pytest
/// `testpaths`) so `test_*`/`Test*` collection roots in those dirs are treated
/// as reachable, and `entry_symbols` (`(module, function)` pairs named by
/// `[project.scripts]`) which are roots invoked by the installed console script.
pub fn analyze_with(
    graph: &ModuleGraph,
    test_dirs: &[String],
    entry_symbols: &[(String, String)],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    unused_files(graph, &mut findings);
    unused_symbols(graph, test_dirs, entry_symbols, &mut findings);
    unused_imports(graph, &mut findings);
    unused_locals(graph, &mut findings);
    unreachable_code(graph, &mut findings);
    duplicate_exports(graph, &mut findings);
    findings
}

/// Flag a re-export surface (`__init__.py`) that binds the **same name** from
/// two different modules — the later import silently shadows the earlier, so one
/// re-export is dead and the public API is ambiguous (fallow's "duplicate
/// export"). Confidence `likely`; skipped under a dynamic sink.
fn duplicate_exports(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in &graph.modules {
        if m.path.file_name() != Some("__init__.py") || m.parsed.has_dynamic_sink {
            continue;
        }
        // binding name -> (first source module, first line)
        let mut first: FxHashMap<&str, (&str, u32)> = FxHashMap::default();
        let mut occ = Occurrences::default();
        for imp in &m.parsed.imports {
            if imp.is_star {
                continue;
            }
            for b in &imp.bindings {
                match first.get(b.as_str()) {
                    None => {
                        first.insert(b.as_str(), (imp.module.as_str(), imp.line));
                    }
                    Some(&(src, _)) if src == imp.module => {} // same source: not a conflict
                    Some(_) => {
                        let rule = "duplicate-export";
                        out.push(Finding {
                            fingerprint: fingerprint(
                                rule,
                                &[m.rel.as_str(), b, &imp.module, &occ.next(b.as_str())],
                            ),
                            rule: rule.into(),
                            category: Category::Architecture,
                            severity: Severity::Warn,
                            confidence: Confidence::Likely,
                            attribution: None,
                            reason: format!(
                                "`{b}` is re-exported from multiple modules here; the later import shadows the earlier"
                            ),
                            location: Location {
                                path: m.path.clone(),
                                line: imp.line,
                                column: 0,
                                end_line: None,
                            },
                            actions: vec![Action {
                                kind: "dedupe-export".into(),
                                description: format!(
                                    "Keep a single source for `{b}` in this package's public API"
                                ),
                                auto_fixable: false,
                                suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                            }],
                        });
                    }
                }
            }
        }
    }
}

/// Flag statements that can never execute because they follow an unconditional
/// terminator (`return`/`raise`/`break`/`continue`/`sys.exit()`) in the same
/// block (ruff F-series / vulture parity). Syntactic and exact → `certain`, but
/// never auto-fixed (the dead statement may document intent).
fn unreachable_code(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in &graph.modules {
        let mut occ = Occurrences::default();
        for u in &m.parsed.unreachable {
            let rule = "unreachable-code";
            out.push(Finding {
                fingerprint: fingerprint(rule, &[m.rel.as_str(), u.after, &occ.next(u.after)]),
                rule: rule.into(),
                category: Category::DeadCode,
                severity: Severity::Warn,
                confidence: Confidence::Certain,
                attribution: None,
                reason: format!("code after `{}` can never execute", u.after),
                location: Location {
                    path: m.path.clone(),
                    line: u.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "remove-unreachable".into(),
                    description: format!("Remove the unreachable code after `{}`", u.after),
                    auto_fixable: false,
                    suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                }],
            });
        }
    }
}

/// Flag unused local variables (`unused-variable`, ruff F841) and parameters
/// (`unused-parameter`) from the parser's per-function scope analysis. Never
/// auto-fixable: the assignment's right-hand side may have side effects.
///
/// Parameters whose signature is dictated by an interface are skipped entirely
/// (see [`param_is_interface_bound`]): the author cannot remove them, so the
/// finding would be pure noise. Found live on flask, where 100 of the corpus
/// `unused-parameter` hits were override/callback signatures.
fn unused_locals(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    // Project-wide `class name -> its method names` index for the override
    // heuristic. Keyed by bare class name: base spellings (`Base`,
    // `mod.Base`) are matched on their last segment.
    let mut base_methods: FxHashMap<&str, FxHashSet<&str>> = FxHashMap::default();
    for m in &graph.modules {
        for c in &m.parsed.classes {
            base_methods.entry(c.name.as_str()).or_default().extend(
                c.members
                    .iter()
                    .filter(|mem| mem.is_method)
                    .map(|mem| mem.name.as_str()),
            );
        }
    }
    for m in &graph.modules {
        let mut occ = Occurrences::default();
        for s in &m.parsed.scope_findings {
            if s.is_param && param_is_interface_bound(m, s.line, &base_methods) {
                continue;
            }
            let (rule, kind, confidence) = if s.is_param {
                ("unused-parameter", "parameter", Confidence::Uncertain)
            } else {
                ("unused-variable", "local variable", Confidence::Likely)
            };
            out.push(Finding {
                fingerprint: fingerprint(rule, &[m.rel.as_str(), &s.name, &occ.next(&s.name)]),
                rule: rule.into(),
                category: Category::DeadCode,
                severity: Severity::Warn,
                confidence,
                attribution: None,
                reason: format!("{kind} `{}` is assigned but never used", s.name),
                location: Location {
                    path: m.path.clone(),
                    line: s.line,
                    column: 0,
                    end_line: None,
                },
                actions: vec![Action {
                    kind: "remove-binding".into(),
                    description: format!(
                        "Remove the unused {kind} `{}` (or prefix it with `_`)",
                        s.name
                    ),
                    auto_fixable: false,
                    suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                }],
            });
        }
    }
}

/// True when a parameter at `line` sits inside a def whose signature is a
/// contract the author doesn't control, so `unused-parameter` must not fire:
///
/// - a dunder method (`__exit__` takes three arguments whether you use them
///   or not);
/// - a method marked `@abstractmethod` / `@overload` / `@override` (the
///   signature *is* the interface);
/// - a method overriding one an in-project base class declares;
/// - a method of a class with an external (unresolvable) base — the parent
///   library may dictate the signature;
/// - a decorated top-level function — decorators commonly register callbacks
///   whose signature the framework requires (`@app.errorhandler` handlers
///   must accept the error argument).
fn param_is_interface_bound(
    m: &mollify_graph::ModuleInfo,
    line: u32,
    base_methods: &FxHashMap<&str, FxHashSet<&str>>,
) -> bool {
    // Innermost enclosing method, if any (nested classes: latest def wins).
    let mut enclosing: Option<(&mollify_parse::ClassInfo, &mollify_parse::ClassMember)> = None;
    for c in &m.parsed.classes {
        if !(c.line <= line && line <= c.end_line) {
            continue;
        }
        for mem in c.members.iter().filter(|mem| mem.is_method) {
            if mem.line <= line && line <= mem.end_line {
                let inner = enclosing.is_none_or(|(_, b)| mem.line >= b.line);
                if inner {
                    enclosing = Some((c, mem));
                }
            }
        }
    }
    if let Some((c, mem)) = enclosing {
        if mem.name.starts_with("__") && mem.name.ends_with("__") {
            return true;
        }
        let contract_decorator = mem.decorators.iter().any(|d| {
            let last = d.rsplit('.').next().unwrap_or(d);
            matches!(last, "abstractmethod" | "overload" | "override")
        });
        if contract_decorator {
            return true;
        }
        for base in &c.bases {
            let last = base.rsplit('.').next().unwrap_or(base);
            if last == "object" {
                continue;
            }
            match base_methods.get(last) {
                // Overrides a method an in-project base declares.
                Some(methods) if methods.contains(mem.name.as_str()) => return true,
                // Base not defined in this project: external interface.
                None => return true,
                Some(_) => {}
            }
        }
        return false;
    }
    // Top-level function: any decorator may register it as a callback.
    m.parsed.definitions.iter().any(|d| {
        matches!(d.kind, DefKind::Function)
            && d.line <= line
            && line <= d.end_line
            && !d.decorators.is_empty()
    })
}

/// Flag unused imports. A *whole-statement*-unused import (every binding unused)
/// is `certain` + auto-fixable (the line can be deleted). A *partially*-unused
/// `from x import a, b` (some names used) reports each unused name as `likely`
/// (not auto-fixed — rewriting the line precisely is left to the human). Skips
/// `import *` and dynamic-sink modules. Deliberate-import idioms are honored:
/// a redundant alias (`import x as x`, PEP 484 re-export) and a binding that
/// another module imports *from* here (`from m import name`) count as used;
/// `__init__.py` re-exports and try/except availability probes are downgraded
/// to `uncertain` (never auto-fixed).
fn unused_imports(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in &graph.modules {
        let local: FxHashSet<&str> = m.parsed.local_uses.iter().map(|s| s.as_str()).collect();
        let dunder_all: Option<&Vec<String>> = m.parsed.dunder_all.as_ref();
        let is_init = m.path.file_name().is_some_and(|f| f == "__init__.py");
        // Occurrence indices are counted over ALL import statements/bindings
        // (in source order), so fixing one finding doesn't shift another's
        // fingerprint.
        let mut occ = Occurrences::default();
        let mut name_occ = Occurrences::default();
        for imp in &m.parsed.imports {
            let bindings_key = imp.bindings.join(",");
            let stmt_occurrence = occ.next(&bindings_key);
            let name_occurrences: Vec<String> = imp
                .bindings
                .iter()
                .map(|b| name_occ.next(b.as_str()))
                .collect();
            if imp.is_star || imp.bindings.is_empty() || imp.type_checking_only {
                continue; // star imports / unparsed bindings / type-only: skip
            }
            if imp.module == "__future__" {
                continue; // future imports have a compiler effect; never "unused"
            }
            let is_used = |i: usize, b: &String| {
                local.contains(b.as_str())
                    || dunder_all.is_some_and(|all| all.contains(b))
                    // `import x as x` / `from m import y as y`: explicit re-export.
                    || imp.redundant.get(i).copied().unwrap_or(false)
                    // Another module does `from <here> import <b>` — the binding
                    // is this module's export surface (compat/shim idiom).
                    || graph.name_imported_by_others(m.id, b)
            };
            let unused: Vec<&String> = imp
                .bindings
                .iter()
                .enumerate()
                .filter(|(i, b)| !is_used(*i, b))
                .map(|(_, b)| b)
                .collect();
            if unused.is_empty() {
                continue;
            }
            let whole = unused.len() == imp.bindings.len();
            let rule = "unused-import";
            if whole {
                // Entire statement unused → safe to delete the line, unless a
                // deliberate-import idiom means deletion could change behavior.
                // Unreachable modules (often fixture/data files — black's
                // formatter cases, pydantic's mypy golden inputs) are never
                // certain: editing a file nothing imports can't be verified.
                let what = format!("`{}`", imp.bindings.join("`, `"));
                let confidence = if is_init || imp.in_try || m.parsed.has_dynamic_sink {
                    Confidence::Uncertain
                } else if !graph.module_reachable(m.id) || crate::paths::is_fixture_tree(&m.rel) {
                    Confidence::Likely
                } else {
                    Confidence::Certain
                };
                // Notebook line numbers are relative to the concatenated code
                // cells, not the raw .ipynb JSON — never auto-edit those files.
                let fixable_file = m.path.extension() == Some("py");
                out.push(Finding {
                    fingerprint: fingerprint(
                        rule,
                        &[m.rel.as_str(), &bindings_key, &stmt_occurrence],
                    ),
                    rule: rule.into(),
                    category: Category::DeadCode,
                    severity: Severity::Warn,
                    confidence,
                    attribution: None,
                    reason: if imp.in_try {
                        format!(
                            "import {what} is never used in this module (inside try/except — may be an availability probe)"
                        )
                    } else {
                        format!("import {what} is never used in this module")
                    },
                    location: Location {
                        path: m.path.clone(),
                        line: imp.line,
                        column: 0,
                        end_line: None,
                    },
                    actions: vec![Action {
                        kind: "remove-import".into(),
                        description: format!("Remove the unused import {what}"),
                        auto_fixable: confidence == Confidence::Certain && fixable_file,
                        suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                    }],
                });
            } else {
                // Some names still used: report each unused name (not auto-fixed,
                // since rewriting a shared import line precisely is risky).
                for (i, name) in imp.bindings.iter().enumerate() {
                    if is_used(i, name) {
                        continue;
                    }
                    out.push(Finding {
                        fingerprint: fingerprint(
                            rule,
                            &[m.rel.as_str(), name, &name_occurrences[i]],
                        ),
                        rule: rule.into(),
                        category: Category::DeadCode,
                        severity: Severity::Warn,
                        confidence: Confidence::Likely,
                        attribution: None,
                        reason: format!(
                            "imported name `{name}` is never used (other names on this import are)"
                        ),
                        location: Location {
                            path: m.path.clone(),
                            line: imp.line,
                            column: 0,
                            end_line: None,
                        },
                        actions: vec![Action {
                            kind: "remove-import-name".into(),
                            description: format!("Remove `{name}` from the import"),
                            auto_fixable: false,
                            suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                        }],
                    });
                }
            }
        }
    }
}

fn unused_files(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in graph.unused_files() {
        // A file that cannot be reached is a strong signal, but dynamic imports
        // anywhere in the project mean we can't be certain it is never loaded.
        let confidence = if graph.global_dynamic {
            Confidence::Uncertain
        } else {
            Confidence::Likely
        };
        // Precise evidence: a file can be unreachable because nothing imports
        // it, or because everything that imports it is itself unreachable.
        let reason = if graph.has_importer(m.id) {
            format!(
                "module `{}` is only imported by unreachable modules (dead subtree)",
                m.dotted
            )
        } else {
            format!(
                "module `{}` is never imported and is not an entry point",
                m.dotted
            )
        };
        out.push(Finding {
            fingerprint: fingerprint("unused-file", &[m.rel.as_str()]),
            rule: "unused-file".into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason,
            location: Location {
                path: m.path.clone(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "remove-file".into(),
                // `rel`, not `path`: descriptions are part of the output
                // contract too, and must not echo the root's spelling.
                description: format!("Delete unused module `{}`", m.rel),
                auto_fixable: false, // file deletion is never auto-applied
                suppression_comment: Some("# mollify: ignore[unused-file]".into()),
            }],
        });
    }
}

fn unused_symbols(
    graph: &ModuleGraph,
    test_dirs: &[String],
    entry_symbols: &[(String, String)],
    out: &mut Vec<Finding>,
) {
    for m in &graph.modules {
        // Count how many top-level defs share each name (to discount def sites).
        let mut def_counts: FxHashMap<&str, u32> = FxHashMap::default();
        for d in &m.parsed.definitions {
            *def_counts.entry(d.name.as_str()).or_insert(0) += 1;
        }
        let dunder_all: Option<&Vec<String>> = m.parsed.dunder_all.as_ref();
        // pytest collects `test_*`/`Test*` in test modules; the runner is the
        // caller, so these have no in-repo references but are not dead.
        let is_test = crate::paths::is_test_module(&m.path, test_dirs);
        // Functions named by a console-script entry point in this module.
        let entry_here: FxHashSet<&str> = entry_symbols
            .iter()
            .filter(|(module, _)| module == m.dotted.as_str())
            .map(|(_, func)| func.as_str())
            .collect();

        // Occurrence is counted over ALL defs of a name (in source order), so
        // a finding's fingerprint doesn't shift when a sibling def's finding
        // is fixed or suppressed.
        let mut occ = Occurrences::default();
        for d in &m.parsed.definitions {
            let occurrence = occ.next(&d.name);
            if is_test && crate::paths::is_pytest_entity(&d.name) {
                continue;
            }
            if entry_here.contains(d.name.as_str()) {
                continue; // invoked by the installed console script
            }
            // Skip dunder/special names and explicit public API (`__all__`).
            if d.name.starts_with("__") && d.name.ends_with("__") {
                continue;
            }
            if let Some(all) = dunder_all {
                if all.contains(&d.name) {
                    continue; // declared public API — treat as used
                }
            }
            // Framework-registered symbols (routes, tasks, fixtures, CLI
            // commands, signal receivers, validators, …) are reached even with
            // zero in-repo callers — the dominant false-positive killer.
            if crate::plugins::is_framework_entry(d) {
                continue;
            }
            let defs_named = def_counts.get(d.name.as_str()).copied().unwrap_or(1);
            if graph.symbol_used(m.id, &d.name, defs_named) {
                continue;
            }

            // Confidence tiering. A dynamic sink (getattr/eval/importlib)
            // anywhere in the project can reference this symbol across module
            // boundaries, so it caps confidence exactly as in `unused_files`.
            // Unreachable modules are never certain (fixture/data hazard —
            // see `unused_imports`).
            let confidence = if m.parsed.has_dynamic_sink || graph.global_dynamic {
                Confidence::Uncertain
            } else if d.private_by_convention
                && graph.module_reachable(m.id)
                && !crate::paths::is_fixture_tree(&m.rel)
            {
                Confidence::Certain
            } else {
                Confidence::Likely
            };

            let kind_str = match d.kind {
                DefKind::Function => "function",
                DefKind::Class => "class",
                DefKind::Variable => "variable",
            };
            let rule = "unused-export";
            out.push(Finding {
                fingerprint: fingerprint(rule, &[m.rel.as_str(), &d.name, &occurrence]),
                rule: rule.into(),
                category: Category::DeadCode,
                severity: Severity::Warn,
                confidence,
                attribution: None,
                reason: format!(
                    "{kind_str} `{}` has no reachable references in the project",
                    d.name
                ),
                location: Location {
                    path: m.path.clone(),
                    line: d.line,
                    column: 0,
                    end_line: Some(d.end_line),
                },
                actions: vec![Action {
                    kind: "remove-symbol".into(),
                    description: format!("Delete unused {kind_str} `{}`", d.name),
                    // Only Certain findings in plain .py files are ever
                    // auto-fixable (notebook lines are cell-relative).
                    auto_fixable: confidence == Confidence::Certain
                        && m.path.extension() == Some("py"),
                    suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                }],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};
    use mollify_graph::discover_python_files;

    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-dc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn flags_unused_public_function_as_likely() {
        let d = temp("pub");
        write(&d, "__main__.py", "from lib import used\nused()\n");
        write(
            &d,
            "lib.py",
            "def used():\n    return 1\n\ndef dead():\n    return 2\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let dead: Vec<_> = f.iter().filter(|x| x.rule == "unused-export").collect();
        assert_eq!(dead.len(), 1, "got {f:?}");
        assert!(dead[0].reason.contains("dead"));
        assert_eq!(dead[0].confidence, Confidence::Likely);
        assert!(!dead[0].actions[0].auto_fixable);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn pytest_tests_are_not_unused_exports() {
        let d = temp("pytest");
        write(&d, "lib.py", "def helper():\n    return 1\n");
        write(
            &d,
            "tests/test_lib.py",
            "from lib import helper\n\n\
             def test_helper():\n    assert helper() == 1\n\n\
             class TestThing:\n    def test_method(self):\n        assert True\n\n\
             def real_dead():\n    return 9\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let dead: Vec<_> = f
            .iter()
            .filter(|x| x.rule == "unused-export")
            .map(|x| x.reason.clone())
            .collect();
        // test_* function and Test* class are reachable via the runner.
        assert!(
            !dead
                .iter()
                .any(|r| r.contains("test_helper") || r.contains("TestThing")),
            "pytest entities wrongly flagged: {dead:?}"
        );
        // A genuinely dead non-test helper in the same file is still flagged.
        assert!(
            dead.iter().any(|r| r.contains("real_dead")),
            "real dead code missed: {dead:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn entry_point_function_not_unused_export() {
        let d = temp("entrysym");
        // `main` has no in-repo caller but is the console-script target.
        write(
            &d,
            "cli.py",
            "def main():\n    return 0\n\ndef helper():\n    return 1\n",
        );
        let files = discover_python_files(&d);
        let mut g = ModuleGraph::build(&d, &files);
        g.mark_entry_points(&["cli".to_string()]);
        let entry_syms = vec![("cli".to_string(), "main".to_string())];
        let f = analyze_with(&g, &[], &entry_syms);
        let dead: Vec<_> = f
            .iter()
            .filter(|x| x.rule == "unused-export")
            .map(|x| x.reason.clone())
            .collect();
        assert!(
            !dead.iter().any(|r| r.contains("main")),
            "entry-point function wrongly flagged: {dead:?}"
        );
        // A sibling non-entry function is still flagged.
        assert!(
            dead.iter().any(|r| r.contains("helper")),
            "real dead code missed: {dead:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn testpaths_widen_pytest_detection() {
        let d = temp("testpaths");
        // Non-conventional dir name; only recognized via testpaths.
        write(
            &d,
            "suite/check_a.py",
            "def test_alpha():\n    assert True\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        // Without testpaths, `suite/` isn't a test dir → flagged.
        let plain = analyze(&g);
        assert!(plain.iter().any(|x| x.rule == "unused-export"));
        // With testpaths = ["suite"], the test fn is a reachable root.
        let widened = analyze_with(&g, &["suite".to_string()], &[]);
        assert!(!widened.iter().any(|x| x.rule == "unused-export"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn private_unused_is_certain_and_autofixable() {
        let d = temp("priv");
        // lib must be reachable: unreachable modules are fixture-hazard-capped.
        write(&d, "__main__.py", "import lib\nlib.used()\n");
        write(
            &d,
            "lib.py",
            "def used():\n    return 1\n\ndef _dead():\n    return 2\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let s = f.iter().find(|x| x.rule == "unused-export").unwrap();
        assert_eq!(s.confidence, Confidence::Certain);
        assert!(s.actions[0].auto_fixable);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn framework_decorator_suppresses_unused() {
        let d = temp("fw");
        write(
            &d,
            "__main__.py",
            "import app
",
        );
        write(
            &d,
            "app.py",
            "import app

@app.route('/x')
def view():
    return 1
",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            !f.iter().any(|x| x.reason.contains("`view`")),
            "route should be reached, got {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn flags_unused_import_and_respects_usage_and_aliases() {
        let d = temp("imp");
        // lib must be reachable: unreachable modules are fixture-hazard-capped
        // below certain (never auto-fixable).
        write(&d, "__main__.py", "import lib\nlib.f(None)\n");
        write(
            &d,
            "lib.py",
            "import os\nimport sys\nfrom typing import List\nfrom typing import Dict\n\ndef f(x: List):\n    return sys.argv\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let imps: Vec<_> = f.iter().filter(|x| x.rule == "unused-import").collect();
        // `os` and `Dict` are unused; `sys` and `List` are used. (Partial-line
        // unused names are intentionally not flagged — only whole statements.)
        assert!(
            imps.iter().any(|x| x.reason.contains("`os`")),
            "got {imps:?}"
        );
        assert!(
            imps.iter().any(|x| x.reason.contains("`Dict`")),
            "got {imps:?}"
        );
        assert!(!imps.iter().any(|x| x.reason.contains("`sys`")));
        assert!(!imps.iter().any(|x| x.reason.contains("`List`")));
        // Regular-module unused imports are certain + auto-fixable.
        assert!(
            imps.iter()
                .find(|x| x.reason.contains("`os`"))
                .unwrap()
                .actions[0]
                .auto_fixable
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn future_imports_never_flagged_unused() {
        let d = temp("future");
        write(&d, "__main__.py", "print('hi')\n");
        write(
            &d,
            "lib.py",
            "from __future__ import annotations\nimport os\n\ndef f() -> int:\n    return 1\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let imps: Vec<_> = f.iter().filter(|x| x.rule == "unused-import").collect();
        // The __future__ import is whitelisted; the genuinely unused `os` isn't.
        assert!(
            !imps.iter().any(|x| x.reason.contains("annotations")),
            "future import wrongly flagged: {imps:?}"
        );
        assert!(
            imps.iter().any(|x| x.reason.contains("`os`")),
            "real unused import missed: {imps:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn interface_bound_params_are_never_flagged() {
        let d = temp("iface");
        write(&d, "__main__.py", "import lib\nimport base\nlib.go()\n");
        write(
            &d,
            "base.py",
            "class Base:\n    def handle(self, event):\n        return event\n",
        );
        write(
            &d,
            "lib.py",
            concat!(
                "from base import Base\n",
                "from somelib import External\n",
                "from flask import app\n",
                "\n",
                "class Sub(Base):\n",
                "    def handle(self, event):\n",
                "        return 1\n",
                "\n",
                "class Plugin(External):\n",
                "    def process(self, ctx):\n",
                "        return 1\n",
                "\n",
                "class Ctx:\n",
                "    def __exit__(self, exc_type, exc, tb):\n",
                "        return False\n",
                "\n",
                "@app.errorhandler(404)\n",
                "def notfound(err):\n",
                "    return 'x'\n",
                "\n",
                "def go(dead_p):\n",
                "    return 1\n",
            ),
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let params: Vec<_> = f.iter().filter(|x| x.rule == "unused-parameter").collect();
        // Override of an in-project base, external-base method, dunder
        // protocol, and decorated callback are all interface-bound: skipped.
        for bound in ["event", "ctx", "exc_type", "exc", "tb", "err"] {
            assert!(
                !params
                    .iter()
                    .any(|x| x.reason.contains(&format!("`{bound}`"))),
                "interface-bound param `{bound}` wrongly flagged: {params:?}"
            );
        }
        // A plain undecorated function keeps its true positive.
        assert!(
            params.iter().any(|x| x.reason.contains("`dead_p`")),
            "real unused param missed: {params:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn plain_class_method_unused_param_still_flagged() {
        let d = temp("iface-plain");
        write(&d, "__main__.py", "import lib\nlib.Svc().run(1)\n");
        write(
            &d,
            "lib.py",
            "class Svc:\n    def run(self, dead_p):\n        return 1\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        // No bases, no decorators, not a dunder: the contract is Svc's own.
        assert!(
            f.iter()
                .any(|x| x.rule == "unused-parameter" && x.reason.contains("`dead_p`")),
            "baseless method param should still be flagged: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn flags_unused_local_and_param_but_not_used_ones() {
        let d = temp("scope");
        write(&d, "__main__.py", "import lib\nlib.f(1, 2)\n");
        write(
            &d,
            "lib.py",
            "def f(used_p, dead_p):\n    dead_local = compute()\n    kept = used_p + 1\n    return kept\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            f.iter()
                .any(|x| x.rule == "unused-variable" && x.reason.contains("dead_local")),
            "got {f:?}"
        );
        assert!(
            f.iter()
                .any(|x| x.rule == "unused-parameter" && x.reason.contains("dead_p")),
            "got {f:?}"
        );
        assert!(!f.iter().any(|x| x.reason.contains("`kept`")));
        assert!(!f.iter().any(|x| x.reason.contains("used_p")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn comma_import_unused_names_get_distinct_fingerprints() {
        let d = temp("commaimp");
        write(&d, "__main__.py", "print('hi')\n");
        write(&d, "lib.py", "import os, sys\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let imps: Vec<_> = f.iter().filter(|x| x.rule == "unused-import").collect();
        assert_eq!(
            imps.len(),
            2,
            "expected one finding per unused name, got {imps:?}"
        );
        assert_ne!(
            imps[0].fingerprint, imps[1].fingerprint,
            "fingerprints must be unique per finding: {imps:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn type_checking_and_string_annotation_imports_not_flagged() {
        let d = temp("tc");
        write(&d, "__main__.py", "import lib\nlib.f(None)\n");
        write(
            &d,
            "lib.py",
            "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from collections import OrderedDict\n\ndef f(x: \"OrderedDict\"):\n    return x\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            !f.iter().any(|x| x.rule == "unused-import"),
            "TYPE_CHECKING + string-annotation import wrongly flagged: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn flags_partial_unused_import_name() {
        let d = temp("partial");
        write(&d, "__main__.py", "import lib\nlib.f()\n");
        write(
            &d,
            "lib.py",
            "from typing import List, Dict\n\ndef f() -> List:\n    return []\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        // Dict unused (List used) → partial report, not auto-fixable.
        let dict = f
            .iter()
            .find(|x| x.rule == "unused-import" && x.reason.contains("`Dict`"));
        assert!(dict.is_some(), "got {f:?}");
        assert!(!dict.unwrap().actions[0].auto_fixable);
        assert!(!f.iter().any(|x| x.reason.contains("`List`")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn quoted_type_alias_value_counts_as_use() {
        // Distilled from pydantic/functional_serializers.py: names inside a
        // quoted TypeAlias value are type syntax evaluated by checkers (and
        // by pydantic at runtime) — removing the import breaks them.
        let d = temp("typealias");
        write(&d, "__main__.py", "import lib\n");
        write(
            &d,
            "lib.py",
            "from functools import partial, partialmethod\nfrom typing import Any, TypeAlias\n\n_Partial: TypeAlias = 'partial[Any] | partialmethod[Any]'\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            !f.iter()
                .any(|x| x.rule == "unused-import" && x.reason.contains("partial")),
            "quoted TypeAlias use wrongly flagged: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn quoted_cast_type_argument_counts_as_use() {
        // Distilled from lmcache: `cast("dict[int, Iface]", x)` uses `Iface`
        // inside a string type expression; `fix --apply` deleting the import
        // introduced real F821s there (caught by the apply-then-verify pass).
        let d = temp("caststr");
        write(&d, "__main__.py", "import lib\n");
        write(
            &d,
            "lib.py",
            "from typing import cast\nfrom iface import Iface\n\ndef f(x):\n    return cast(\"dict[int, Iface]\", x)\n",
        );
        write(&d, "iface.py", "class Iface:\n    pass\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            !f.iter()
                .any(|x| x.rule == "unused-import" && x.reason.contains("Iface")),
            "quoted cast type wrongly flagged: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn redundant_alias_is_an_explicit_reexport_never_flagged() {
        // PEP 484 convention, distilled from flask/src/flask/blueprints.py:
        // `from x import Y as Y` declares a re-export; deleting it breaks the
        // public API even with zero in-module uses.
        let d = temp("xasx");
        write(&d, "__main__.py", "import blueprints\n");
        write(
            &d,
            "blueprints.py",
            "from sansio import BlueprintSetupState as BlueprintSetupState\nfrom sansio import Blueprint as SansioBlueprint\n\nclass Blueprint(SansioBlueprint):\n    pass\n",
        );
        write(
            &d,
            "sansio.py",
            "class Blueprint:\n    pass\n\nclass BlueprintSetupState:\n    pass\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            !f.iter()
                .any(|x| x.rule == "unused-import" && x.reason.contains("BlueprintSetupState")),
            "X-as-X re-export wrongly flagged: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn try_except_probe_import_is_never_certain() {
        // Distilled from requests/tests/compat.py: both arms of an
        // availability probe bind the name; removing either changes behavior.
        let d = temp("tryimp");
        write(&d, "__main__.py", "print('hi')\n");
        write(
            &d,
            "compat.py",
            "try:\n    import StringIO\nexcept ImportError:\n    import io as StringIO\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let imps: Vec<_> = f.iter().filter(|x| x.rule == "unused-import").collect();
        for imp in &imps {
            assert_ne!(
                imp.confidence,
                Confidence::Certain,
                "try/except probe import graded certain: {imp:?}"
            );
            assert!(
                !imp.actions[0].auto_fixable,
                "try/except probe import auto-fixable: {imp:?}"
            );
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn import_consumed_by_downstream_module_counts_as_used() {
        // Distilled from requests: tests/compat.py binds a name only so that
        // sibling modules can `from .compat import` it — a re-export shim.
        let d = temp("consumer");
        write(&d, "__main__.py", "import user\n");
        write(&d, "compat.py", "import json\n");
        write(
            &d,
            "user.py",
            "from compat import json\nprint(json.dumps({}))\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(
            !f.iter().any(|x| {
                x.rule == "unused-import" && x.location.path.as_str().ends_with("compat.py")
            }),
            "re-export shim import wrongly flagged: {f:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn init_unused_import_is_uncertain_reexport() {
        let d = temp("impinit");
        write(&d, "__init__.py", "from .sub import thing\n");
        write(&d, "sub.py", "thing = 1\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let imp = f.iter().find(|x| x.rule == "unused-import");
        // Present, but never auto-fixed (re-export idiom).
        if let Some(imp) = imp {
            assert_eq!(imp.confidence, Confidence::Uncertain);
            assert!(!imp.actions[0].auto_fixable);
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn flags_unreachable_code_after_return() {
        let d = temp("unreach");
        write(&d, "__main__.py", "import lib\nlib.f()\n");
        write(
            &d,
            "lib.py",
            "def f():\n    return 1\n    print('never')\n\ndef g(x):\n    if x:\n        raise ValueError\n        cleanup()\n    return x\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let ur: Vec<_> = f.iter().filter(|x| x.rule == "unreachable-code").collect();
        // `print` after `return` is line 3; `cleanup()` after `raise` is line 8.
        assert_eq!(ur.len(), 2, "got {ur:?}");
        assert!(ur
            .iter()
            .any(|x| x.reason.contains("return") && x.location.line == 3));
        assert!(ur
            .iter()
            .any(|x| x.reason.contains("raise") && x.location.line == 8));
        assert!(ur.iter().all(|x| x.confidence == Confidence::Certain));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn flags_duplicate_reexport_in_init() {
        let d = temp("dupexport");
        write(
            &d,
            "pkg/__init__.py",
            "from .a import Thing\nfrom .b import Thing\nfrom .a import Other\n",
        );
        write(&d, "pkg/a.py", "class Thing:\n    pass\n\nOther = 1\n");
        write(&d, "pkg/b.py", "class Thing:\n    pass\n");
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        let dup: Vec<_> = f.iter().filter(|x| x.rule == "duplicate-export").collect();
        // `Thing` is re-exported from .a and .b → one duplicate at line 2.
        assert_eq!(dup.len(), 1, "got {dup:?}");
        assert!(dup[0].reason.contains("Thing") && dup[0].location.line == 2);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn dunder_all_suppresses() {
        let d = temp("all");
        write(
            &d,
            "__init__.py",
            "__all__ = ['api']\ndef api():\n    return 1\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g);
        assert!(!f.iter().any(|x| x.reason.contains("`api`")));
        std::fs::remove_dir_all(&d).ok();
    }
}
