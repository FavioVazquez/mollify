//! # mollify-parse
//!
//! Python parsing for Mollify. **Parser abstraction** so the rest of the engine
//! never touches the concrete parser directly.
//!
//! ## ADR-0001: tree-sitter today, ruff_python_parser later
//! The plan (PLAN.md §3.2) specifies building on Astral's `ruff_python_parser`
//! crates via a pinned git rev. That is **not buildable in the current
//! environment** — git dependencies from GitHub are blocked by the egress
//! policy (cargo gets HTTP 403). We therefore build on `tree-sitter-python`
//! (crates.io, compiles cleanly), the same foundation skylos and Bury use.
//! The types below (`ParsedModule`, `Definition`, `Import`) are
//! parser-agnostic, so swapping in the ruff AST later is localized to this crate.

use camino::Utf8Path;
use tree_sitter::{Node, Parser, Tree};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("failed to initialize the Python grammar")]
    Grammar,
    #[error("parser produced no tree for {0}")]
    NoTree(String),
}

/// What a top-level definition is, for dead-code granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Function,
    Class,
    /// A module-level name binding (assignment target).
    Variable,
}

/// A symbol defined at module scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub kind: DefKind,
    pub line: u32,
    pub end_line: u32,
    /// Convention: names starting with `_` are private by default.
    pub private_by_convention: bool,
    /// Decorator paths applied to this def, normalized to the callable path
    /// without call args, e.g. `app.route`, `pytest.fixture`, `staticmethod`.
    pub decorators: Vec<String>,
}

/// An `import` / `from ... import ...` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The module path, e.g. `os.path` or `mypkg.sub`. Empty for relative dots
    /// captured in `relative_dots`.
    pub module: String,
    /// Number of leading dots in a relative import (`from . import x` -> 1).
    pub relative_dots: u8,
    /// Imported names (`from m import a, b` -> [a, b]). Empty for `import m`.
    pub names: Vec<String>,
    /// True for `from m import *`.
    pub is_star: bool,
    pub line: u32,
}

/// Per-function complexity metrics (cyclomatic + cognitive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionComplexity {
    pub name: String,
    pub line: u32,
    /// McCabe cyclomatic complexity (1 + decision points).
    pub cyclomatic: u32,
    /// SonarSource-style cognitive complexity (nesting-weighted).
    pub cognitive: u32,
}

/// The parsed view of one Python module that the graph builds on.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub path: camino::Utf8PathBuf,
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    /// Complexity per function/method (including nested), attributed separately.
    pub functions: Vec<FunctionComplexity>,
    /// Explicit `__all__` contents if present (public-API surface for libraries).
    pub dunder_all: Option<Vec<String>>,
    /// Every identifier *used* anywhere in the module (call targets, attribute
    /// bases, names in expressions). Coarse but sufficient for reachability v1.
    pub used_names: Vec<String>,
    /// Occurrence count per identifier across the whole module (includes the
    /// definition site). `count(name) > defs(name)` ⇒ the name is referenced,
    /// not just defined — used by the symbol-usage analysis.
    pub name_counts: std::collections::HashMap<String, u32>,
    /// True if the module contains a dynamic-dispatch sink (`getattr`, `eval`,
    /// `exec`, `__import__`, `importlib`) that should downgrade confidence.
    pub has_dynamic_sink: bool,
    had_errors: bool,
}

impl ParsedModule {
    /// Whether tree-sitter reported syntax errors (we still extract best-effort).
    pub fn had_errors(&self) -> bool {
        self.had_errors
    }
}

/// A reusable parser handle (tree-sitter parsers are not `Sync`; create one per
/// thread / rayon task).
pub struct PyParser {
    parser: Parser,
}

impl PyParser {
    pub fn new() -> Result<Self, ParseError> {
        let mut parser = Parser::new();
        let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
        parser
            .set_language(&lang)
            .map_err(|_| ParseError::Grammar)?;
        Ok(Self { parser })
    }

    /// Parse and extract the module view.
    pub fn parse(&mut self, path: &Utf8Path, source: &str) -> Result<ParsedModule, ParseError> {
        let tree: Tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| ParseError::NoTree(path.to_string()))?;
        let root = tree.root_node();
        let bytes = source.as_bytes();

        let mut m = ParsedModule {
            path: path.to_owned(),
            definitions: Vec::new(),
            imports: Vec::new(),
            functions: Vec::new(),
            dunder_all: None,
            used_names: Vec::new(),
            name_counts: std::collections::HashMap::new(),
            has_dynamic_sink: false,
            had_errors: root.has_error(),
        };

        // Top-level definitions and imports (module scope = direct children of
        // the `module` root, plus those guarded by `if`/`try` at top level).
        collect_top_level(root, bytes, &mut m);
        // Walk the whole tree once for used identifiers and dynamic sinks.
        collect_uses(root, bytes, &mut m);
        // Per-function complexity.
        collect_complexity(root, bytes, &mut m);

        Ok(m)
    }
}

const DYNAMIC_SINKS: &[&str] = &["getattr", "setattr", "eval", "exec", "__import__"];

fn node_text<'a>(node: Node, bytes: &'a [u8]) -> &'a str {
    node.utf8_text(bytes).unwrap_or("")
}

fn collect_top_level(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        scan_stmt(child, bytes, m);
    }
}

/// Scan a module-scope statement; descends into top-level `if`/`try` blocks so
/// conditional imports (`try: import x except ImportError:`) are seen.
fn scan_stmt(node: Node, bytes: &[u8], m: &mut ParsedModule) {
    match node.kind() {
        "function_definition" => {
            if let Some(def) = function_def(node, bytes) {
                m.definitions.push(def);
            }
        }
        "class_definition" => {
            if let Some(def) = class_def(node, bytes) {
                m.definitions.push(def);
            }
        }
        "decorated_definition" => {
            let decorators = collect_decorators(node, bytes);
            if let Some(mut def) = function_def(node, bytes) {
                def.decorators = decorators;
                m.definitions.push(def);
            }
        }
        "import_statement" => parse_import(node, bytes, m, false),
        "import_from_statement" => parse_import(node, bytes, m, true),
        "expression_statement" => {
            // Detect `__all__ = [...]`.
            if let Some(assign) = node.named_child(0) {
                if assign.kind() == "assignment" {
                    maybe_dunder_all(assign, bytes, m);
                    maybe_module_var(assign, bytes, m);
                }
            }
        }
        "if_statement" | "try_statement" => {
            // Recurse into nested blocks for conditional top-level imports/defs.
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if ch.kind() == "block" {
                    let mut bc = ch.walk();
                    for stmt in ch.children(&mut bc) {
                        scan_stmt(stmt, bytes, m);
                    }
                }
            }
        }
        _ => {}
    }
}

fn function_def(node: Node, bytes: &[u8]) -> Option<Definition> {
    // `decorated_definition` wraps the real def in its last child.
    let real = if node.kind() == "decorated_definition" {
        let mut found = None;
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            if ch.kind() == "function_definition" || ch.kind() == "class_definition" {
                found = Some(ch);
            }
        }
        found?
    } else {
        node
    };
    if real.kind() == "class_definition" {
        return class_def(real, bytes);
    }
    let name = real.child_by_field_name("name")?;
    let n = node_text(name, bytes).to_string();
    Some(Definition {
        private_by_convention: n.starts_with('_'),
        name: n,
        kind: DefKind::Function,
        line: real.start_position().row as u32 + 1,
        end_line: real.end_position().row as u32 + 1,
        decorators: Vec::new(),
    })
}

/// Collect normalized decorator paths from a `decorated_definition` node.
fn collect_decorators(node: Node, bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        if ch.kind() == "decorator" {
            // Text looks like `@app.route('/x')` or `@staticmethod`.
            let raw = node_text(ch, bytes).trim_start_matches('@').trim();
            let path = raw
                .split(['(', ' ', '\n', '\t'])
                .next()
                .unwrap_or("")
                .trim();
            if !path.is_empty() {
                out.push(path.to_string());
            }
        }
    }
    out
}

fn class_def(node: Node, bytes: &[u8]) -> Option<Definition> {
    let name = node.child_by_field_name("name")?;
    let n = node_text(name, bytes).to_string();
    Some(Definition {
        private_by_convention: n.starts_with('_'),
        name: n,
        kind: DefKind::Class,
        line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        decorators: Vec::new(),
    })
}

fn parse_import(node: Node, bytes: &[u8], m: &mut ParsedModule, from: bool) {
    let line = node.start_position().row as u32 + 1;
    if !from {
        // `import a.b.c, d` -> one Import per dotted_name / aliased_import.
        let mut c = node.walk();
        for ch in node.named_children(&mut c) {
            let module = match ch.kind() {
                "dotted_name" => node_text(ch, bytes).to_string(),
                "aliased_import" => ch
                    .child_by_field_name("name")
                    .map(|n| node_text(n, bytes).to_string())
                    .unwrap_or_default(),
                _ => continue,
            };
            if !module.is_empty() {
                m.imports.push(Import {
                    module,
                    relative_dots: 0,
                    names: vec![],
                    is_star: false,
                    line,
                });
            }
        }
        return;
    }

    // from [.]*module import names | *
    let mut module = String::new();
    let mut relative_dots = 0u8;
    let mut names = Vec::new();
    let mut is_star = false;
    let mut c = node.walk();
    let mut seen_module = false;
    for ch in node.children(&mut c) {
        match ch.kind() {
            "import_prefix" => relative_dots = node_text(ch, bytes).matches('.').count() as u8,
            "relative_import" => {
                relative_dots += node_text(ch, bytes).matches('.').count() as u8;
                let mut rc = ch.walk();
                let dn = ch
                    .named_children(&mut rc)
                    .find(|n| n.kind() == "dotted_name");
                if let Some(dn) = dn {
                    module = node_text(dn, bytes).to_string();
                    seen_module = true;
                }
            }
            "dotted_name" if !seen_module => {
                module = node_text(ch, bytes).to_string();
                seen_module = true;
            }
            "wildcard_import" => is_star = true,
            "dotted_name" => names.push(node_text(ch, bytes).to_string()),
            "aliased_import" => {
                if let Some(n) = ch.child_by_field_name("name") {
                    names.push(node_text(n, bytes).to_string());
                }
            }
            _ => {}
        }
    }
    m.imports.push(Import {
        module,
        relative_dots,
        names,
        is_star,
        line,
    });
}

fn maybe_dunder_all(assign: Node, bytes: &[u8], m: &mut ParsedModule) {
    let Some(lhs) = assign.child_by_field_name("left") else {
        return;
    };
    if node_text(lhs, bytes) != "__all__" {
        return;
    }
    let Some(rhs) = assign.child_by_field_name("right") else {
        return;
    };
    if rhs.kind() == "list" || rhs.kind() == "tuple" {
        let mut names = Vec::new();
        let mut c = rhs.walk();
        for ch in rhs.named_children(&mut c) {
            if ch.kind() == "string" {
                names.push(string_literal_value(ch, bytes));
            }
        }
        m.dunder_all = Some(names);
    }
}

fn maybe_module_var(assign: Node, bytes: &[u8], m: &mut ParsedModule) {
    let Some(lhs) = assign.child_by_field_name("left") else {
        return;
    };
    if lhs.kind() == "identifier" {
        let n = node_text(lhs, bytes).to_string();
        if n == "__all__" {
            return;
        }
        m.definitions.push(Definition {
            private_by_convention: n.starts_with('_'),
            name: n,
            kind: DefKind::Variable,
            line: assign.start_position().row as u32 + 1,
            end_line: assign.end_position().row as u32 + 1,
            decorators: Vec::new(),
        });
    }
}

fn string_literal_value(node: Node, bytes: &[u8]) -> String {
    // Strip surrounding quotes via the string_content child if present.
    let mut c = node.walk();
    for ch in node.named_children(&mut c) {
        if ch.kind() == "string_content" {
            return node_text(ch, bytes).to_string();
        }
    }
    node_text(node, bytes).trim_matches(['"', '\'']).to_string()
}

/// Decision-point node kinds for cyclomatic complexity.
fn is_cyclo_node(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "elif_clause"
            | "for_statement"
            | "while_statement"
            | "except_clause"
            | "conditional_expression"
            | "assert_statement"
            | "case_clause"
            | "for_in_clause"
            | "if_clause"
            | "boolean_operator"
    )
}

fn is_nested_scope(kind: &str) -> bool {
    kind == "function_definition" || kind == "class_definition"
}

/// Count cyclomatic decision points in a subtree, NOT descending into nested
/// function/class scopes (they are attributed separately).
fn count_cyclo(node: Node) -> u32 {
    let mut count = 0;
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if is_nested_scope(child.kind()) {
            continue;
        }
        if is_cyclo_node(child.kind()) {
            count += 1;
        }
        count += count_cyclo(child);
    }
    count
}

/// Cognitive complexity with a nesting penalty. Approximation of the SonarSource
/// model: structural breaks add `1 + nesting`; `elif`/`else` and boolean
/// sequences add a flat 1; nesting increments inside loops/conditionals.
fn count_cognitive(node: Node, nesting: u32) -> u32 {
    let mut sum = 0;
    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            k if is_nested_scope(k) => {}
            "if_statement"
            | "for_statement"
            | "while_statement"
            | "except_clause"
            | "conditional_expression" => {
                sum += 1 + nesting;
                sum += count_cognitive(child, nesting + 1);
            }
            "elif_clause" | "else_clause" | "boolean_operator" => {
                sum += 1;
                sum += count_cognitive(child, nesting);
            }
            _ => sum += count_cognitive(child, nesting),
        }
    }
    sum
}

fn collect_complexity(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_definition" {
            if let (Some(name), Some(body)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("body"),
            ) {
                m.functions.push(FunctionComplexity {
                    name: node_text(name, bytes).to_string(),
                    line: node.start_position().row as u32 + 1,
                    cyclomatic: 1 + count_cyclo(body),
                    cognitive: count_cognitive(body, 0),
                });
            }
        }
        let mut c = node.walk();
        for child in node.children(&mut c) {
            stack.push(child);
        }
    }
    m.functions.sort_by_key(|f| f.line);
}

fn collect_uses(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "identifier" => {
                let name = node_text(node, bytes).to_string();
                *m.name_counts.entry(name.clone()).or_insert(0) += 1;
                m.used_names.push(name);
            }
            "call" => {
                if let Some(func) = node.child_by_field_name("function") {
                    let t = node_text(func, bytes);
                    if DYNAMIC_SINKS.contains(&t) || t.starts_with("importlib") {
                        m.has_dynamic_sink = true;
                    }
                }
            }
            _ => {}
        }
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            stack.push(ch);
        }
    }
    m.used_names.sort();
    m.used_names.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> ParsedModule {
        let mut p = PyParser::new().unwrap();
        p.parse(Utf8Path::new("m.py"), src).unwrap()
    }

    #[test]
    fn extracts_functions_and_classes() {
        let m = parse("def foo():\n    pass\n\nclass Bar:\n    pass\n");
        let names: Vec<_> = m.definitions.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"Bar"));
    }

    #[test]
    fn private_convention_detected() {
        let m = parse("def _helper():\n    pass\n");
        assert!(m.definitions[0].private_by_convention);
    }

    #[test]
    fn extracts_imports() {
        let m = parse("import os\nfrom a.b import c, d\nfrom . import e\nfrom x import *\n");
        assert!(m.imports.iter().any(|i| i.module == "os"));
        let frm = m.imports.iter().find(|i| i.module == "a.b").unwrap();
        assert_eq!(frm.names, vec!["c", "d"]);
        assert!(m.imports.iter().any(|i| i.relative_dots == 1));
        assert!(m.imports.iter().any(|i| i.is_star));
    }

    #[test]
    fn extracts_dunder_all() {
        let m = parse("__all__ = ['foo', 'bar']\n");
        assert_eq!(m.dunder_all, Some(vec!["foo".into(), "bar".into()]));
    }

    #[test]
    fn computes_complexity() {
        let m = parse("def f(x):\n    if x:\n        for i in range(x):\n            if i and x:\n                return i\n    return 0\n");
        let f = m.functions.iter().find(|f| f.name == "f").unwrap();
        assert!(f.cyclomatic >= 4, "cyclo {:?}", f.cyclomatic);
        assert!(f.cognitive >= 3, "cog {:?}", f.cognitive);
    }

    #[test]
    fn captures_decorators() {
        let m = parse(
            "import app
@app.route('/x')
def view():
    return 1
",
        );
        let d = m.definitions.iter().find(|d| d.name == "view").unwrap();
        assert!(
            d.decorators.iter().any(|x| x == "app.route"),
            "got {:?}",
            d.decorators
        );
    }

    #[test]
    fn detects_dynamic_sink() {
        let m = parse("x = getattr(obj, 'attr')\n");
        assert!(m.has_dynamic_sink);
        let m2 = parse("y = 1 + 2\n");
        assert!(!m2.has_dynamic_sink);
    }

    #[test]
    fn conditional_import_seen() {
        let m = parse("try:\n    import fast\nexcept ImportError:\n    import slow as fast\n");
        assert!(m.imports.iter().any(|i| i.module == "fast"));
    }
}
