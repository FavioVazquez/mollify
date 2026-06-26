//! Dead-code engine: reachability-based unused files and unused top-level
//! symbols, with confidence tiers (RESEARCH.md §4 / PLAN.md §4).

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_parse::DefKind;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use rustc_hash::FxHashMap;

/// Run dead-code analysis over the graph.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    unused_files(graph, &mut findings);
    unused_symbols(graph, &mut findings);
    unused_imports(graph, &mut findings);
    unused_locals(graph, &mut findings);
    unreachable_code(graph, &mut findings);
    findings
}

/// Flag statements that can never execute because they follow an unconditional
/// terminator (`return`/`raise`/`break`/`continue`/`sys.exit()`) in the same
/// block (ruff F-series / vulture parity). Syntactic and exact → `certain`, but
/// never auto-fixed (the dead statement may document intent).
fn unreachable_code(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in &graph.modules {
        for u in &m.parsed.unreachable {
            let rule = "unreachable-code";
            out.push(Finding {
                fingerprint: fingerprint(rule, &[m.path.as_str(), &u.line.to_string()]),
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
fn unused_locals(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in &graph.modules {
        for s in &m.parsed.scope_findings {
            let (rule, kind, confidence) = if s.is_param {
                ("unused-parameter", "parameter", Confidence::Uncertain)
            } else {
                ("unused-variable", "local variable", Confidence::Likely)
            };
            out.push(Finding {
                fingerprint: fingerprint(rule, &[m.path.as_str(), &s.name, &s.line.to_string()]),
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

/// Flag unused imports. A *whole-statement*-unused import (every binding unused)
/// is `certain` + auto-fixable (the line can be deleted). A *partially*-unused
/// `from x import a, b` (some names used) reports each unused name as `likely`
/// (not auto-fixed — rewriting the line precisely is left to the human). Skips
/// `import *`, `__init__.py` re-exports (downgraded), and dynamic-sink modules.
fn unused_imports(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    use rustc_hash::FxHashSet;
    for m in &graph.modules {
        let local: FxHashSet<&str> = m.parsed.local_uses.iter().map(|s| s.as_str()).collect();
        let dunder_all: Option<&Vec<String>> = m.parsed.dunder_all.as_ref();
        let is_init = m.path.file_name().is_some_and(|f| f == "__init__.py");
        for imp in &m.parsed.imports {
            if imp.is_star || imp.bindings.is_empty() || imp.type_checking_only {
                continue; // star imports / unparsed bindings / type-only: skip
            }
            let is_used = |b: &String| {
                local.contains(b.as_str()) || dunder_all.is_some_and(|all| all.contains(b))
            };
            let unused: Vec<&String> = imp.bindings.iter().filter(|b| !is_used(b)).collect();
            if unused.is_empty() {
                continue;
            }
            let whole = unused.len() == imp.bindings.len();
            let rule = "unused-import";
            if whole {
                // Entire statement unused → safe to delete the line.
                let what = format!("`{}`", imp.bindings.join("`, `"));
                let confidence = if is_init || m.parsed.has_dynamic_sink {
                    Confidence::Uncertain
                } else {
                    Confidence::Certain
                };
                out.push(Finding {
                    fingerprint: fingerprint(
                        rule,
                        &[
                            m.path.as_str(),
                            &imp.line.to_string(),
                            &imp.bindings.join(","),
                        ],
                    ),
                    rule: rule.into(),
                    category: Category::DeadCode,
                    severity: Severity::Warn,
                    confidence,
                    attribution: None,
                    reason: format!("import {what} is never used in this module"),
                    location: Location {
                        path: m.path.clone(),
                        line: imp.line,
                        column: 0,
                        end_line: None,
                    },
                    actions: vec![Action {
                        kind: "remove-import".into(),
                        description: format!("Remove the unused import {what}"),
                        auto_fixable: confidence == Confidence::Certain,
                        suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
                    }],
                });
            } else {
                // Some names still used: report each unused name (not auto-fixed,
                // since rewriting a shared import line precisely is risky).
                for name in unused {
                    out.push(Finding {
                        fingerprint: fingerprint(
                            rule,
                            &[m.path.as_str(), &imp.line.to_string(), name],
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
        out.push(Finding {
            fingerprint: fingerprint("unused-file", &[m.path.as_str()]),
            rule: "unused-file".into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason: format!(
                "module `{}` is never imported and is not an entry point",
                m.dotted
            ),
            location: Location {
                path: m.path.clone(),
                line: 1,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "remove-file".into(),
                description: format!("Delete unused module `{}`", m.path),
                auto_fixable: false, // file deletion is never auto-applied
                suppression_comment: Some("# mollify: ignore[unused-file]".into()),
            }],
        });
    }
}

fn unused_symbols(graph: &ModuleGraph, out: &mut Vec<Finding>) {
    for m in &graph.modules {
        // Count how many top-level defs share each name (to discount def sites).
        let mut def_counts: FxHashMap<&str, u32> = FxHashMap::default();
        for d in &m.parsed.definitions {
            *def_counts.entry(d.name.as_str()).or_insert(0) += 1;
        }
        let dunder_all: Option<&Vec<String>> = m.parsed.dunder_all.as_ref();

        for d in &m.parsed.definitions {
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

            // Confidence tiering.
            let confidence = if m.parsed.has_dynamic_sink {
                Confidence::Uncertain
            } else if d.private_by_convention {
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
                fingerprint: fingerprint(rule, &[m.path.as_str(), &d.name]),
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
                    // Only Certain findings are ever auto-fixable.
                    auto_fixable: confidence == Confidence::Certain,
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
    fn private_unused_is_certain_and_autofixable() {
        let d = temp("priv");
        write(&d, "__main__.py", "print('hi')\n");
        write(&d, "lib.py", "def _dead():\n    return 2\n");
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
        write(&d, "__main__.py", "print('hi')\n");
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
