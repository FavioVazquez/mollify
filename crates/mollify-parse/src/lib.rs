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
    /// Local names this statement binds, honoring aliases: `import a.b` -> [a];
    /// `import a.b as c` -> [c]; `from m import x as y` -> [y]. Empty for `*`.
    pub bindings: Vec<String>,
    /// True for `from m import *`.
    pub is_star: bool,
    /// True if guarded by `if TYPE_CHECKING:` / `if False:` — a deliberate
    /// type-only import that must never be flagged as unused.
    pub type_checking_only: bool,
    pub line: u32,
}

/// Per-function complexity metrics (cyclomatic + cognitive) and type-annotation
/// coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionComplexity {
    pub name: String,
    pub line: u32,
    /// Last line of the function (inclusive) — for coverage range checks.
    pub end_line: u32,
    /// McCabe cyclomatic complexity (1 + decision points).
    pub cyclomatic: u32,
    /// SonarSource-style cognitive complexity (nesting-weighted).
    pub cognitive: u32,
    /// Parameters excluding `self`/`cls`.
    pub params_total: u32,
    /// Of those, how many carry a type annotation.
    pub params_annotated: u32,
    /// Whether the function has a `-> T` return annotation.
    pub return_annotated: bool,
}

/// A potential security issue detected syntactically (a *candidate*, per the
/// candidate-producer/verifier split — never a confirmed vulnerability).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityHit {
    /// Stable rule id, e.g. `dangerous-eval`, `subprocess-shell-true`.
    pub rule: &'static str,
    pub line: u32,
    pub detail: String,
}

/// A single call expression's callee text and 1-based line. The callee is the
/// surface text of the `function` field (e.g. `print`, `subprocess.run`,
/// `os.system`) — enough for declarative `forbid_call` policies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    pub callee: String,
    pub line: u32,
}

/// An unused local binding within a function scope: a local variable or a
/// parameter that is bound but never referenced again in the function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFinding {
    pub name: String,
    pub line: u32,
    /// True for a parameter, false for a local-variable assignment.
    pub is_param: bool,
}

/// A class and, per method, the set of `self.<attr>` it touches — the input to
/// the LCOM* cohesion metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassInfo {
    pub name: String,
    pub line: u32,
    /// `(method_name, set-of-instance-attributes-it-references)`.
    pub methods: Vec<(String, Vec<String>)>,
}

/// The parsed view of one Python module that the graph builds on.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub path: camino::Utf8PathBuf,
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    /// Every call expression's callee text + line (for policy enforcement).
    pub calls: Vec<CallSite>,
    /// Complexity per function/method (including nested), attributed separately.
    pub functions: Vec<FunctionComplexity>,
    /// Syntactic security candidates.
    pub security_hits: Vec<SecurityHit>,
    /// Explicit `__all__` contents if present (public-API surface for libraries).
    pub dunder_all: Option<Vec<String>>,
    /// Every identifier *used* anywhere in the module (call targets, attribute
    /// bases, names in expressions). Coarse but sufficient for reachability v1.
    pub used_names: Vec<String>,
    /// Identifiers referenced *outside* import statements — lets the unused-import
    /// engine tell "the name is actually used" from "the name only appears in its
    /// own import". Includes names extracted from string/forward-ref annotations.
    /// Sorted + deduped.
    pub local_uses: Vec<String>,
    /// Inline suppressions parsed from `# mollify: ignore[<rule>]` comments:
    /// `(line, rule)` where rule is `"*"` for a bare `# mollify: ignore`.
    pub ignores: Vec<(u32, String)>,
    /// Unused local variables / parameters discovered by per-function scope
    /// analysis (conservative: name bound but referenced nowhere else).
    pub scope_findings: Vec<ScopeFinding>,
    /// Classes with per-method instance-attribute usage (for LCOM cohesion).
    pub classes: Vec<ClassInfo>,
    /// Occurrence count per identifier across the whole module (includes the
    /// definition site). `count(name) > defs(name)` ⇒ the name is referenced,
    /// not just defined — used by the symbol-usage analysis.
    pub name_counts: std::collections::HashMap<String, u32>,
    /// True if the module contains a dynamic-dispatch sink (`getattr`, `eval`,
    /// `exec`, `__import__`, `importlib`) that should downgrade confidence.
    pub has_dynamic_sink: bool,
    /// Halstead volume `(N1+N2) * log2(n1+n2)` over the module's tokens — the
    /// volume term of the Maintainability Index (`mollify metrics`).
    pub halstead_volume: f64,
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
            calls: Vec::new(),
            functions: Vec::new(),
            security_hits: Vec::new(),
            dunder_all: None,
            used_names: Vec::new(),
            local_uses: Vec::new(),
            ignores: Vec::new(),
            scope_findings: Vec::new(),
            classes: Vec::new(),
            name_counts: std::collections::HashMap::new(),
            has_dynamic_sink: false,
            halstead_volume: halstead_volume(root, bytes),
            had_errors: root.has_error(),
        };

        // Top-level definitions and imports (module scope = direct children of
        // the `module` root, plus those guarded by `if`/`try` at top level).
        collect_top_level(root, bytes, &mut m);
        // Walk the whole tree once for used identifiers and dynamic sinks.
        collect_uses(root, bytes, &mut m);
        // Uses outside import statements (for unused-import detection).
        collect_local_uses(root, bytes, &mut m);
        // Per-function complexity.
        collect_complexity(root, bytes, &mut m);
        // Per-function unused locals / parameters.
        collect_scopes(root, bytes, &mut m);
        // Per-class method/attribute map for cohesion (LCOM).
        collect_classes(root, bytes, &mut m);
        // Security candidates.
        collect_security(root, bytes, &mut m);

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
            // A `if TYPE_CHECKING:` / `if False:` guard marks deliberately
            // type-only imports so they are never flagged as unused.
            let type_checking = node.kind() == "if_statement"
                && node
                    .child_by_field_name("condition")
                    .map(|c| {
                        let t = node_text(c, bytes);
                        t.contains("TYPE_CHECKING") || t.trim() == "False"
                    })
                    .unwrap_or(false);
            let before = m.imports.len();
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
            if type_checking {
                for imp in m.imports[before..].iter_mut() {
                    imp.type_checking_only = true;
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
            let (module, binding) = match ch.kind() {
                // `import a.b.c` binds the top-level name `a`.
                "dotted_name" => {
                    let m = node_text(ch, bytes).to_string();
                    let b = m.split('.').next().unwrap_or(&m).to_string();
                    (m, b)
                }
                // `import a.b as c` binds the alias `c`.
                "aliased_import" => {
                    let m = ch
                        .child_by_field_name("name")
                        .map(|n| node_text(n, bytes).to_string())
                        .unwrap_or_default();
                    let b = ch
                        .child_by_field_name("alias")
                        .map(|n| node_text(n, bytes).to_string())
                        .unwrap_or_else(|| m.split('.').next().unwrap_or(&m).to_string());
                    (m, b)
                }
                _ => continue,
            };
            if !module.is_empty() {
                m.imports.push(Import {
                    module,
                    relative_dots: 0,
                    names: vec![],
                    bindings: if binding.is_empty() {
                        vec![]
                    } else {
                        vec![binding]
                    },
                    is_star: false,
                    type_checking_only: false,
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
    let mut bindings = Vec::new();
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
            "dotted_name" => {
                let n = node_text(ch, bytes).to_string();
                bindings.push(n.clone());
                names.push(n);
            }
            "aliased_import" => {
                if let Some(n) = ch.child_by_field_name("name") {
                    names.push(node_text(n, bytes).to_string());
                }
                // The local binding is the alias when present, else the name.
                let bind = ch
                    .child_by_field_name("alias")
                    .or_else(|| ch.child_by_field_name("name"))
                    .map(|n| node_text(n, bytes).to_string());
                if let Some(b) = bind {
                    bindings.push(b);
                }
            }
            _ => {}
        }
    }
    m.imports.push(Import {
        module,
        relative_dots,
        names,
        bindings,
        is_star,
        type_checking_only: false,
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

/// Count (total, annotated) parameters of a function, excluding a leading
/// `self`/`cls`.
fn count_params(func: Node, bytes: &[u8]) -> (u32, u32) {
    let Some(params) = func.child_by_field_name("parameters") else {
        return (0, 0);
    };
    let mut total = 0;
    let mut annotated = 0;
    let mut first = true;
    let mut c = params.walk();
    for p in params.named_children(&mut c) {
        let is_first = first;
        first = false;
        match p.kind() {
            "identifier" => {
                // Skip a leading conventional `self`/`cls`.
                let name = node_text(p, bytes);
                if is_first && (name == "self" || name == "cls") {
                    continue;
                }
                total += 1;
            }
            "typed_parameter" | "typed_default_parameter" => {
                total += 1;
                annotated += 1;
            }
            "default_parameter" => total += 1,
            _ => {}
        }
    }
    (total, annotated.min(total))
}

fn collect_complexity(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_definition" {
            if let (Some(name), Some(body)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("body"),
            ) {
                let (params_total, params_annotated) = count_params(node, bytes);
                m.functions.push(FunctionComplexity {
                    name: node_text(name, bytes).to_string(),
                    line: node.start_position().row as u32 + 1,
                    end_line: node.end_position().row as u32 + 1,
                    cyclomatic: 1 + count_cyclo(body),
                    cognitive: count_cognitive(body, 0),
                    params_total,
                    params_annotated,
                    return_annotated: node.child_by_field_name("return_type").is_some(),
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

/// Approximate Halstead volume: classify leaf tokens into operators (anonymous
/// punctuation/keyword nodes) and operands (named identifier/literal leaves),
/// then `volume = (N1+N2) * log2(n1+n2)`. Used by the Maintainability Index.
fn halstead_volume(root: Node, bytes: &[u8]) -> f64 {
    use std::collections::HashSet;
    let mut distinct_ops: HashSet<String> = HashSet::new();
    let mut distinct_oprs: HashSet<String> = HashSet::new();
    let (mut total_ops, mut total_oprs) = (0u64, 0u64);
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let mut c = n.walk();
        let children: Vec<Node> = n.children(&mut c).collect();
        if children.is_empty() {
            // Leaf.
            let kind = n.kind();
            if kind == "comment" {
                // skip
            } else if n.is_named() {
                // identifiers and literals are operands.
                if matches!(
                    kind,
                    "identifier" | "integer" | "float" | "string" | "true" | "false" | "none"
                ) {
                    total_oprs += 1;
                    distinct_oprs.insert(node_text(n, bytes).to_string());
                }
            } else {
                // anonymous punctuation / keyword = operator.
                total_ops += 1;
                distinct_ops.insert(kind.to_string());
            }
        } else {
            for ch in children {
                stack.push(ch);
            }
        }
    }
    let vocab = (distinct_ops.len() + distinct_oprs.len()) as f64;
    let length = (total_ops + total_oprs) as f64;
    if vocab <= 1.0 {
        0.0
    } else {
        length * vocab.log2()
    }
}

/// Names whose presence makes scope analysis unreliable — skip the function.
const SCOPE_DYNAMIC: &[&str] = &["locals", "vars", "globals", "eval", "exec"];

/// Per-function unused local / parameter detection. Conservative: a binding is
/// flagged only when its name occurs exactly once in the whole function (its
/// bind site) — i.e. it is never read, augmented, deleted, or closed over.
fn collect_scopes(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_definition" {
            analyze_function_scope(node, bytes, m);
        }
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            stack.push(ch);
        }
    }
    m.scope_findings.sort_by_key(|s| s.line);
}

fn analyze_function_scope(func: Node, bytes: &[u8], m: &mut ParsedModule) {
    // Frequency of every identifier across the whole function subtree (a binding
    // referenced anywhere — including nested closures — appears more than once).
    let mut freq: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut sub = vec![func];
    while let Some(n) = sub.pop() {
        if n.kind() == "identifier" {
            *freq.entry(node_text(n, bytes).to_string()).or_insert(0) += 1;
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            sub.push(ch);
        }
    }
    // Dynamic scope access → don't claim anything is unused.
    if SCOPE_DYNAMIC.iter().any(|d| freq.contains_key(*d)) {
        return;
    }
    let decorated = func
        .parent()
        .map(|p| p.kind() == "decorated_definition")
        .unwrap_or(false);
    let fname = func
        .child_by_field_name("name")
        .map(|n| node_text(n, bytes))
        .unwrap_or("");
    let is_dunder = fname.starts_with("__") && fname.ends_with("__");

    // global/nonlocal-declared names are not locals.
    let mut declared_global: std::collections::HashSet<String> = Default::default();
    let mut gn = vec![func];
    while let Some(n) = gn.pop() {
        if matches!(n.kind(), "global_statement" | "nonlocal_statement") {
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                if ch.kind() == "identifier" {
                    declared_global.insert(node_text(ch, bytes).to_string());
                }
            }
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            gn.push(ch);
        }
    }

    // Unused parameters (Uncertain — interface conformance). Skip decorated /
    // dunder / stub-bodied functions where unused params are expected.
    let body_is_stub = func
        .child_by_field_name("body")
        .map(is_stub_body)
        .unwrap_or(true);
    if !decorated && !is_dunder && !body_is_stub {
        if let Some(params) = func.child_by_field_name("parameters") {
            let mut c = params.walk();
            let mut first = true;
            for p in params.named_children(&mut c) {
                let is_first = first;
                first = false;
                let pname = param_name(p, bytes);
                let Some(pname) = pname else { continue };
                if is_first && (pname == "self" || pname == "cls") {
                    continue;
                }
                if pname.starts_with('_') || declared_global.contains(&pname) {
                    continue;
                }
                if freq.get(&pname).copied().unwrap_or(0) == 1 {
                    m.scope_findings.push(ScopeFinding {
                        line: p.start_position().row as u32 + 1,
                        name: pname,
                        is_param: true,
                    });
                }
            }
        }
    }

    // Unused local variables: top-level `name = expr` whose name occurs once.
    if let Some(body) = func.child_by_field_name("body") {
        let mut c = body.walk();
        for stmt in body.named_children(&mut c) {
            if stmt.kind() != "expression_statement" {
                continue;
            }
            let Some(assign) = stmt.named_child(0) else {
                continue;
            };
            if assign.kind() != "assignment" {
                continue;
            }
            let Some(left) = assign.child_by_field_name("left") else {
                continue;
            };
            if left.kind() != "identifier" {
                continue; // skip tuple/attribute/subscript targets
            }
            let name = node_text(left, bytes).to_string();
            if name == "_" || declared_global.contains(&name) {
                continue;
            }
            if freq.get(&name).copied().unwrap_or(0) == 1 {
                m.scope_findings.push(ScopeFinding {
                    line: assign.start_position().row as u32 + 1,
                    name,
                    is_param: false,
                });
            }
        }
    }
}

/// The bound name of a parameter node, honoring typed/default forms. `None` for
/// `*args` / `**kwargs` (always "used" by convention).
fn param_name(p: Node, bytes: &[u8]) -> Option<String> {
    match p.kind() {
        "identifier" => Some(node_text(p, bytes).to_string()),
        "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
            let mut c = p.walk();
            let kids: Vec<Node> = p.named_children(&mut c).collect();
            kids.into_iter()
                .find(|ch| ch.kind() == "identifier")
                .map(|ch| node_text(ch, bytes).to_string())
        }
        _ => None, // list_splat_pattern / dictionary_splat_pattern
    }
}

/// Is a function body a stub (only `pass`, `...`, a docstring, or
/// `raise NotImplementedError`)? Such bodies legitimately ignore parameters.
fn is_stub_body(body: Node) -> bool {
    let mut c = body.walk();
    for stmt in body.named_children(&mut c) {
        match stmt.kind() {
            "pass_statement" => {}
            "raise_statement" => {}
            "expression_statement" => {
                let inner = stmt.named_child(0).map(|n| n.kind());
                if !matches!(inner, Some("string") | Some("ellipsis")) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Collect classes with each method's referenced `self.<attr>` set (LCOM input).
fn collect_classes(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "class_definition" {
            if let Some(ci) = class_info(node, bytes) {
                m.classes.push(ci);
            }
        }
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            stack.push(ch);
        }
    }
    m.classes.sort_by_key(|c| c.line);
}

fn class_info(class: Node, bytes: &[u8]) -> Option<ClassInfo> {
    let name = node_text(class.child_by_field_name("name")?, bytes).to_string();
    let line = class.start_position().row as u32 + 1;
    let body = class.child_by_field_name("body")?;
    let mut methods = Vec::new();
    let mut c = body.walk();
    let children: Vec<Node> = body.named_children(&mut c).collect();
    for stmt in children {
        // A method may be wrapped in `decorated_definition`.
        let func = if stmt.kind() == "function_definition" {
            Some(stmt)
        } else if stmt.kind() == "decorated_definition" {
            let mut dc = stmt.walk();
            let kids: Vec<Node> = stmt.children(&mut dc).collect();
            kids.into_iter().find(|n| n.kind() == "function_definition")
        } else {
            None
        };
        let Some(func) = func else { continue };
        let mname = func
            .child_by_field_name("name")
            .map(|n| node_text(n, bytes).to_string())
            .unwrap_or_default();
        methods.push((mname, self_attrs(func, bytes)));
    }
    Some(ClassInfo {
        name,
        line,
        methods,
    })
}

/// The distinct `self.<attr>` (and `cls.<attr>`) names referenced in a method.
fn self_attrs(func: Node, bytes: &[u8]) -> Vec<String> {
    let mut attrs: std::collections::BTreeSet<String> = Default::default();
    let mut stack = vec![func];
    while let Some(n) = stack.pop() {
        if n.kind() == "attribute" {
            if let (Some(obj), Some(attr)) = (
                n.child_by_field_name("object"),
                n.child_by_field_name("attribute"),
            ) {
                let o = node_text(obj, bytes);
                if o == "self" || o == "cls" {
                    attrs.insert(node_text(attr, bytes).to_string());
                }
            }
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            stack.push(ch);
        }
    }
    attrs.into_iter().collect()
}

const SECRET_NAMES: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "secret_key",
    "private_key",
    "auth_token",
];

fn collect_security(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "call" => security_call(node, bytes, m),
            "assignment" => security_assignment(node, bytes, m),
            _ => {}
        }
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            stack.push(ch);
        }
    }
}

fn call_func<'a>(call: Node<'a>) -> Option<Node<'a>> {
    call.child_by_field_name("function")
}

fn call_args(call: Node) -> Option<Node> {
    call.child_by_field_name("arguments")
}

/// Whether the call has a keyword argument `name` whose value text equals `val`.
fn kwarg_is(call: Node, bytes: &[u8], name: &str, val: &str) -> bool {
    let Some(args) = call_args(call) else {
        return false;
    };
    let mut c = args.walk();
    for a in args.named_children(&mut c) {
        if a.kind() == "keyword_argument" {
            let n = a.child_by_field_name("name").map(|x| node_text(x, bytes));
            let v = a.child_by_field_name("value").map(|x| node_text(x, bytes));
            if n == Some(name) && v == Some(val) {
                return true;
            }
        }
    }
    false
}

fn has_kwarg(call: Node, bytes: &[u8], name: &str) -> bool {
    let Some(args) = call_args(call) else {
        return false;
    };
    let mut c = args.walk();
    for a in args.named_children(&mut c) {
        if a.kind() == "keyword_argument"
            && a.child_by_field_name("name").map(|x| node_text(x, bytes)) == Some(name)
        {
            return true;
        }
    }
    false
}

fn first_positional_is_string(call: Node) -> bool {
    first_positional_arg(call).map(|a| a.kind()) == Some("string")
}

/// The first positional argument node of a call, if any.
fn first_positional_arg(call: Node) -> Option<Node> {
    let args = call_args(call)?;
    let mut c = args.walk();
    let kids: Vec<Node> = args.named_children(&mut c).collect();
    kids.into_iter().find(|a| a.kind() != "keyword_argument")
}

/// Does a string node contain `{...}` interpolation (i.e. an f-string)?
fn string_has_interpolation(string_node: Node) -> bool {
    let mut c = string_node.walk();
    let kids: Vec<Node> = string_node.children(&mut c).collect();
    kids.iter().any(|ch| ch.kind() == "interpolation")
}

/// Is an argument expression "dynamically built" (f-string / `%`/`+` concat /
/// `.format(...)`) rather than a static literal?
fn is_dynamic_string(arg: Node, bytes: &[u8]) -> bool {
    match arg.kind() {
        "binary_operator" => true,
        "string" => string_has_interpolation(arg),
        "call" => arg
            .child_by_field_name("function")
            .map(|fnode| node_text(fnode, bytes).ends_with(".format"))
            .unwrap_or(false),
        _ => false,
    }
}

fn security_call(call: Node, bytes: &[u8], m: &mut ParsedModule) {
    let Some(func) = call_func(call) else { return };
    let f = node_text(func, bytes);
    let last = f.rsplit('.').next().unwrap_or(f);
    let line = call.start_position().row as u32 + 1;
    let mut hit = |rule: &'static str, detail: String| {
        m.security_hits.push(SecurityHit { rule, line, detail });
    };

    if (last == "eval" || last == "exec") && !first_positional_is_string(call) {
        hit(
            "dangerous-eval",
            format!("`{f}` on a non-literal expression executes dynamic code"),
        );
    }
    if f == "yaml.load" && !has_kwarg(call, bytes, "Loader") {
        hit(
            "unsafe-yaml-load",
            "yaml.load without an explicit Loader= is unsafe; use yaml.safe_load".into(),
        );
    }
    if matches!(
        f,
        "pickle.load"
            | "pickle.loads"
            | "cPickle.load"
            | "cPickle.loads"
            | "marshal.load"
            | "marshal.loads"
            | "dill.load"
            | "dill.loads"
            | "shelve.open"
            | "jsonpickle.decode"
    ) {
        hit(
            "unsafe-deserialization",
            format!("`{f}` can execute arbitrary code on untrusted input"),
        );
    }
    if matches!(
        last,
        "call" | "run" | "Popen" | "check_output" | "check_call"
    ) && kwarg_is(call, bytes, "shell", "True")
    {
        hit(
            "subprocess-shell-true",
            "subprocess call with shell=True risks shell injection".into(),
        );
    }
    // os.system / os.popen always spawn a shell.
    if matches!(f, "os.system" | "os.popen" | "os.popen2" | "os.popen3") {
        hit(
            "subprocess-shell-true",
            format!("`{f}` runs a command through the shell; prefer subprocess with an argv list"),
        );
    }
    if kwarg_is(call, bytes, "verify", "False") {
        hit(
            "tls-verify-disabled",
            "TLS certificate verification disabled (verify=False)".into(),
        );
    }
    if matches!(f, "ssl._create_unverified_context") {
        hit(
            "tls-verify-disabled",
            "ssl._create_unverified_context disables certificate validation".into(),
        );
    }
    // Weak hashes (bandit B303/B324): md5/sha1 used directly.
    if matches!(f, "hashlib.md5" | "hashlib.sha1" | "md5.new") {
        hit(
            "weak-hash",
            format!("`{f}` is a weak hash; use sha256+ (or pass usedforsecurity=False)"),
        );
    }
    // Weak ciphers (bandit B304/B305): DES/ARC4/Blowfish or ECB mode.
    if matches!(last, "DES" | "ARC4" | "Blowfish" | "RC2" | "RC4")
        && (f.contains("Crypto") || f.contains("cryptography") || f.contains("Cipher"))
    {
        hit(
            "weak-cipher",
            format!("`{f}` is a broken/weak cipher; use AES-GCM or ChaCha20-Poly1305"),
        );
    }
    if last == "MODE_ECB" {
        hit(
            "weak-cipher",
            "ECB mode leaks plaintext structure; use an authenticated mode (GCM)".into(),
        );
    }
    // Insecure randomness for security contexts (bandit B311). Uncertain: the
    // stdlib `random` is fine for non-security use.
    if matches!(
        f,
        "random.random"
            | "random.randint"
            | "random.randrange"
            | "random.choice"
            | "random.getrandbits"
    ) {
        hit(
            "insecure-random",
            format!("`{f}` is not cryptographically secure; use the `secrets` module for tokens"),
        );
    }
    // SQL built from a dynamically-formatted string (bandit B608). Only flags
    // execute-style sinks whose query argument is f-string/concat/.format.
    if matches!(
        last,
        "execute" | "executemany" | "executescript" | "raw" | "extra"
    ) {
        if let Some(arg) = first_positional_arg(call) {
            if is_dynamic_string(arg, bytes) {
                hit(
                    "sql-injection",
                    format!(
                        "`{last}(...)` builds SQL from a dynamic string; use parameterized queries"
                    ),
                );
            }
        }
    }
    // requests/httpx call without a timeout can hang forever (bandit-adjacent).
    if matches!(
        f,
        "requests.get"
            | "requests.post"
            | "requests.put"
            | "requests.delete"
            | "requests.patch"
            | "requests.head"
            | "requests.request"
    ) && !has_kwarg(call, bytes, "timeout")
    {
        hit(
            "request-without-timeout",
            format!("`{f}` without a timeout= can block indefinitely"),
        );
    }
}

fn security_assignment(assign: Node, bytes: &[u8], m: &mut ParsedModule) {
    let Some(lhs) = assign.child_by_field_name("left") else {
        return;
    };
    if lhs.kind() != "identifier" {
        return;
    }
    let name = node_text(lhs, bytes).to_ascii_lowercase();
    if !SECRET_NAMES.iter().any(|s| name.contains(s)) {
        return;
    }
    let Some(rhs) = assign.child_by_field_name("right") else {
        return;
    };
    if rhs.kind() == "string" {
        let val = string_literal_value(rhs, bytes);
        // Skip empty or obvious placeholders / env lookups.
        if val.len() >= 4 && !val.contains("${") && !val.eq_ignore_ascii_case("changeme") {
            m.security_hits.push(SecurityHit {
                rule: "hardcoded-secret",
                line: assign.start_position().row as u32 + 1,
                detail: format!(
                    "`{}` assigned a hardcoded string literal",
                    node_text(lhs, bytes)
                ),
            });
        }
    }
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
                    m.calls.push(CallSite {
                        callee: t.to_string(),
                        line: func.start_position().row as u32 + 1,
                    });
                }
            }
            "comment" => {
                if let Some(rules) = parse_ignore_comment(node_text(node, bytes)) {
                    let line = node.start_position().row as u32 + 1;
                    for r in rules {
                        m.ignores.push((line, r));
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

/// Parse a `# mollify: ignore[rule1,rule2]` (or bare `# mollify: ignore`)
/// comment into the suppressed rule ids (`["*"]` for a bare ignore).
fn parse_ignore_comment(text: &str) -> Option<Vec<String>> {
    let t = text.trim_start_matches('#').trim();
    let rest = t.strip_prefix("mollify:")?.trim();
    let rest = rest.strip_prefix("ignore")?;
    let rest = rest.trim();
    if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let rules: Vec<String> = inner
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if rules.is_empty() {
            Some(vec!["*".into()])
        } else {
            Some(rules)
        }
    } else if rest.is_empty() {
        Some(vec!["*".into()])
    } else {
        None
    }
}

/// Collect identifiers that appear *outside* `import` / `from ... import`
/// statements, so the unused-import engine doesn't count an import's own
/// binding site as a use.
fn collect_local_uses(root: Node, bytes: &[u8], m: &mut ParsedModule) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        // Do not descend into import statements — their identifiers are bindings,
        // not uses.
        if matches!(node.kind(), "import_statement" | "import_from_statement") {
            continue;
        }
        if node.kind() == "identifier" {
            m.local_uses.push(node_text(node, bytes).to_string());
        }
        // Forward-reference / string annotations (`x: "Foo"`, `List["Bar"]`):
        // the referenced names live inside a string literal, so extract any
        // identifier-like tokens when the string sits in annotation position.
        if node.kind() == "string" && in_annotation_position(node) {
            for tok in identifier_tokens(node_text(node, bytes)) {
                m.local_uses.push(tok);
            }
        }
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            stack.push(ch);
        }
    }
    m.local_uses.sort();
    m.local_uses.dedup();
}

/// Is this node within a type-annotation context (param/return/variable type)?
/// Walks up a few ancestors looking for the annotation-bearing node kinds.
fn in_annotation_position(node: Node) -> bool {
    let mut cur = node.parent();
    for _ in 0..6 {
        let Some(n) = cur else { return false };
        match n.kind() {
            "type" | "typed_parameter" | "typed_default_parameter" => return true,
            // Stop at statement/function boundaries we know aren't annotations.
            "expression_statement" | "function_definition" | "block" | "module" => return false,
            _ => cur = n.parent(),
        }
    }
    false
}

/// Extract identifier-like tokens (`Foo`, `pkg.Bar` -> `pkg`, `Bar`) from a
/// string-literal annotation's text, skipping the quotes.
fn identifier_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if !cur.is_empty() && !cur.chars().next().unwrap().is_ascii_digit() {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if !cur.is_empty() && !cur.chars().next().unwrap().is_ascii_digit() {
        out.push(cur);
    }
    out
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
    fn detects_security_candidates() {
        let m = parse("import subprocess\npassword = \"hunter2xyz\"\nsubprocess.run(cmd, shell=True)\neval(user_input)\n");
        let rules: Vec<_> = m.security_hits.iter().map(|h| h.rule).collect();
        assert!(rules.contains(&"hardcoded-secret"), "got {rules:?}");
        assert!(rules.contains(&"subprocess-shell-true"), "got {rules:?}");
        assert!(rules.contains(&"dangerous-eval"), "got {rules:?}");
        // eval on a literal is fine
        let ok = parse("eval(\"1+1\")\n");
        assert!(!ok.security_hits.iter().any(|h| h.rule == "dangerous-eval"));
    }

    #[test]
    fn counts_type_annotations() {
        let m = parse("def f(a: int, b) -> int:\n    return a\n\nclass C:\n    def m(self, x: int):\n        return x\n");
        let f = m.functions.iter().find(|f| f.name == "f").unwrap();
        assert_eq!(f.params_total, 2);
        assert_eq!(f.params_annotated, 1);
        assert!(f.return_annotated);
        let mm = m.functions.iter().find(|f| f.name == "m").unwrap();
        assert_eq!(mm.params_total, 1, "self should be excluded");
        assert_eq!(mm.params_annotated, 1);
        assert!(!mm.return_annotated);
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
