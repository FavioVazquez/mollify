//! # mollify-parse
//!
//! Python parsing for Mollify. **Parser abstraction** so the rest of the engine
//! never touches the concrete parser directly.
//!
//! ## ADR-0001: full-fidelity ruff AST
//! Built on Astral's `ruff_python_parser` / `ruff_python_ast` (pinned git rev) —
//! the same battle-tested, error-resilient parser that powers `ruff`. The types
//! below (`ParsedModule`, `Definition`, `Import`, …) are parser-agnostic, so the
//! concrete parser remains an implementation detail confined to this crate.

use camino::Utf8Path;
use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::{walk_expr, walk_stmt, Visitor};
use ruff_python_ast::{
    Expr, ExprContext, Parameters, Stmt, StmtClassDef, StmtFunctionDef, StmtImport, StmtImportFrom,
};
use ruff_python_parser::parse_module;
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextRange, TextSize};
use std::collections::{HashMap, HashSet};

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
    /// Per-binding: true when written with a redundant alias (`import x as x`,
    /// `from m import y as y`) — the PEP 484 explicit re-export convention, so
    /// the binding is public API even with zero in-module uses.
    pub redundant: Vec<bool>,
    /// True if the statement sits in a `try:` body or an `except:` handler —
    /// the availability-probe idiom (`try: import x / except ImportError: …`).
    /// Removing either arm changes behavior, so this is never a certain fix.
    pub in_try: bool,
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

/// A single call expression's callee text and 1-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    pub callee: String,
    pub line: u32,
}

/// An unused local binding within a function scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFinding {
    pub name: String,
    pub line: u32,
    /// True for a parameter, false for a local-variable assignment.
    pub is_param: bool,
}

/// A class and, per method, the set of `self.<attr>` it touches — the input to
/// the LCOM* cohesion metric. Also carries member + base metadata for unused
/// class-member / unused enum-member detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassInfo {
    pub name: String,
    pub line: u32,
    pub end_line: u32,
    /// True if this is private by convention (`_Name`).
    pub is_private: bool,
    /// Decorator paths on the class (`dataclass`, `runtime_checkable`, …).
    pub decorators: Vec<String>,
    /// Base-class paths as written (`Enum`, `enum.IntEnum`, `BaseModel`, …).
    pub bases: Vec<String>,
    /// True if a base resolves to an `enum`-family class (Enum/IntEnum/…).
    pub is_enum: bool,
    /// `(method_name, set-of-instance-attributes-it-references)`.
    pub methods: Vec<(String, Vec<String>)>,
    /// Declared members: methods and class-level attribute/constant assignments.
    pub members: Vec<ClassMember>,
}

/// One member declared directly in a class body (a method or a class-level
/// attribute / enum value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMember {
    pub name: String,
    pub line: u32,
    pub end_line: u32,
    /// True for a `def`, false for a class-level assignment (attribute/constant).
    pub is_method: bool,
    pub is_private: bool,
    /// Decorator paths (`property`, `staticmethod`, `abstractmethod`, …).
    pub decorators: Vec<String>,
}

/// A statement that can never execute because it follows an unconditional
/// terminator (`return`/`raise`/`break`/`continue`/`sys.exit()`) in the same
/// block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreachableCode {
    pub line: u32,
    /// The terminator that makes it unreachable, e.g. `return`, `raise`.
    pub after: &'static str,
}

/// A **private type** (`_Name`) referenced in the signature of a *public*
/// function/method — an API-hygiene leak (callers can't name the type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeLeak {
    /// `func` or `Class.method`.
    pub function: String,
    /// The private type name referenced (`_Internal`).
    pub type_name: String,
    pub line: u32,
    /// True if the leak is in the return annotation (else a parameter).
    pub is_return: bool,
}

/// The parsed view of one Python module that the graph builds on.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub path: camino::Utf8PathBuf,
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    /// Imports nested inside function/class bodies (lazy/deferred imports). Kept
    /// separate from `imports` so module-scope unused-import analysis is
    /// unaffected, while dependency-usage and reachability can still see them.
    pub nested_imports: Vec<Import>,
    pub calls: Vec<CallSite>,
    pub functions: Vec<FunctionComplexity>,
    pub security_hits: Vec<SecurityHit>,
    pub dunder_all: Option<Vec<String>>,
    pub used_names: Vec<String>,
    pub local_uses: Vec<String>,
    /// Names accessed as an attribute (`obj.attr`, `self.attr`, `Class.attr`) —
    /// the precise "member used" signal for unused class / enum members (sorted,
    /// deduped). Distinct from `local_uses`, which also mixes in bare/store names
    /// that would otherwise mask an unused attribute via its own definition.
    pub attr_accessed: Vec<String>,
    /// Module-level names referenced by a **resolved** free load — i.e. a
    /// `Name` in load context whose scope resolution reaches module/global scope
    /// (not shadowed by a function-local binding, and not an attribute access).
    /// This is the precise signal for whether a top-level symbol is used
    /// internally, replacing coarse token-frequency counting. Sorted + deduped.
    pub module_used: Vec<String>,
    pub ignores: Vec<(u32, String)>,
    pub scope_findings: Vec<ScopeFinding>,
    pub classes: Vec<ClassInfo>,
    /// Statements that can never execute (follow a terminator in their block).
    pub unreachable: Vec<UnreachableCode>,
    /// Private types leaked through public function/method signatures.
    pub type_leaks: Vec<TypeLeak>,
    pub name_counts: HashMap<String, u32>,
    pub has_dynamic_sink: bool,
    /// True if the module has a top-level `if __name__ == "__main__":` guard —
    /// it's a runnable script, hence a reachability root.
    pub has_main_guard: bool,
    pub halstead_volume: f64,
    had_errors: bool,
}

impl ParsedModule {
    /// Whether the parser reported syntax errors (we still extract best-effort).
    pub fn had_errors(&self) -> bool {
        self.had_errors
    }
}

/// A reusable parser handle. The ruff parser is stateless (a free function), so
/// this is a zero-sized handle kept for API stability and ergonomic call sites.
#[derive(Default)]
pub struct PyParser;

impl PyParser {
    pub fn new() -> Result<Self, ParseError> {
        Ok(Self)
    }

    /// Parse and extract the module view.
    pub fn parse(&mut self, path: &Utf8Path, source: &str) -> Result<ParsedModule, ParseError> {
        let li = LineIndex::from_source_text(source);
        let mut m = ParsedModule {
            path: path.to_owned(),
            definitions: Vec::new(),
            imports: Vec::new(),
            nested_imports: Vec::new(),
            calls: Vec::new(),
            functions: Vec::new(),
            security_hits: Vec::new(),
            dunder_all: None,
            used_names: Vec::new(),
            local_uses: Vec::new(),
            attr_accessed: Vec::new(),
            module_used: Vec::new(),
            ignores: Vec::new(),
            scope_findings: Vec::new(),
            classes: Vec::new(),
            unreachable: Vec::new(),
            type_leaks: Vec::new(),
            name_counts: HashMap::new(),
            has_dynamic_sink: false,
            has_main_guard: false,
            halstead_volume: 0.0,
            had_errors: false,
        };

        let parsed = match parse_module(source) {
            Ok(p) => p,
            Err(_) => {
                // Catastrophic parse failure: return an empty best-effort view.
                m.had_errors = true;
                return Ok(m);
            }
        };
        m.had_errors = !parsed.errors().is_empty();
        let module = parsed.syntax();

        // Token-derived data (mirrors the old "every identifier token" model):
        // name occurrence counts, used-name set, Halstead volume, ignores, and a
        // per-position Name index for scope frequency.
        let mut name_tokens: Vec<(TextSize, &str)> = Vec::new();
        let mut h_total_ops = 0u64;
        let mut h_total_oprs = 0u64;
        let mut h_ops: HashSet<TokenKind> = HashSet::new();
        let mut h_oprs: HashSet<&str> = HashSet::new();
        for tok in parsed.tokens() {
            let kind = tok.kind();
            let text = &source[tok.range()];
            if kind == TokenKind::Name {
                *m.name_counts.entry(text.to_string()).or_insert(0) += 1;
                m.used_names.push(text.to_string());
                name_tokens.push((tok.range().start(), text));
            }
            if kind == TokenKind::Comment {
                let line = line1(&li, tok.range().start());
                if let Some(rules) = parse_ignore_comment(text) {
                    for r in rules {
                        m.ignores.push((line, r));
                    }
                }
                if let Some(rules) = parse_noqa_comment(text) {
                    for r in rules {
                        m.ignores.push((line, r));
                    }
                }
            }
            // Halstead classification.
            if is_operand(kind) {
                h_total_oprs += 1;
                h_oprs.insert(text);
            } else if !kind.is_trivia()
                && !matches!(
                    kind,
                    TokenKind::Newline
                        | TokenKind::Indent
                        | TokenKind::Dedent
                        | TokenKind::EndOfFile
                )
            {
                h_total_ops += 1;
                h_ops.insert(kind);
            }
        }
        m.used_names.sort();
        m.used_names.dedup();
        let vocab = (h_ops.len() + h_oprs.len()) as f64;
        let length = (h_total_ops + h_total_oprs) as f64;
        m.halstead_volume = if vocab <= 1.0 {
            0.0
        } else {
            length * vocab.log2()
        };

        // Top-level definitions / imports / __all__ / module vars.
        scan_top_level(&module.body, &li, false, &mut m);

        // Lazy/deferred imports inside function & class bodies (collected
        // separately — see `nested_imports`).
        let mut nested = NestedImportVisitor {
            li: &li,
            depth: 0,
            out: Vec::new(),
        };
        for stmt in &module.body {
            nested.visit_stmt(stmt);
        }
        m.nested_imports = nested.out;

        // Calls, dynamic sinks, security candidates (whole-tree walk).
        let mut main = MainVisitor { li: &li, m: &mut m };
        for stmt in &module.body {
            main.visit_stmt(stmt);
        }

        // Identifiers used outside import statements (for unused-import), plus
        // the set of attribute-accessed names (for unused class/enum members).
        let mut lu = LocalUseVisitor {
            uses: Vec::new(),
            attrs: Vec::new(),
        };
        for stmt in &module.body {
            lu.visit_stmt(stmt);
        }
        lu.uses.sort();
        lu.uses.dedup();
        m.local_uses = lu.uses;
        lu.attrs.sort();
        lu.attrs.dedup();
        m.attr_accessed = lu.attrs;

        // Scope/binding resolution: which module-level names are referenced by a
        // free load that resolves to module scope (not a shadowing local).
        let mut res = Resolver {
            scopes: Vec::new(),
            used: HashSet::new(),
        };
        for stmt in &module.body {
            res.visit_stmt(stmt);
        }
        let mut mu: Vec<String> = res.used.into_iter().collect();
        mu.sort();
        m.module_used = mu;

        // Per-function complexity, per-function scope analysis, per-class cohesion.
        let mut defs = DefVisitor {
            funcs: Vec::new(),
            classes: Vec::new(),
        };
        for stmt in &module.body {
            defs.visit_stmt(stmt);
        }
        for f in &defs.funcs {
            m.functions.push(function_complexity(f, &li));
            analyze_scope(f, &name_tokens, &mut m.scope_findings, &li);
        }
        m.functions.sort_by_key(|f| f.line);
        m.scope_findings.sort_by_key(|s| s.line);
        for c in &defs.classes {
            m.classes.push(class_info(c, &li));
        }
        m.classes.sort_by_key(|c| c.line);

        // Unreachable code: statements following an unconditional terminator in
        // any block (whole-tree walk over suites).
        let mut ur = UnreachableVisitor {
            li: &li,
            out: Vec::new(),
        };
        ur.scan(&module.body);
        for stmt in &module.body {
            ur.visit_stmt(stmt);
        }
        ur.out.sort_by_key(|u| u.line);
        ur.out.dedup();
        m.unreachable = ur.out;

        // Private-type leaks through public function/method signatures.
        scan_type_leaks(&module.body, &li, &mut m.type_leaks);
        m.type_leaks
            .sort_by(|a, b| a.line.cmp(&b.line).then(a.type_name.cmp(&b.type_name)));
        m.type_leaks.dedup();

        // Import-based weak-cipher candidates (needs the parsed import list).
        security_imports(&mut m);
        m.security_hits
            .sort_by(|a, b| a.line.cmp(&b.line).then(a.rule.cmp(b.rule)));
        m.security_hits
            .dedup_by(|a, b| a.rule == b.rule && a.line == b.line);

        Ok(m)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const DYNAMIC_SINKS: &[&str] = &["getattr", "setattr", "eval", "exec", "__import__"];

/// 1-based line for a byte offset.
fn line1(li: &LineIndex, off: TextSize) -> u32 {
    li.line_index(off).get() as u32
}

/// 1-based line of the last byte covered by `range` (for inclusive end lines).
fn end_line1(li: &LineIndex, range: TextRange) -> u32 {
    let end = range.end();
    if end > range.start() {
        line1(li, end.checked_sub(TextSize::from(1)).unwrap_or(end))
    } else {
        line1(li, end)
    }
}

/// Whether a token kind is a Halstead "operand" (identifier or literal).
fn is_operand(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Name
            | TokenKind::Int
            | TokenKind::Float
            | TokenKind::Complex
            | TokenKind::String
            | TokenKind::FStringStart
            | TokenKind::FStringMiddle
            | TokenKind::FStringEnd
            | TokenKind::True
            | TokenKind::False
            | TokenKind::None
    )
}

/// Render an attribute/name expression to a dotted path (`os.path.join`).
fn expr_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Name(n) => Some(n.id.as_str().to_string()),
        Expr::Attribute(a) => Some(format!("{}.{}", expr_path(&a.value)?, a.attr.as_str())),
        _ => None,
    }
}

/// The decorator's normalized callable path (strip any call arguments).
fn decorator_path(e: &Expr) -> Option<String> {
    match e {
        Expr::Call(c) => expr_path(&c.func),
        other => expr_path(other),
    }
}

fn is_private(name: &str) -> bool {
    name.starts_with('_')
}

// ---------------------------------------------------------------------------
// Top-level scan: definitions, imports, __all__, module vars.
// ---------------------------------------------------------------------------

fn scan_top_level(stmts: &[Stmt], li: &LineIndex, type_checking: bool, m: &mut ParsedModule) {
    for stmt in stmts {
        match stmt {
            Stmt::FunctionDef(f) => m.definitions.push(Definition {
                private_by_convention: is_private(f.name.as_str()),
                name: f.name.to_string(),
                kind: DefKind::Function,
                // The full range includes decorators; point `line` at the `def`.
                line: line1(li, f.name.range().start()),
                end_line: end_line1(li, f.range()),
                decorators: f
                    .decorator_list
                    .iter()
                    .filter_map(|d| decorator_path(&d.expression))
                    .collect(),
            }),
            Stmt::ClassDef(c) => m.definitions.push(Definition {
                private_by_convention: is_private(c.name.as_str()),
                name: c.name.to_string(),
                kind: DefKind::Class,
                line: line1(li, c.name.range().start()),
                end_line: end_line1(li, c.range()),
                decorators: c
                    .decorator_list
                    .iter()
                    .filter_map(|d| decorator_path(&d.expression))
                    .collect(),
            }),
            Stmt::Import(i) => parse_import(i, li, &mut m.imports),
            Stmt::ImportFrom(i) => {
                let mut imp = parse_import_from(i, li);
                imp.type_checking_only = type_checking;
                m.imports.push(imp);
            }
            Stmt::Assign(a) => {
                if let [Expr::Name(target)] = a.targets.as_slice() {
                    let name = target.id.as_str();
                    if name == "__all__" {
                        if let Some(items) = string_list(&a.value) {
                            m.dunder_all = Some(items);
                        }
                    } else {
                        m.definitions.push(Definition {
                            private_by_convention: is_private(name),
                            name: name.to_string(),
                            kind: DefKind::Variable,
                            line: line1(li, a.range().start()),
                            end_line: end_line1(li, a.range()),
                            decorators: Vec::new(),
                        });
                    }
                }
            }
            Stmt::AnnAssign(a) => {
                if let Expr::Name(target) = &*a.target {
                    let name = target.id.as_str();
                    if name == "__all__" {
                        if let Some(v) = &a.value {
                            if let Some(items) = string_list(v) {
                                m.dunder_all = Some(items);
                            }
                        }
                    } else {
                        m.definitions.push(Definition {
                            private_by_convention: is_private(name),
                            name: name.to_string(),
                            kind: DefKind::Variable,
                            line: line1(li, a.range().start()),
                            end_line: end_line1(li, a.range()),
                            decorators: Vec::new(),
                        });
                    }
                }
            }
            // `__all__ += [...]` extends the export list; a non-literal RHS
            // makes it unknowable, so drop to None rather than keep a wrong
            // partial list.
            Stmt::AugAssign(a) => {
                if let Expr::Name(t) = &*a.target {
                    if t.id.as_str() == "__all__" {
                        match string_list(&a.value) {
                            Some(items) => {
                                if let Some(all) = &mut m.dunder_all {
                                    all.extend(items);
                                }
                            }
                            None => m.dunder_all = None,
                        }
                    }
                }
            }
            // `__all__.extend([...])` / `__all__.append('x')` — same policy.
            Stmt::Expr(e) => {
                if let Expr::Call(c) = &*e.value {
                    match expr_path(&c.func).as_deref() {
                        Some("__all__.extend") => {
                            match c.arguments.args.first().and_then(string_list) {
                                Some(items) => {
                                    if let Some(all) = &mut m.dunder_all {
                                        all.extend(items);
                                    }
                                }
                                None => m.dunder_all = None,
                            }
                        }
                        Some("__all__.append") => match c.arguments.args.first() {
                            Some(Expr::StringLiteral(s)) => {
                                if let Some(all) = &mut m.dunder_all {
                                    all.push(s.value.to_str().to_string());
                                }
                            }
                            _ => m.dunder_all = None,
                        },
                        _ => {}
                    }
                }
            }
            // Recurse into top-level guards for conditional imports/defs.
            Stmt::If(i) => {
                if is_main_guard(&i.test) {
                    m.has_main_guard = true;
                }
                // Only the if-body executes under `if TYPE_CHECKING:`; the
                // elif/else clauses are the runtime branches. Conversely,
                // `if not TYPE_CHECKING:` makes the *else* the type-only side.
                let body_tc = type_checking || is_type_checking_guard(&i.test);
                let else_tc = type_checking || is_not_type_checking_guard(&i.test);
                let before = m.imports.len();
                scan_top_level(&i.body, li, body_tc, m);
                if body_tc {
                    for imp in m.imports[before..].iter_mut() {
                        imp.type_checking_only = true;
                    }
                }
                for clause in &i.elif_else_clauses {
                    let before = m.imports.len();
                    scan_top_level(&clause.body, li, else_tc, m);
                    if else_tc {
                        for imp in m.imports[before..].iter_mut() {
                            imp.type_checking_only = true;
                        }
                    }
                }
            }
            Stmt::Try(t) => {
                // Imports in the `try:` body or an `except:` handler are the
                // availability-probe idiom; mark them so unused-import analysis
                // never grades them certain. The `else:`/`finally:` suites run
                // unconditionally and carry no probe semantics.
                let before = m.imports.len();
                scan_top_level(&t.body, li, type_checking, m);
                for h in &t.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(eh) = h;
                    scan_top_level(&eh.body, li, type_checking, m);
                }
                for imp in m.imports[before..].iter_mut() {
                    imp.in_try = true;
                }
                scan_top_level(&t.orelse, li, type_checking, m);
                scan_top_level(&t.finalbody, li, type_checking, m);
            }
            // These suites also execute at module import time.
            Stmt::With(w) => scan_top_level(&w.body, li, type_checking, m),
            Stmt::For(f) => {
                scan_top_level(&f.body, li, type_checking, m);
                scan_top_level(&f.orelse, li, type_checking, m);
            }
            Stmt::While(w) => {
                scan_top_level(&w.body, li, type_checking, m);
                scan_top_level(&w.orelse, li, type_checking, m);
            }
            Stmt::Match(mt) => {
                for case in &mt.cases {
                    scan_top_level(&case.body, li, type_checking, m);
                }
            }
            _ => {}
        }
    }
}

/// Collects imports nested inside function/class bodies. `depth` tracks how many
/// function/class scopes deep we are; `depth > 0` means the import is lazy.
struct NestedImportVisitor<'a> {
    li: &'a LineIndex,
    depth: u32,
    out: Vec<Import>,
}

impl<'a> Visitor<'a> for NestedImportVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {
                self.depth += 1;
                walk_stmt(self, stmt);
                self.depth -= 1;
            }
            Stmt::Import(i) if self.depth > 0 => {
                parse_import(i, self.li, &mut self.out);
                walk_stmt(self, stmt);
            }
            Stmt::ImportFrom(i) if self.depth > 0 => {
                self.out.push(parse_import_from(i, self.li));
                walk_stmt(self, stmt);
            }
            _ => walk_stmt(self, stmt),
        }
    }
}

/// `if __name__ == "__main__":` (either operand order) — the module is a
/// runnable script.
fn is_main_guard(test: &Expr) -> bool {
    let Expr::Compare(c) = test else {
        return false;
    };
    if c.ops.as_ref() != [ruff_python_ast::CmpOp::Eq] || c.comparators.len() != 1 {
        return false;
    }
    let is_name = |e: &Expr| matches!(e, Expr::Name(n) if n.id.as_str() == "__name__");
    let is_main_str =
        |e: &Expr| matches!(e, Expr::StringLiteral(s) if s.value.to_str() == "__main__");
    (is_name(&c.left) && is_main_str(&c.comparators[0]))
        || (is_main_str(&c.left) && is_name(&c.comparators[0]))
}

/// `if TYPE_CHECKING:` / `if typing.TYPE_CHECKING:` / `if False:` guard.
/// Exact match only — `MY_TYPE_CHECKING_OVERRIDE` is not a guard.
fn is_type_checking_guard(test: &Expr) -> bool {
    if let Expr::BooleanLiteral(b) = test {
        return !b.value; // `if False:`
    }
    expr_path(test)
        .map(|p| p == "TYPE_CHECKING" || p.ends_with(".TYPE_CHECKING"))
        .unwrap_or(false)
}

/// `if not TYPE_CHECKING:` — the body is the runtime branch; the else clause
/// is the type-only side.
fn is_not_type_checking_guard(test: &Expr) -> bool {
    if let Expr::UnaryOp(u) = test {
        return matches!(u.op, ruff_python_ast::UnaryOp::Not) && is_type_checking_guard(&u.operand);
    }
    false
}

fn parse_import(i: &StmtImport, li: &LineIndex, out: &mut Vec<Import>) {
    let line = line1(li, i.range().start());
    for alias in &i.names {
        let module = alias.name.as_str().to_string();
        let redundant = matches!(&alias.asname, Some(a) if a.as_str() == alias.name.as_str());
        let binding = match &alias.asname {
            Some(a) => a.as_str().to_string(),
            None => module.split('.').next().unwrap_or(&module).to_string(),
        };
        if !module.is_empty() {
            let bindings = if binding.is_empty() {
                vec![]
            } else {
                vec![binding]
            };
            out.push(Import {
                module,
                relative_dots: 0,
                names: vec![],
                redundant: vec![redundant; bindings.len()],
                bindings,
                is_star: false,
                type_checking_only: false,
                in_try: false,
                line,
            });
        }
    }
}

fn parse_import_from(i: &StmtImportFrom, li: &LineIndex) -> Import {
    let line = line1(li, i.range().start());
    let module = i.module.as_ref().map(|m| m.to_string()).unwrap_or_default();
    let mut names = Vec::new();
    let mut bindings = Vec::new();
    let mut redundant = Vec::new();
    let mut is_star = false;
    for alias in &i.names {
        let name = alias.name.as_str();
        if name == "*" {
            is_star = true;
            continue;
        }
        names.push(name.to_string());
        redundant.push(matches!(&alias.asname, Some(a) if a.as_str() == name));
        bindings.push(match &alias.asname {
            Some(a) => a.as_str().to_string(),
            None => name.to_string(),
        });
    }
    Import {
        module,
        relative_dots: i.level.min(u8::MAX as u32) as u8,
        names,
        bindings,
        redundant,
        is_star,
        type_checking_only: false,
        in_try: false,
        line,
    }
}

/// Extract a list/tuple of string-literal values (for `__all__`).
fn string_list(e: &Expr) -> Option<Vec<String>> {
    let elts = match e {
        Expr::List(l) => &l.elts,
        Expr::Tuple(t) => &t.elts,
        _ => return None,
    };
    Some(
        elts.iter()
            .filter_map(|el| match el {
                Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
                _ => None,
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Complexity
// ---------------------------------------------------------------------------

fn function_complexity(f: &StmtFunctionDef, li: &LineIndex) -> FunctionComplexity {
    let (params_total, params_annotated) = count_params(&f.parameters);
    let mut cv = CycloVisitor { count: 0 };
    for s in &f.body {
        cv.visit_stmt(s);
    }
    FunctionComplexity {
        name: f.name.to_string(),
        // The full range includes decorators; point `line` at the `def`.
        line: line1(li, f.name.range().start()),
        end_line: end_line1(li, f.range()),
        cyclomatic: 1 + cv.count,
        cognitive: cog_stmts(&f.body, 0),
        params_total,
        params_annotated,
        return_annotated: f.returns.is_some(),
    }
}

fn count_params(params: &Parameters) -> (u32, u32) {
    let positional: Vec<_> = params
        .posonlyargs
        .iter()
        .chain(params.args.iter())
        .collect();
    let mut total = 0u32;
    let mut annotated = 0u32;
    for (idx, p) in positional.iter().enumerate() {
        let name = p.parameter.name.as_str();
        if idx == 0 && (name == "self" || name == "cls") {
            continue;
        }
        total += 1;
        if p.parameter.annotation.is_some() {
            annotated += 1;
        }
    }
    for p in &params.kwonlyargs {
        total += 1;
        if p.parameter.annotation.is_some() {
            annotated += 1;
        }
    }
    (total, annotated.min(total))
}

/// Cyclomatic decision-point counter; does not descend into nested scopes.
struct CycloVisitor {
    count: u32,
}
impl<'a> Visitor<'a> for CycloVisitor {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return, // attributed separately
            Stmt::If(i) => {
                self.count += 1 + i
                    .elif_else_clauses
                    .iter()
                    .filter(|c| c.test.is_some())
                    .count() as u32;
            }
            Stmt::For(_) | Stmt::While(_) => self.count += 1,
            Stmt::Try(t) => self.count += t.handlers.len() as u32,
            Stmt::Assert(_) => self.count += 1,
            Stmt::Match(mt) => self.count += mt.cases.len() as u32,
            _ => {}
        }
        walk_stmt(self, stmt);
    }
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::BoolOp(b) => self.count += (b.values.len() as u32).saturating_sub(1),
            Expr::If(_) => self.count += 1, // ternary
            Expr::ListComp(c) => self.count += comp_points(&c.generators),
            Expr::SetComp(c) => self.count += comp_points(&c.generators),
            Expr::DictComp(c) => self.count += comp_points(&c.generators),
            Expr::Generator(c) => self.count += comp_points(&c.generators),
            _ => {}
        }
        walk_expr(self, expr);
    }
}

fn comp_points(gens: &[ruff_python_ast::Comprehension]) -> u32 {
    gens.iter().map(|g| 1 + g.ifs.len() as u32).sum()
}

/// Cognitive complexity (nesting-weighted approximation of the SonarSource model).
fn cog_stmts(stmts: &[Stmt], nesting: u32) -> u32 {
    stmts.iter().map(|s| cog_stmt(s, nesting)).sum()
}

fn cog_stmt(s: &Stmt, nesting: u32) -> u32 {
    match s {
        Stmt::FunctionDef(_) | Stmt::ClassDef(_) => 0,
        Stmt::If(i) => {
            let mut c = 1 + nesting + cog_cond(&i.test);
            c += cog_stmts(&i.body, nesting + 1);
            for clause in &i.elif_else_clauses {
                c += 1; // elif/else: flat increment
                if let Some(t) = &clause.test {
                    c += cog_cond(t);
                }
                c += cog_stmts(&clause.body, nesting + 1);
            }
            c
        }
        Stmt::For(f) => {
            1 + nesting + cog_stmts(&f.body, nesting + 1) + cog_stmts(&f.orelse, nesting + 1)
        }
        Stmt::While(w) => {
            1 + nesting
                + cog_cond(&w.test)
                + cog_stmts(&w.body, nesting + 1)
                + cog_stmts(&w.orelse, nesting + 1)
        }
        Stmt::With(w) => cog_stmts(&w.body, nesting),
        Stmt::Try(t) => {
            let mut c = cog_stmts(&t.body, nesting);
            for h in &t.handlers {
                let ruff_python_ast::ExceptHandler::ExceptHandler(eh) = h;
                c += 1 + nesting + cog_stmts(&eh.body, nesting + 1);
            }
            c += cog_stmts(&t.orelse, nesting) + cog_stmts(&t.finalbody, nesting);
            c
        }
        Stmt::Match(mt) => {
            let mut c = 0;
            for case in &mt.cases {
                c += 1 + nesting + cog_stmts(&case.body, nesting + 1);
            }
            c
        }
        Stmt::Expr(e) => cog_cond(&e.value),
        Stmt::Return(r) => r.value.as_ref().map(|v| cog_cond(v)).unwrap_or(0),
        Stmt::Assign(a) => cog_cond(&a.value),
        Stmt::AugAssign(a) => cog_cond(&a.value),
        Stmt::AnnAssign(a) => a.value.as_ref().map(|v| cog_cond(v)).unwrap_or(0),
        _ => 0,
    }
}

/// Count boolean operators (+1 each) and ternaries within a condition expr.
fn cog_cond(e: &Expr) -> u32 {
    let mut v = CondVisitor { count: 0 };
    v.visit_expr(e);
    v.count
}
struct CondVisitor {
    count: u32,
}
impl<'a> Visitor<'a> for CondVisitor {
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::BoolOp(b) => self.count += (b.values.len() as u32).saturating_sub(1),
            Expr::If(_) => self.count += 1,
            _ => {}
        }
        walk_expr(self, expr);
    }
}

// ---------------------------------------------------------------------------
// Scope analysis: unused locals / parameters.
// ---------------------------------------------------------------------------

const SCOPE_DYNAMIC: &[&str] = &["locals", "vars", "globals", "eval", "exec"];

fn analyze_scope(
    f: &StmtFunctionDef,
    name_tokens: &[(TextSize, &str)],
    out: &mut Vec<ScopeFinding>,
    li: &LineIndex,
) {
    // Name-token frequency within the function's byte range (binding site + uses).
    let range = f.range();
    let mut freq: HashMap<&str, u32> = HashMap::new();
    for (off, text) in name_tokens {
        if *off >= range.start() && *off < range.end() {
            *freq.entry(*text).or_insert(0) += 1;
        }
    }
    if SCOPE_DYNAMIC.iter().any(|d| freq.contains_key(*d)) {
        return;
    }

    // global/nonlocal-declared names are not locals.
    let mut gv = GlobalVisitor {
        names: HashSet::new(),
    };
    for s in &f.body {
        gv.visit_stmt(s);
    }
    let declared_global = gv.names;

    let decorated = !f.decorator_list.is_empty();
    let fname = f.name.as_str();
    let is_dunder = fname.starts_with("__") && fname.ends_with("__");
    let stub = is_stub_body(&f.body);

    if !decorated && !is_dunder && !stub {
        let positional: Vec<_> = f
            .parameters
            .posonlyargs
            .iter()
            .chain(f.parameters.args.iter())
            .collect();
        for (idx, p) in positional.iter().enumerate() {
            let name = p.parameter.name.as_str();
            if idx == 0 && (name == "self" || name == "cls") {
                continue;
            }
            if name.starts_with('_') || declared_global.contains(name) {
                continue;
            }
            if freq.get(name).copied().unwrap_or(0) == 1 {
                out.push(ScopeFinding {
                    line: line1(li, p.parameter.range().start()),
                    name: name.to_string(),
                    is_param: true,
                });
            }
        }
        for p in &f.parameters.kwonlyargs {
            let name = p.parameter.name.as_str();
            if name.starts_with('_') || declared_global.contains(name) {
                continue;
            }
            if freq.get(name).copied().unwrap_or(0) == 1 {
                out.push(ScopeFinding {
                    line: line1(li, p.parameter.range().start()),
                    name: name.to_string(),
                    is_param: true,
                });
            }
        }
    }

    // Unused local variables: top-level `name = expr` whose name occurs once.
    for stmt in &f.body {
        if let Stmt::Assign(a) = stmt {
            if let [Expr::Name(target)] = a.targets.as_slice() {
                let name = target.id.as_str();
                if name == "_" || declared_global.contains(name) {
                    continue;
                }
                if freq.get(name).copied().unwrap_or(0) == 1 {
                    out.push(ScopeFinding {
                        line: line1(li, a.range().start()),
                        name: name.to_string(),
                        is_param: false,
                    });
                }
            }
        }
    }
}

struct GlobalVisitor {
    names: HashSet<String>,
}
impl<'a> Visitor<'a> for GlobalVisitor {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Global(g) => {
                for n in &g.names {
                    self.names.insert(n.as_str().to_string());
                }
            }
            Stmt::Nonlocal(g) => {
                for n in &g.names {
                    self.names.insert(n.as_str().to_string());
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }
}

/// Is a function body a stub (only `pass`, `...`, a docstring, or `raise ...`)?
fn is_stub_body(body: &[Stmt]) -> bool {
    body.iter().all(|s| match s {
        Stmt::Pass(_) => true,
        Stmt::Raise(_) => true,
        Stmt::Expr(e) => matches!(&*e.value, Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)),
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// Classes / cohesion.
// ---------------------------------------------------------------------------

fn class_info(c: &StmtClassDef, li: &LineIndex) -> ClassInfo {
    let mut methods = Vec::new();
    let mut members: Vec<ClassMember> = Vec::new();
    for stmt in &c.body {
        match stmt {
            Stmt::FunctionDef(f) => {
                methods.push((f.name.to_string(), self_attrs(f)));
                members.push(ClassMember {
                    name: f.name.to_string(),
                    // The full range includes decorators; point at the `def`.
                    line: line1(li, f.name.range().start()),
                    end_line: end_line1(li, f.range()),
                    is_method: true,
                    is_private: is_private(f.name.as_str()),
                    decorators: f
                        .decorator_list
                        .iter()
                        .filter_map(|d| decorator_path(&d.expression))
                        .collect(),
                });
            }
            Stmt::Assign(a) => {
                if let [Expr::Name(t)] = a.targets.as_slice() {
                    members.push(class_attr_member(t.id.as_str(), a.range(), li));
                }
            }
            Stmt::AnnAssign(a) => {
                if let Expr::Name(t) = &*a.target {
                    members.push(class_attr_member(t.id.as_str(), a.range(), li));
                }
            }
            _ => {}
        }
    }
    let bases: Vec<String> = c
        .arguments
        .as_ref()
        .map(|args| args.args.iter().filter_map(expr_path).collect())
        .unwrap_or_default();
    let is_enum = bases.iter().any(|b| {
        let last = b.rsplit('.').next().unwrap_or(b);
        matches!(
            last,
            "Enum" | "IntEnum" | "StrEnum" | "Flag" | "IntFlag" | "ReprEnum" | "EnumMeta"
        )
    });
    ClassInfo {
        name: c.name.to_string(),
        // The full range includes decorators; point `line` at the `class`.
        line: line1(li, c.name.range().start()),
        end_line: end_line1(li, c.range()),
        is_private: is_private(c.name.as_str()),
        decorators: c
            .decorator_list
            .iter()
            .filter_map(|d| decorator_path(&d.expression))
            .collect(),
        bases,
        is_enum,
        methods,
        members,
    }
}

fn class_attr_member(name: &str, range: TextRange, li: &LineIndex) -> ClassMember {
    ClassMember {
        name: name.to_string(),
        line: line1(li, range.start()),
        end_line: end_line1(li, range),
        is_method: false,
        is_private: is_private(name),
        decorators: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Unreachable code: statements after an unconditional terminator in a block.
// ---------------------------------------------------------------------------

struct UnreachableVisitor<'li> {
    li: &'li LineIndex,
    out: Vec<UnreachableCode>,
}
impl<'li> UnreachableVisitor<'li> {
    /// Inspect one suite (block) for a terminator followed by more statements.
    fn scan(&mut self, body: &[Stmt]) {
        for (i, stmt) in body.iter().enumerate() {
            if let Some(term) = terminator_kind(stmt) {
                if let Some(next) = body.get(i + 1) {
                    // Ignore a lone trailing string (rare) — still report code.
                    self.out.push(UnreachableCode {
                        line: line1(self.li, next.range().start()),
                        after: term,
                    });
                }
                break; // first terminator in the block is enough
            }
        }
    }
}
impl<'a, 'li> Visitor<'a> for UnreachableVisitor<'li> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        // Scan every nested suite, then recurse.
        match stmt {
            Stmt::FunctionDef(f) => self.scan(&f.body),
            Stmt::ClassDef(c) => self.scan(&c.body),
            Stmt::If(i) => {
                self.scan(&i.body);
                for c in &i.elif_else_clauses {
                    self.scan(&c.body);
                }
            }
            Stmt::For(f) => {
                self.scan(&f.body);
                self.scan(&f.orelse);
            }
            Stmt::While(w) => {
                self.scan(&w.body);
                self.scan(&w.orelse);
            }
            Stmt::With(w) => self.scan(&w.body),
            Stmt::Try(t) => {
                self.scan(&t.body);
                for h in &t.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(eh) = h;
                    self.scan(&eh.body);
                }
                self.scan(&t.orelse);
                self.scan(&t.finalbody);
            }
            Stmt::Match(mt) => {
                for case in &mt.cases {
                    self.scan(&case.body);
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }
}

/// If `stmt` unconditionally exits its block, return the terminator label.
fn terminator_kind(stmt: &Stmt) -> Option<&'static str> {
    match stmt {
        Stmt::Return(_) => Some("return"),
        Stmt::Raise(_) => Some("raise"),
        Stmt::Break(_) => Some("break"),
        Stmt::Continue(_) => Some("continue"),
        Stmt::Expr(e) if is_noreturn_call(&e.value) => Some("exit call"),
        _ => None,
    }
}

/// `sys.exit(...)`, `os._exit(...)`, `exit(...)`, `quit(...)` — process-ending.
fn is_noreturn_call(e: &Expr) -> bool {
    if let Expr::Call(c) = e {
        if let Some(p) = expr_path(&c.func) {
            // Exact paths only — avoids treating a user method `self.exit()` as
            // process-ending.
            return matches!(p.as_str(), "sys.exit" | "os._exit" | "exit" | "quit");
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Private-type leaks: a public function/method exposing a `_Private` type.
// ---------------------------------------------------------------------------

/// A type name is "private by convention" if it starts with a single underscore
/// (but is not a dunder like `__init__`).
fn is_private_type(name: &str) -> bool {
    name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__"))
}

fn scan_type_leaks(body: &[Stmt], li: &LineIndex, out: &mut Vec<TypeLeak>) {
    // `_T = TypeVar(...)` and friends are *intentionally* private type params,
    // not API leaks — collect and exclude them.
    let mut typevars: HashSet<String> = HashSet::new();
    collect_typevars(body, &mut typevars);
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(f) if !is_private(f.name.as_str()) => {
                collect_fn_leaks(None, f, li, &typevars, out);
            }
            Stmt::ClassDef(c) if !is_private(c.name.as_str()) => {
                for s in &c.body {
                    if let Stmt::FunctionDef(f) = s {
                        if !is_private(f.name.as_str()) {
                            collect_fn_leaks(Some(c.name.as_str()), f, li, &typevars, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Collect names bound to `TypeVar`/`ParamSpec`/`TypeVarTuple` (anywhere),
/// including under `if TYPE_CHECKING:`-style guards and try blocks.
fn collect_typevars(body: &[Stmt], out: &mut HashSet<String>) {
    for stmt in body {
        match stmt {
            Stmt::Assign(a) => {
                if let (Some(Expr::Name(t)), Expr::Call(c)) = (a.targets.first(), &*a.value) {
                    if let Some(p) = expr_path(&c.func) {
                        let last = p.rsplit('.').next().unwrap_or(&p);
                        if matches!(last, "TypeVar" | "ParamSpec" | "TypeVarTuple") {
                            out.insert(t.id.as_str().to_string());
                        }
                    }
                }
            }
            Stmt::If(i) => {
                collect_typevars(&i.body, out);
                for clause in &i.elif_else_clauses {
                    collect_typevars(&clause.body, out);
                }
            }
            Stmt::Try(t) => {
                collect_typevars(&t.body, out);
                for h in &t.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(eh) = h;
                    collect_typevars(&eh.body, out);
                }
                collect_typevars(&t.orelse, out);
                collect_typevars(&t.finalbody, out);
            }
            _ => {}
        }
    }
}

fn collect_fn_leaks(
    class: Option<&str>,
    f: &StmtFunctionDef,
    li: &LineIndex,
    typevars: &HashSet<String>,
    out: &mut Vec<TypeLeak>,
) {
    let qualified = match class {
        Some(c) => format!("{c}.{}", f.name),
        None => f.name.to_string(),
    };
    let push_leaks = |ann: &Expr, line: u32, is_return: bool, out: &mut Vec<TypeLeak>| {
        let mut idents = Vec::new();
        annotation_idents(ann, &mut idents);
        for id in idents {
            if is_private_type(&id) && !typevars.contains(&id) {
                out.push(TypeLeak {
                    function: qualified.clone(),
                    type_name: id,
                    line,
                    is_return,
                });
            }
        }
    };
    for p in f
        .parameters
        .posonlyargs
        .iter()
        .chain(f.parameters.args.iter())
        .chain(f.parameters.kwonlyargs.iter())
    {
        if let Some(ann) = &p.parameter.annotation {
            push_leaks(ann, line1(li, p.parameter.range().start()), false, out);
        }
    }
    if let Some(r) = &f.returns {
        // Point at the `def` line, not the first decorator.
        push_leaks(r, line1(li, f.name.range().start()), true, out);
    }
}

/// Collect type-name identifiers referenced in an annotation expression,
/// descending through subscripts/unions/strings (`Optional[_Foo]`, `_A | _B`,
/// `"_Forward"`, `mod._Priv`).
fn annotation_idents(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Name(n) => out.push(n.id.as_str().to_string()),
        Expr::Attribute(a) => {
            annotation_idents(&a.value, out);
            out.push(a.attr.as_str().to_string());
        }
        Expr::Subscript(s) => {
            annotation_idents(&s.value, out);
            annotation_idents(&s.slice, out);
        }
        Expr::Tuple(t) => t.elts.iter().for_each(|el| annotation_idents(el, out)),
        Expr::List(l) => l.elts.iter().for_each(|el| annotation_idents(el, out)),
        Expr::BinOp(b) => {
            annotation_idents(&b.left, out);
            annotation_idents(&b.right, out);
        }
        Expr::StringLiteral(s) => {
            for tok in identifier_tokens(s.value.to_str()) {
                out.push(tok);
            }
        }
        _ => {}
    }
}

fn self_attrs(f: &StmtFunctionDef) -> Vec<String> {
    let mut v = SelfAttrVisitor {
        attrs: std::collections::BTreeSet::new(),
    };
    for s in &f.body {
        v.visit_stmt(s);
    }
    v.attrs.into_iter().collect()
}

struct SelfAttrVisitor {
    attrs: std::collections::BTreeSet<String>,
}
impl<'a> Visitor<'a> for SelfAttrVisitor {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Attribute(a) = expr {
            if let Expr::Name(obj) = &*a.value {
                if obj.id.as_str() == "self" || obj.id.as_str() == "cls" {
                    self.attrs.insert(a.attr.as_str().to_string());
                }
            }
        }
        walk_expr(self, expr);
    }
}

// ---------------------------------------------------------------------------
// Collect definitions of nested functions/classes (whole tree).
// ---------------------------------------------------------------------------

struct DefVisitor<'a> {
    funcs: Vec<&'a StmtFunctionDef>,
    classes: Vec<&'a StmtClassDef>,
}
impl<'a> Visitor<'a> for DefVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(f) => self.funcs.push(f),
            Stmt::ClassDef(c) => self.classes.push(c),
            _ => {}
        }
        walk_stmt(self, stmt);
    }
}

// ---------------------------------------------------------------------------
// Local uses (identifiers outside import statements + string annotations).
// ---------------------------------------------------------------------------

struct LocalUseVisitor {
    uses: Vec<String>,
    /// Attribute names accessed (`obj.attr`) — the "member used" signal.
    attrs: Vec<String>,
}
impl<'a> Visitor<'a> for LocalUseVisitor {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        // Import bindings are not "uses".
        if matches!(stmt, Stmt::Import(_) | Stmt::ImportFrom(_)) {
            return;
        }
        // String forward-ref annotations: extract identifier tokens.
        if let Stmt::AnnAssign(a) = stmt {
            collect_annotation_strings(&a.annotation, &mut self.uses);
            // A quoted TypeAlias *value* is type syntax too:
            // `_P: TypeAlias = 'partial[Any] | partialmethod[Any]'` uses
            // `partial`/`partialmethod` (type checkers — and pydantic at
            // runtime — evaluate the string). Found live on pydantic, where
            // the import was wrongly certain + auto-fixable.
            if is_type_alias_annotation(&a.annotation) {
                if let Some(v) = &a.value {
                    collect_annotation_strings(v, &mut self.uses);
                }
            }
        }
        if let Stmt::FunctionDef(f) = stmt {
            if let Some(r) = &f.returns {
                collect_annotation_strings(r, &mut self.uses);
            }
            for p in f
                .parameters
                .posonlyargs
                .iter()
                .chain(f.parameters.args.iter())
                .chain(f.parameters.kwonlyargs.iter())
            {
                if let Some(ann) = &p.parameter.annotation {
                    collect_annotation_strings(ann, &mut self.uses);
                }
            }
        }
        walk_stmt(self, stmt);
    }
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(n) => self.uses.push(n.id.as_str().to_string()),
            Expr::Attribute(a) => {
                self.uses.push(a.attr.as_str().to_string());
                self.attrs.push(a.attr.as_str().to_string());
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// `TypeAlias` / `typing.TypeAlias` / `typing_extensions.TypeAlias` as an
/// AnnAssign annotation — marks the assigned value as type syntax.
fn is_type_alias_annotation(e: &Expr) -> bool {
    expr_path(e)
        .map(|p| p == "TypeAlias" || p.ends_with(".TypeAlias"))
        .unwrap_or(false)
}

/// Pull identifier-like tokens out of any string literal inside an annotation
/// expression (`x: "Foo"`, `List["pkg.Bar"]`), plus referenced Names.
fn collect_annotation_strings(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::StringLiteral(s) => {
            for tok in identifier_tokens(s.value.to_str()) {
                out.push(tok);
            }
        }
        Expr::Subscript(s) => {
            collect_annotation_strings(&s.value, out);
            collect_annotation_strings(&s.slice, out);
        }
        Expr::Tuple(t) => {
            for el in &t.elts {
                collect_annotation_strings(el, out);
            }
        }
        Expr::List(l) => {
            for el in &l.elts {
                collect_annotation_strings(el, out);
            }
        }
        Expr::BinOp(b) => {
            collect_annotation_strings(&b.left, out);
            collect_annotation_strings(&b.right, out);
        }
        _ => {}
    }
}

fn identifier_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() && !cur.chars().next().unwrap().is_ascii_digit() {
            out.push(std::mem::take(cur));
        } else {
            cur.clear();
        }
    };
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Scope/binding resolution.
//
// A real (if compact) LEGB resolver: it tracks a stack of *function* scopes,
// each with its statically-determined local bindings (Python's rule: a name
// assigned anywhere in a function body is local to it, unless declared
// `global`). A `Name` load resolves to module/global scope when no enclosing
// function scope binds it. `global x` forces module resolution; `nonlocal x`
// binds to an enclosing function (treated as local-here so it never bubbles to
// module). Class bodies are transparent to nested functions, matching Python.
// ---------------------------------------------------------------------------

struct FnScope {
    locals: HashSet<String>,
    globals: HashSet<String>,
}

struct Resolver {
    scopes: Vec<FnScope>,
    used: HashSet<String>,
}

impl Resolver {
    fn resolve_load(&mut self, name: &str) {
        for s in self.scopes.iter().rev() {
            if s.globals.contains(name) {
                self.used.insert(name.to_string()); // `global` → module binding
                return;
            }
            if s.locals.contains(name) {
                return; // bound by an enclosing function scope
            }
        }
        // Not bound by any function scope → module/global scope.
        self.used.insert(name.to_string());
    }

    fn enter_function(&mut self, f: &StmtFunctionDef) {
        let mut bv = BindingVisitor {
            locals: HashSet::new(),
            globals: HashSet::new(),
        };
        for p in param_names(&f.parameters) {
            bv.locals.insert(p);
        }
        for stmt in &f.body {
            bv.visit_stmt(stmt);
        }
        // `global` names are not locals.
        for g in &bv.globals {
            bv.locals.remove(g);
        }
        self.scopes.push(FnScope {
            locals: bv.locals,
            globals: bv.globals,
        });
    }

    /// Parameter defaults and annotations (and the return annotation) evaluate
    /// in the *enclosing* scope, before the function/lambda scope exists.
    fn visit_signature_exprs(&mut self, params: &Parameters) {
        for p in params
            .posonlyargs
            .iter()
            .chain(params.args.iter())
            .chain(params.kwonlyargs.iter())
        {
            if let Some(d) = &p.default {
                self.visit_expr(d);
            }
            if let Some(a) = &p.parameter.annotation {
                self.visit_expr(a);
            }
        }
        if let Some(v) = &params.vararg {
            if let Some(a) = &v.annotation {
                self.visit_expr(a);
            }
        }
        if let Some(k) = &params.kwarg {
            if let Some(a) = &k.annotation {
                self.visit_expr(a);
            }
        }
    }
}

impl<'a> Visitor<'a> for Resolver {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(f) => {
                // Decorators / default values / annotations resolve in the
                // current scope (visited before the function scope is pushed).
                for d in &f.decorator_list {
                    self.visit_expr(&d.expression);
                }
                self.visit_signature_exprs(&f.parameters);
                if let Some(r) = &f.returns {
                    self.visit_expr(r);
                }
                self.enter_function(f);
                for stmt in &f.body {
                    self.visit_stmt(stmt);
                }
                self.scopes.pop();
            }
            Stmt::ClassDef(c) => {
                for d in &c.decorator_list {
                    self.visit_expr(&d.expression);
                }
                if let Some(args) = &c.arguments {
                    for a in args.args.iter() {
                        self.visit_expr(a);
                    }
                    for kw in args.keywords.iter() {
                        self.visit_expr(&kw.value);
                    }
                }
                // Class body is transparent (its bindings are Stores, not loads).
                for stmt in &c.body {
                    self.visit_stmt(stmt);
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(n) => {
                if matches!(n.ctx, ExprContext::Load) {
                    self.resolve_load(n.id.as_str());
                }
            }
            Expr::Lambda(l) => {
                let mut locals = HashSet::new();
                if let Some(params) = &l.parameters {
                    // Defaults resolve in the enclosing scope, not the lambda's.
                    self.visit_signature_exprs(params);
                    for p in param_names(params) {
                        locals.insert(p);
                    }
                }
                self.scopes.push(FnScope {
                    locals,
                    globals: HashSet::new(),
                });
                self.visit_expr(&l.body);
                self.scopes.pop();
            }
            _ => walk_expr(self, expr),
        }
    }
}

/// Collect a function scope's local bindings (Store names, nested def/class
/// names, `global`/`nonlocal` declarations) without descending into nested
/// function/class/lambda scopes.
struct BindingVisitor {
    locals: HashSet<String>,
    globals: HashSet<String>,
}
impl<'a> Visitor<'a> for BindingVisitor {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(f) => {
                self.locals.insert(f.name.to_string());
            }
            Stmt::ClassDef(c) => {
                self.locals.insert(c.name.to_string());
            }
            Stmt::Global(g) => {
                for n in &g.names {
                    self.globals.insert(n.to_string());
                }
            }
            Stmt::Nonlocal(g) => {
                for n in &g.names {
                    // nonlocal binds to an enclosing function — never module.
                    self.locals.insert(n.to_string());
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(n) if matches!(n.ctx, ExprContext::Store) => {
                self.locals.insert(n.id.as_str().to_string());
            }
            // Don't descend into nested scopes: their bindings aren't ours.
            // Python 3 comprehensions have their own scope, so their targets
            // are not locals here either. (Their iterables/conditions do
            // evaluate in this scope, but they only load, never bind.)
            Expr::Lambda(_)
            | Expr::ListComp(_)
            | Expr::SetComp(_)
            | Expr::DictComp(_)
            | Expr::Generator(_) => {}
            _ => walk_expr(self, expr),
        }
    }
}

fn param_names(params: &Parameters) -> Vec<String> {
    let mut out = Vec::new();
    for p in params
        .posonlyargs
        .iter()
        .chain(params.args.iter())
        .chain(params.kwonlyargs.iter())
    {
        out.push(p.parameter.name.as_str().to_string());
    }
    if let Some(v) = &params.vararg {
        out.push(v.name.as_str().to_string());
    }
    if let Some(k) = &params.kwarg {
        out.push(k.name.as_str().to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// Calls, dynamic sinks, security (whole tree).
// ---------------------------------------------------------------------------

struct MainVisitor<'a, 'm> {
    li: &'a LineIndex,
    m: &'m mut ParsedModule,
}
impl<'a, 'm> Visitor<'a> for MainVisitor<'a, 'm> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Assign(a) => {
                if let [Expr::Name(t)] = a.targets.as_slice() {
                    security_secret(t.id.as_str(), &a.value, a.range(), self.li, self.m);
                }
            }
            Stmt::AnnAssign(a) => {
                if let (Expr::Name(t), Some(v)) = (&*a.target, &a.value) {
                    security_secret(t.id.as_str(), v, a.range(), self.li, self.m);
                }
            }
            Stmt::Try(t) => {
                // try/except/pass (B110): a broad handler that silently swallows
                // errors. Only flag bare `except:` or `except Exception/BaseException`.
                for h in &t.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(eh) = h;
                    let broad = match &eh.type_ {
                        None => true,
                        Some(ty) => expr_path(ty)
                            .map(|p| {
                                matches!(
                                    p.rsplit('.').next().unwrap_or(&p),
                                    "Exception" | "BaseException"
                                )
                            })
                            .unwrap_or(false),
                    };
                    if broad && eh.body.iter().all(|s| matches!(s, Stmt::Pass(_))) {
                        self.m.security_hits.push(SecurityHit {
                            rule: "try-except-pass",
                            line: line1(self.li, eh.range().start()),
                            detail:
                                "broad `except: pass` silently swallows errors; log or handle them"
                                    .into(),
                        });
                    }
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(c) = expr {
            let callee = expr_path(&c.func).unwrap_or_default();
            if !callee.is_empty() {
                if DYNAMIC_SINKS.contains(&callee.as_str()) || callee.starts_with("importlib") {
                    self.m.has_dynamic_sink = true;
                }
                self.m.calls.push(CallSite {
                    callee: callee.clone(),
                    line: line1(self.li, c.func.range().start()),
                });
            }
            security_call(c, &callee, line1(self.li, c.range().start()), self.m);
        }
        walk_expr(self, expr);
    }
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

fn security_secret(
    name: &str,
    value: &Expr,
    range: TextRange,
    li: &LineIndex,
    m: &mut ParsedModule,
) {
    let lname = name.to_ascii_lowercase();
    if !SECRET_NAMES.iter().any(|s| lname.contains(s)) {
        return;
    }
    if let Expr::StringLiteral(s) = value {
        let val = s.value.to_str();
        if val.len() >= 4 && !val.contains("${") && !val.eq_ignore_ascii_case("changeme") {
            m.security_hits.push(SecurityHit {
                rule: "hardcoded-secret",
                line: line1(li, range.start()),
                detail: format!("`{name}` assigned a hardcoded string literal"),
            });
        }
    }
}

const WEAK_CIPHERS: &[&str] = &[
    "DES",
    "DES3",
    "TripleDES",
    "ARC2",
    "RC2",
    "ARC4",
    "RC4",
    "Blowfish",
    "IDEA",
    "CAST",
    "XOR",
];

fn kwarg_bool(c: &ruff_python_ast::ExprCall, name: &str, want: bool) -> bool {
    c.arguments
        .find_keyword(name)
        .map(|kw| matches!(&kw.value, Expr::BooleanLiteral(b) if b.value == want))
        .unwrap_or(false)
}

fn has_kwarg(c: &ruff_python_ast::ExprCall, name: &str) -> bool {
    c.arguments.find_keyword(name).is_some()
}

fn first_positional_is_string(c: &ruff_python_ast::ExprCall) -> bool {
    matches!(c.arguments.args.first(), Some(Expr::StringLiteral(_)))
}

fn is_dynamic_string(arg: &Expr) -> bool {
    match arg {
        Expr::FString(_) => true,
        Expr::BinOp(_) => true,
        Expr::Call(c) => expr_path(&c.func)
            .map(|p| p.ends_with(".format"))
            .unwrap_or(false),
        _ => false,
    }
}

/// Does any argument reference `.MODE_ECB`?
fn args_reference_ecb(c: &ruff_python_ast::ExprCall) -> bool {
    let refs = |e: &Expr| {
        expr_path(e)
            .map(|p| p.contains("MODE_ECB"))
            .unwrap_or(false)
    };
    c.arguments.args.iter().any(refs) || c.arguments.keywords.iter().any(|k| refs(&k.value))
}

fn security_call(c: &ruff_python_ast::ExprCall, f: &str, line: u32, m: &mut ParsedModule) {
    let last = f.rsplit('.').next().unwrap_or(f);
    let mut hit = |rule: &'static str, detail: String| {
        m.security_hits.push(SecurityHit { rule, line, detail });
    };

    // Only the *builtins* eval/exec/compile — bare names, or explicitly via
    // `builtins.`. Matching any trailing `.exec`/`.eval` segment falsely flagged
    // ORM/driver methods like SQLModel's `session.exec(select(...))` (CWE-95 FP).
    if matches!(
        f,
        "eval" | "exec" | "compile" | "builtins.eval" | "builtins.exec" | "builtins.compile"
    ) && !first_positional_is_string(c)
    {
        hit(
            "dangerous-eval",
            format!("`{f}` on a non-literal expression executes dynamic code"),
        );
    }
    if f == "yaml.load" && !has_kwarg(c, "Loader") {
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
    ) && kwarg_bool(c, "shell", true)
    {
        hit(
            "subprocess-shell-true",
            "subprocess call with shell=True risks shell injection".into(),
        );
    }
    if matches!(f, "os.system" | "os.popen" | "os.popen2" | "os.popen3") {
        hit(
            "subprocess-shell-true",
            format!("`{f}` runs a command through the shell; prefer subprocess with an argv list"),
        );
    }
    if kwarg_bool(c, "verify", false) {
        hit(
            "tls-verify-disabled",
            "TLS certificate verification disabled (verify=False)".into(),
        );
    }
    if f == "ssl._create_unverified_context" {
        hit(
            "tls-verify-disabled",
            "ssl._create_unverified_context disables certificate validation".into(),
        );
    }
    if matches!(f, "hashlib.md5" | "hashlib.sha1" | "md5.new") {
        hit(
            "weak-hash",
            format!("`{f}` is a weak hash; use sha256+ (or pass usedforsecurity=False)"),
        );
    }
    if WEAK_CIPHERS.contains(&last) {
        hit(
            "weak-cipher",
            format!("`{f}` is a broken/weak cipher; use AES-GCM or ChaCha20-Poly1305"),
        );
    }
    if args_reference_ecb(c) {
        hit(
            "weak-cipher",
            "ECB mode leaks plaintext structure; use an authenticated mode (GCM)".into(),
        );
    }
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
    if matches!(
        last,
        "execute" | "executemany" | "executescript" | "raw" | "extra"
    ) {
        if let Some(arg) = c.arguments.args.first() {
            if is_dynamic_string(arg) {
                hit(
                    "sql-injection",
                    format!(
                        "`{last}(...)` builds SQL from a dynamic string; use parameterized queries"
                    ),
                );
            }
        }
    }
    if matches!(
        f,
        "requests.get"
            | "requests.post"
            | "requests.put"
            | "requests.delete"
            | "requests.patch"
            | "requests.head"
            | "requests.request"
    ) && !has_kwarg(c, "timeout")
    {
        hit(
            "request-without-timeout",
            format!("`{f}` without a timeout= can block indefinitely"),
        );
    }
    // Flask/Bottle debug server (B201): `app.run(debug=True)` ships the
    // interactive debugger (RCE) in production.
    if last == "run" && kwarg_bool(c, "debug", true) {
        hit(
            "flask-debug-true",
            "running a web app with debug=True exposes the interactive debugger".into(),
        );
    }
    // Jinja2 without autoescaping (B701): `Environment(autoescape=False)` (or the
    // implicit default) risks XSS.
    if last == "Environment" && kwarg_bool(c, "autoescape", false) {
        hit(
            "jinja2-autoescape-false",
            "Jinja2 Environment with autoescape=False risks XSS; enable autoescaping".into(),
        );
    }
}

fn security_imports(m: &mut ParsedModule) {
    let mut hits: Vec<SecurityHit> = Vec::new();
    for imp in m.imports.iter().chain(m.nested_imports.iter()) {
        let from_crypto = imp.module.contains("Crypto") || imp.module.contains("cryptography");
        if !from_crypto {
            continue;
        }
        for name in &imp.names {
            if WEAK_CIPHERS.contains(&name.as_str()) {
                hits.push(SecurityHit {
                    rule: "weak-cipher",
                    line: imp.line,
                    detail: format!(
                        "`{name}` (imported from `{}`) is a broken/weak cipher; use AES-GCM or ChaCha20-Poly1305",
                        imp.module
                    ),
                });
            }
        }
        if imp.names.is_empty() {
            if let Some(seg) = imp.module.rsplit('.').next() {
                if WEAK_CIPHERS.contains(&seg) {
                    hits.push(SecurityHit {
                        rule: "weak-cipher",
                        line: imp.line,
                        detail: format!(
                            "`{}` is a broken/weak cipher; use AES-GCM or ChaCha20-Poly1305",
                            imp.module
                        ),
                    });
                }
            }
        }
    }
    m.security_hits.extend(hits);
}

/// Parse a `# mollify: ignore[rule1,rule2]` comment into suppressed rule ids.
/// Trailing text after the closing bracket (e.g. `-- reason`) is allowed.
/// Map a flake8 `# noqa` comment to the mollify rules it suppresses. These are
/// author-deliberate exceptions that predate mollify, so honoring them kills a
/// whole class of false positives (`from hello import app  # noqa: F401`).
/// Scope is intentionally narrow — only the unused-binding rules that flake8's
/// F401/F841 correspond to; a blanket `# noqa` suppresses both. Other codes are
/// other tools' semantics and are not interpreted.
fn parse_noqa_comment(text: &str) -> Option<Vec<String>> {
    let t = text.trim_start_matches('#').trim();
    if t.len() < 4 || !t.is_char_boundary(4) || !t[..4].eq_ignore_ascii_case("noqa") {
        return None;
    }
    let rest = t[4..].trim_start();
    if rest.is_empty() || rest.starts_with('#') {
        return Some(vec!["unused-import".into(), "unused-variable".into()]);
    }
    let codes = rest.strip_prefix(':')?;
    let mut rules = Vec::new();
    for code in codes.split([',', ' ', '#']).map(str::trim) {
        match code.to_ascii_uppercase().as_str() {
            "F401" => rules.push("unused-import".to_string()),
            "F841" => rules.push("unused-variable".to_string()),
            _ => {}
        }
    }
    if rules.is_empty() {
        None
    } else {
        Some(rules)
    }
}

fn parse_ignore_comment(text: &str) -> Option<Vec<String>> {
    let t = text.trim_start_matches('#').trim();
    let rest = t.strip_prefix("mollify:")?.trim();
    let rest = rest.strip_prefix("ignore")?.trim();
    if let Some(inner) = rest
        .strip_prefix('[')
        .and_then(|r| r.find(']').map(|i| &r[..i]))
    {
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
    fn detects_expanded_security_rules() {
        let m = parse(
            "app.run(debug=True)\nenv = Environment(autoescape=False)\ntry:\n    risky()\nexcept Exception:\n    pass\n",
        );
        let rules: Vec<_> = m.security_hits.iter().map(|h| h.rule).collect();
        assert!(rules.contains(&"flask-debug-true"), "got {rules:?}");
        assert!(rules.contains(&"jinja2-autoescape-false"), "got {rules:?}");
        assert!(rules.contains(&"try-except-pass"), "got {rules:?}");
        // A narrow `except ValueError: pass` must NOT be flagged.
        let narrow = parse("try:\n    x()\nexcept ValueError:\n    pass\n");
        assert!(!narrow
            .security_hits
            .iter()
            .any(|h| h.rule == "try-except-pass"));
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
        let ok = parse("eval(\"1+1\")\n");
        assert!(!ok.security_hits.iter().any(|h| h.rule == "dangerous-eval"));
    }

    #[test]
    fn dangerous_eval_only_matches_builtins_not_methods() {
        // Methods named exec/eval on ORMs/drivers (SQLModel session.exec, etc.)
        // must NOT be flagged — that was the v0.1.2 CWE-95 false positive.
        for src in [
            "session.exec(select(Item))\n",
            "conn.exec(query)\n",
            "obj.eval(expr)\n",
            "db.compile(stmt)\n",
        ] {
            let m = parse(src);
            assert!(
                !m.security_hits.iter().any(|h| h.rule == "dangerous-eval"),
                "method call wrongly flagged: {src}"
            );
        }
        // Bare builtins on a non-literal are still flagged.
        for src in [
            "exec(code)\n",
            "eval(user_input)\n",
            "compile(src, '<s>', 'exec')\n",
        ] {
            let m = parse(src);
            assert!(
                m.security_hits.iter().any(|h| h.rule == "dangerous-eval"),
                "builtin not flagged: {src}"
            );
        }
    }

    #[test]
    fn detects_weak_cipher_imports() {
        let m = parse(
            "from Crypto.Cipher import DES as pycrypto_des\n\
             from Cryptodome.Cipher import ARC4 as ax\n\
             cipher = pycrypto_des.new(key, pycrypto_des.MODE_CTR)\n\
             c2 = ax.new(key)\n",
        );
        let cipher_hits: Vec<_> = m
            .security_hits
            .iter()
            .filter(|h| h.rule == "weak-cipher")
            .collect();
        assert_eq!(
            cipher_hits.len(),
            2,
            "expected DES + ARC4 imports flagged, got {:?}",
            m.security_hits
        );
        let lines: Vec<u32> = cipher_hits.iter().map(|h| h.line).collect();
        assert!(lines.contains(&1) && lines.contains(&2), "lines {lines:?}");
    }

    #[test]
    fn detects_weak_cipher_direct_constructor_and_ecb() {
        let m = parse(
            "from cryptography.hazmat.primitives.ciphers import algorithms, modes, Cipher\n\
             c = Cipher(algorithms.ARC4(key), mode=None)\n",
        );
        assert!(
            m.security_hits.iter().any(|h| h.rule == "weak-cipher"),
            "expected ARC4 constructor flagged, got {:?}",
            m.security_hits
        );
        let ecb = parse("from Crypto.Cipher import AES\nc = AES.new(key, AES.MODE_ECB)\n");
        assert!(
            ecb.security_hits.iter().any(|h| h.rule == "weak-cipher"),
            "expected ECB mode flagged, got {:?}",
            ecb.security_hits
        );
    }

    #[test]
    fn strong_cipher_and_modes_not_flagged() {
        let m = parse(
            "from cryptography.hazmat.primitives.ciphers import algorithms, modes, Cipher\n\
             c = Cipher(algorithms.AES(key), modes.GCM(iv))\n",
        );
        assert!(
            !m.security_hits.iter().any(|h| h.rule == "weak-cipher"),
            "AES-GCM should not be flagged, got {:?}",
            m.security_hits
        );
        let unrelated = parse("from myapp.utils import DES\nDES.do_thing()\n");
        assert!(
            !unrelated
                .security_hits
                .iter()
                .any(|h| h.rule == "weak-cipher"),
            "non-crypto `DES` import should not be flagged, got {:?}",
            unrelated.security_hits
        );
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
        let m = parse("import app\n@app.route('/x')\ndef view():\n    return 1\n");
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

    #[test]
    fn scope_resolution_excludes_shadows_and_attributes() {
        // `helper` is defined at module scope but never *loaded* there: the only
        // references are a function-local binding (a shadow) and an attribute
        // access (`obj.helper`). Token counting would call it "used"; scope
        // resolution correctly does not.
        let m = parse(
            "def helper():\n    pass\n\ndef f():\n    helper = 1\n    return helper\n\nobj.helper()\n",
        );
        assert!(
            !m.module_used.iter().any(|s| s == "helper"),
            "module_used should exclude shadowed/attribute `helper`: {:?}",
            m.module_used
        );
        // A genuine free load that resolves to module scope IS captured.
        let m2 = parse("def g():\n    pass\n\ng()\n");
        assert!(
            m2.module_used.iter().any(|s| s == "g"),
            "{:?}",
            m2.module_used
        );
        // `global` forces module resolution: the RHS load of `counter` binds to
        // the module-level name even though it is assigned inside the function.
        let m3 =
            parse("counter = 0\n\ndef bump():\n    global counter\n    counter = counter + 1\n");
        assert!(
            m3.module_used.iter().any(|s| s == "counter"),
            "{:?}",
            m3.module_used
        );
        // Without `global`, the same assignment makes `counter` a local shadow.
        let m4 = parse("counter = 0\n\ndef bump():\n    counter = counter + 1\n");
        assert!(
            !m4.module_used.iter().any(|s| s == "counter"),
            "{:?}",
            m4.module_used
        );
    }

    #[test]
    fn scope_resolution_sees_defaults_and_annotations() {
        // Defaults, parameter annotations, and return annotations evaluate in
        // the enclosing (module) scope — they are genuine uses.
        let m = parse("DEFAULT = 5\nMyType = int\ndef f(x=DEFAULT) -> MyType: ...\n");
        assert!(
            m.module_used.iter().any(|s| s == "DEFAULT"),
            "{:?}",
            m.module_used
        );
        assert!(
            m.module_used.iter().any(|s| s == "MyType"),
            "{:?}",
            m.module_used
        );
        let m2 = parse("MyType = int\ndef g(x: MyType): ...\n");
        assert!(
            m2.module_used.iter().any(|s| s == "MyType"),
            "{:?}",
            m2.module_used
        );
        // Lambda parameter defaults too.
        let m3 = parse("DEFAULT = 5\ng = lambda x=DEFAULT: x\n");
        assert!(
            m3.module_used.iter().any(|s| s == "DEFAULT"),
            "{:?}",
            m3.module_used
        );
    }

    #[test]
    fn imports_inside_module_level_suites_seen() {
        let m = parse(
            "from contextlib import suppress\n\
             with suppress(ImportError):\n    import ujson\n\
             for _i in range(1):\n    import for_mod\n\
             while cond():\n    import while_mod\n\
             match val:\n    case 1:\n        import match_mod\n",
        );
        for want in ["ujson", "for_mod", "while_mod", "match_mod"] {
            assert!(
                m.imports.iter().any(|i| i.module == want),
                "missing {want}: {:?}",
                m.imports
            );
        }
    }

    #[test]
    fn type_checking_marks_body_not_else() {
        let m = parse(
            "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    import a\nelse:\n    import b\n",
        );
        let a = m.imports.iter().find(|i| i.module == "a").unwrap();
        let b = m.imports.iter().find(|i| i.module == "b").unwrap();
        assert!(a.type_checking_only);
        assert!(!b.type_checking_only, "else branch is the runtime branch");
        // `if not TYPE_CHECKING:` inverts the branches.
        let m2 = parse(
            "from typing import TYPE_CHECKING\nif not TYPE_CHECKING:\n    import rt\nelse:\n    import tc\n",
        );
        let rt = m2.imports.iter().find(|i| i.module == "rt").unwrap();
        let tc = m2.imports.iter().find(|i| i.module == "tc").unwrap();
        assert!(!rt.type_checking_only);
        assert!(tc.type_checking_only);
    }

    #[test]
    fn type_checking_guard_is_exact() {
        let fp = parse("if MY_TYPE_CHECKING_OVERRIDE:\n    from x import y\n");
        assert!(
            !fp.imports
                .iter()
                .find(|i| i.module == "x")
                .unwrap()
                .type_checking_only,
            "substring match must not treat this as a guard"
        );
        let ok = parse("import typing\nif typing.TYPE_CHECKING:\n    from x import y\n");
        assert!(
            ok.imports
                .iter()
                .find(|i| i.module == "x")
                .unwrap()
                .type_checking_only
        );
    }

    #[test]
    fn comprehension_targets_are_not_function_locals() {
        // Python 3 comprehensions have their own scope: `item` here does not
        // shadow the module-level binding for the trailing `return item`.
        let m =
            parse("item = 1\ndef f(items):\n    xs = [item for item in items]\n    return item\n");
        assert!(
            m.module_used.iter().any(|s| s == "item"),
            "{:?}",
            m.module_used
        );
    }

    #[test]
    fn dunder_all_mutations() {
        let m = parse("__all__ = ['a']\n__all__ += ['b']\n");
        assert_eq!(m.dunder_all, Some(vec!["a".into(), "b".into()]));
        let m2 = parse("__all__ = ['a']\n__all__.extend(['b', 'c'])\n__all__.append('d')\n");
        assert_eq!(
            m2.dunder_all,
            Some(vec!["a".into(), "b".into(), "c".into(), "d".into()])
        );
        // Non-literal mutations make the list unknowable — None, not a wrong
        // partial list.
        let m3 = parse("__all__ = ['a']\n__all__ += make()\n");
        assert_eq!(m3.dunder_all, None);
        let m4 = parse("__all__ = ['a']\n__all__.extend(names)\n");
        assert_eq!(m4.dunder_all, None);
        let m5 = parse("__all__ = ['a']\n__all__.append(name)\n");
        assert_eq!(m5.dunder_all, None);
    }

    #[test]
    fn decorated_def_line_points_at_def() {
        let m = parse("import app\n\n@app.route('/x')\ndef view() -> _Priv:\n    return 1\n");
        let d = m.definitions.iter().find(|d| d.name == "view").unwrap();
        assert_eq!(d.line, 4, "decorator on line 3, def on line 4");
        assert_eq!(d.end_line, 5, "end_line keeps the full range");
        let f = m.functions.iter().find(|f| f.name == "view").unwrap();
        assert_eq!(f.line, 4);
        let leak = m
            .type_leaks
            .iter()
            .find(|l| l.type_name == "_Priv")
            .unwrap();
        assert_eq!(leak.line, 4);
        let m2 = parse("@decorate\nclass C:\n    @property\n    def p(self):\n        return 1\n");
        let c = m2.classes.iter().find(|c| c.name == "C").unwrap();
        assert_eq!(c.line, 2);
        let p = c.members.iter().find(|mb| mb.name == "p").unwrap();
        assert_eq!(p.line, 4);
        let cd = m2.definitions.iter().find(|d| d.name == "C").unwrap();
        assert_eq!(cd.line, 2);
    }

    #[test]
    fn typevar_under_guard_not_a_leak() {
        let m = parse(
            "from typing import TYPE_CHECKING, TypeVar\nif TYPE_CHECKING:\n    _T = TypeVar('_T')\ndef f(x: _T) -> _T: ...\n",
        );
        assert!(m.type_leaks.is_empty(), "{:?}", m.type_leaks);
        let m2 = parse(
            "try:\n    _P = ParamSpec('_P')\nexcept ImportError:\n    pass\ndef g(x: _P): ...\n",
        );
        assert!(m2.type_leaks.is_empty(), "{:?}", m2.type_leaks);
    }

    #[test]
    fn noqa_comments_map_to_unused_binding_rules() {
        // Blanket noqa silences both unused-binding rules on that line.
        assert_eq!(
            parse_noqa_comment("# noqa"),
            Some(vec!["unused-import".into(), "unused-variable".into()])
        );
        assert_eq!(
            parse_noqa_comment("#NOQA"),
            Some(vec!["unused-import".into(), "unused-variable".into()])
        );
        // Coded noqa maps only the codes that correspond to our rules.
        assert_eq!(
            parse_noqa_comment("# noqa: F401"),
            Some(vec!["unused-import".into()])
        );
        assert_eq!(
            parse_noqa_comment("# noqa: E501, F841"),
            Some(vec!["unused-variable".into()])
        );
        // Foreign codes and noqa-ish prose are not ours to interpret.
        assert_eq!(parse_noqa_comment("# noqa: E501"), None);
        assert_eq!(parse_noqa_comment("# noqable"), None);
        assert_eq!(parse_noqa_comment("# see noqa docs"), None);
        // End-to-end: the wsgi entry-point idiom lands in `ignores`.
        let m = parse("from hello import app  # noqa: F401\n");
        assert!(
            m.ignores.contains(&(1, "unused-import".into())),
            "{:?}",
            m.ignores
        );
    }

    #[test]
    fn redundant_alias_and_try_body_imports_are_marked() {
        let m = parse(
            "from sansio import State as State\nfrom sansio import Blueprint as Sansio\ntry:\n    import fast_json\nexcept ImportError:\n    import json as fast_json\nimport os\n",
        );
        let state = m.imports.iter().find(|i| i.bindings == ["State"]).unwrap();
        assert_eq!(state.redundant, vec![true]);
        let aliased = m.imports.iter().find(|i| i.bindings == ["Sansio"]).unwrap();
        assert_eq!(aliased.redundant, vec![false]);
        let probe = m.imports.iter().find(|i| i.module == "fast_json").unwrap();
        assert!(probe.in_try, "try-body import not marked: {probe:?}");
        let fallback = m.imports.iter().find(|i| i.module == "json").unwrap();
        assert!(fallback.in_try, "except-handler import not marked");
        let plain = m.imports.iter().find(|i| i.module == "os").unwrap();
        assert!(!plain.in_try);
        std::assert!(!plain.redundant.iter().any(|r| *r));
    }

    #[test]
    fn ignore_comment_allows_trailing_text() {
        assert_eq!(
            parse_ignore_comment("# mollify: ignore[dead-code]  -- migrating soon"),
            Some(vec!["dead-code".into()])
        );
        assert_eq!(
            parse_ignore_comment("# mollify: ignore[a, b] reason"),
            Some(vec!["a".into(), "b".into()])
        );
        let m = parse("x = 1  # mollify: ignore[dead-code] -- reason\n");
        assert!(
            m.ignores.contains(&(1, "dead-code".into())),
            "{:?}",
            m.ignores
        );
    }

    #[test]
    fn nested_weak_cipher_import_flagged() {
        let m = parse("def f():\n    from Crypto.Cipher import DES\n    return DES\n");
        assert!(
            m.security_hits.iter().any(|h| h.rule == "weak-cipher"),
            "nested import must be scanned: {:?}",
            m.security_hits
        );
    }
}
