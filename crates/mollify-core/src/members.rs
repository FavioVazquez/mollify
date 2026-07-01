//! Unused **class members** (methods + class-level attributes) and unused
//! **enum members** — vulture's signature signal and fallow's largest static
//! analysis area, brought to Python.
//!
//! Python member usage is dynamic (duck typing, `getattr`, serialization,
//! polymorphic overrides), so this is a *candidate producer*: every finding is
//! confidence-tiered, never auto-fixable, and guarded heavily to keep precision
//! high. The core signal is the project-wide **attribute-access set** (`obj.x`,
//! `self.x`, `Class.x`) collected by the parser — a member referenced nowhere as
//! an attribute is a strong (not certain) dead-code candidate.
//!
//! Suppressed by design: dunders, descriptors (`@property`/`@cached_property`/
//! `@*.setter`), `@staticmethod`/`@classmethod`, abstract/override/overload
//! methods, framework-registered methods, interface classes (`ABC`/`Protocol`),
//! and data-container fields (`@dataclass`, `BaseModel`, `NamedTuple`,
//! `TypedDict`, `attrs`).

use crate::fingerprint::fingerprint;
use crate::plugins::is_framework_entry_decorator;
use mollify_graph::ModuleGraph;
use mollify_parse::ClassInfo;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};
use rustc_hash::FxHashSet;

/// Detect unused class members and unused enum members across the project.
pub fn analyze(graph: &ModuleGraph) -> Vec<Finding> {
    // Project-wide signals.
    let mut attr_accessed: FxHashSet<&str> = FxHashSet::default();
    let mut referenced: FxHashSet<&str> = FxHashSet::default();
    for m in &graph.modules {
        for a in &m.parsed.attr_accessed {
            attr_accessed.insert(a.as_str());
        }
        for u in &m.parsed.local_uses {
            referenced.insert(u.as_str());
        }
        for u in &m.parsed.module_used {
            referenced.insert(u.as_str());
        }
        if let Some(all) = &m.parsed.dunder_all {
            for a in all {
                referenced.insert(a.as_str());
            }
        }
        for imp in &m.parsed.imports {
            for b in &imp.bindings {
                referenced.insert(b.as_str());
            }
        }
    }
    let dynamic = graph.global_dynamic;

    let mut out = Vec::new();
    for m in &graph.modules {
        // Disambiguates same-named (class, member) pairs across conditional
        // class redefinitions in one module.
        let mut occ = crate::fingerprint::Occurrences::default();
        for c in &m.parsed.classes {
            // A fully-unused class is reported once as `unused-export`; don't
            // also flag every member inside it.
            if !referenced.contains(c.name.as_str()) {
                continue;
            }
            if c.is_enum {
                enum_members(m, c, &attr_accessed, dynamic, &mut occ, &mut out);
            } else {
                class_members(m, c, &attr_accessed, &referenced, dynamic, &mut occ, &mut out);
            }
        }
    }
    out
}

fn is_dunder(name: &str) -> bool {
    name.starts_with("__") && name.ends_with("__")
}

/// Interface/abstract classes define members for *implementers*; their members
/// look unused locally but are part of a contract — skip them.
fn is_interface_class(c: &ClassInfo) -> bool {
    c.bases.iter().any(|b| {
        let last = b.rsplit('.').next().unwrap_or(b);
        matches!(last, "ABC" | "ABCMeta" | "Protocol")
    }) || c.decorators.iter().any(|d| {
        let last = d.rsplit('.').next().unwrap_or(d);
        last == "runtime_checkable"
    })
}

/// Data-container classes whose class-level names are *fields*, not dead code.
fn is_data_class(c: &ClassInfo) -> bool {
    c.decorators.iter().any(|d| {
        let last = d.rsplit('.').next().unwrap_or(d);
        matches!(
            last,
            "dataclass" | "define" | "frozen" | "attrs" | "attr" | "s"
        )
    }) || c.bases.iter().any(|b| {
        let last = b.rsplit('.').next().unwrap_or(b);
        matches!(last, "BaseModel" | "NamedTuple" | "TypedDict")
    })
}

/// Methods exempt from unused-member analysis: descriptors, static/class
/// methods, abstract/override/overload, and framework registrations.
fn method_exempt(decorators: &[String]) -> bool {
    decorators.iter().any(|d| {
        let last = d.rsplit('.').next().unwrap_or(d);
        matches!(
            last,
            "property"
                | "cached_property"
                | "setter"
                | "getter"
                | "deleter"
                | "staticmethod"
                | "classmethod"
                | "abstractmethod"
                | "abstractproperty"
                | "override"
                | "overload"
                | "singledispatchmethod"
        ) || is_framework_entry_decorator(d)
    })
}

fn class_members(
    m: &mollify_graph::ModuleInfo,
    c: &ClassInfo,
    attr_accessed: &FxHashSet<&str>,
    referenced: &FxHashSet<&str>,
    dynamic: bool,
    occ: &mut crate::fingerprint::Occurrences,
    out: &mut Vec<Finding>,
) {
    if is_interface_class(c) {
        return;
    }
    let path = m.path.as_path();
    let data = is_data_class(c);
    for mem in &c.members {
        let name = mem.name.as_str();
        let occurrence = occ.next(&format!("{}\u{1f}{name}", c.name));
        if is_dunder(name) || name == "_" {
            continue;
        }
        if mem.is_method {
            if method_exempt(&mem.decorators) {
                continue;
            }
            // A method is "used" if referenced anywhere as an attribute.
            if attr_accessed.contains(name) {
                continue;
            }
        } else {
            // Class-level attribute. Skip fields of data containers, and require
            // both signals to be silent (attributes can be read as bare names in
            // the class body) — precision over recall for the riskier case.
            if data || attr_accessed.contains(name) || referenced.contains(name) {
                continue;
            }
        }
        let (rule, word) = if mem.is_method {
            ("unused-method", "method")
        } else {
            ("unused-attribute", "attribute")
        };
        let confidence = if dynamic {
            Confidence::Uncertain
        } else if mem.is_private {
            Confidence::Likely
        } else {
            Confidence::Uncertain
        };
        out.push(Finding {
            fingerprint: fingerprint(rule, &[m.rel.as_str(), &c.name, name, &occurrence]),
            rule: rule.into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason: format!(
                "{word} `{}.{}` is never referenced as an attribute in the project",
                c.name, name
            ),
            location: Location {
                path: path.to_owned(),
                line: mem.line,
                column: 0,
                end_line: Some(mem.end_line),
            },
            actions: vec![Action {
                kind: format!("remove-{word}"),
                description: format!(
                    "Remove unused {word} `{}.{}` (or confirm it is an external/override API)",
                    c.name, name
                ),
                auto_fixable: false,
                suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
            }],
        });
    }
}

fn enum_members(
    m: &mollify_graph::ModuleInfo,
    c: &ClassInfo,
    attr_accessed: &FxHashSet<&str>,
    dynamic: bool,
    occ: &mut crate::fingerprint::Occurrences,
    out: &mut Vec<Finding>,
) {
    let path = m.path.as_path();
    for mem in &c.members {
        if mem.is_method {
            continue; // enum methods are regular methods; out of scope here
        }
        let name = mem.name.as_str();
        let occurrence = occ.next(&format!("{}\u{1f}{name}", c.name));
        if is_dunder(name) || name == "_" {
            continue;
        }
        // Enum machinery names are not members.
        if matches!(name, "_ignore_" | "_order_" | "_generate_next_value_") {
            continue;
        }
        if attr_accessed.contains(name) {
            continue;
        }
        // Enums are frequently accessed dynamically (`Color["RED"]`, `Color(1)`,
        // iteration, serialization) so default to `uncertain`; private → likely.
        let confidence = if !dynamic && mem.is_private {
            Confidence::Likely
        } else {
            Confidence::Uncertain
        };
        let rule = "unused-enum-member";
        out.push(Finding {
            fingerprint: fingerprint(rule, &[m.rel.as_str(), &c.name, name, &occurrence]),
            rule: rule.into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason: format!(
                "enum member `{}.{}` is never referenced (note: enums are often accessed dynamically)",
                c.name, name
            ),
            location: Location {
                path: path.to_owned(),
                line: mem.line,
                column: 0,
                end_line: Some(mem.end_line),
            },
            actions: vec![Action {
                kind: "remove-enum-member".into(),
                description: format!(
                    "Remove unused enum member `{}.{}` (or confirm dynamic/serialized use)",
                    c.name, name
                ),
                auto_fixable: false,
                suppression_comment: Some(format!("# mollify: ignore[{rule}]")),
            }],
        });
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
            std::env::temp_dir().join(format!("mollify-core-mem-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    fn run(d: &Utf8Path) -> Vec<Finding> {
        let files = discover_python_files(d);
        let g = ModuleGraph::build(d, &files);
        analyze(&g)
    }

    #[test]
    fn flags_unused_method_but_not_used_one() {
        let d = temp("methods");
        write(
            &d,
            "__main__.py",
            "from svc import Service\ns = Service()\ns.run()\n",
        );
        write(
            &d,
            "svc.py",
            "class Service:\n    def run(self):\n        return self._helper()\n\n    def _helper(self):\n        return 1\n\n    def dead(self):\n        return 2\n",
        );
        let f = run(&d);
        // `run` used (called), `_helper` used (self.), `dead` never referenced.
        assert!(
            f.iter()
                .any(|x| x.rule == "unused-method" && x.reason.contains("Service.dead")),
            "got {f:?}"
        );
        assert!(!f.iter().any(|x| x.reason.contains("Service.run")));
        assert!(!f.iter().any(|x| x.reason.contains("Service._helper")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn skips_dunder_property_and_dataclass_fields() {
        let d = temp("exempt");
        write(&d, "__main__.py", "from m import C\nC()\n");
        write(
            &d,
            "m.py",
            "from dataclasses import dataclass\n\n@dataclass\nclass C:\n    x: int = 0\n    y: int = 0\n\n    def __init__(self):\n        pass\n\n    @property\n    def val(self):\n        return self.x\n",
        );
        let f = run(&d);
        // dataclass fields x/y, __init__, and @property val must NOT be flagged.
        assert!(f.is_empty(), "data/dunder/property wrongly flagged: {f:?}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn flags_unused_enum_member() {
        let d = temp("enum");
        write(
            &d,
            "__main__.py",
            "from colors import Color\nprint(Color.RED)\n",
        );
        write(
            &d,
            "colors.py",
            "from enum import Enum\n\nclass Color(Enum):\n    RED = 1\n    GREEN = 2\n    BLUE = 3\n",
        );
        let f = run(&d);
        // RED is used; GREEN and BLUE are not.
        assert!(f
            .iter()
            .any(|x| x.rule == "unused-enum-member" && x.reason.contains("Color.GREEN")));
        assert!(f
            .iter()
            .any(|x| x.rule == "unused-enum-member" && x.reason.contains("Color.BLUE")));
        assert!(!f.iter().any(|x| x.reason.contains("Color.RED")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn skips_abstract_interface_members() {
        let d = temp("abc");
        write(&d, "__main__.py", "from base import Base\nprint(Base)\n");
        write(
            &d,
            "base.py",
            "from abc import ABC, abstractmethod\n\nclass Base(ABC):\n    @abstractmethod\n    def handle(self):\n        ...\n\n    def never_called(self):\n        return 1\n",
        );
        let f = run(&d);
        // ABC members are a contract for implementers — none flagged.
        assert!(f.is_empty(), "ABC members wrongly flagged: {f:?}");
        std::fs::remove_dir_all(&d).ok();
    }
}
