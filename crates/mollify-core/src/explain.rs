//! `mollify explain <rule>` — human-readable semantics for a rule id, with no
//! analysis run. Keeps the "evidence, not decisions" contract legible: every
//! rule states what it proves, its confidence ceiling, and how to act on it.

/// Return the explanation for a rule id, or `None` if unknown.
pub fn text(rule: &str) -> Option<&'static str> {
    let t = match rule {
        "engine-panic" => {
            "An analysis engine crashed while producing this report; its findings \
            are missing. The rest of the report is complete and trustworthy — \
            engines are isolated so one failure cannot corrupt the others' output. \
            Confidence: certain (the crash observably happened). Action: file a \
            bug with the reason text; nothing in your code needs to change."
        }
        "unused-file" => {
            "A module that nothing reachable from an entry point imports. \
            Confidence: certain when there is no dynamic import sink in the project. \
            Action: delete the file, or mark its module as an entry point."
        }
        "unused-import" => {
            "An imported name that is never referenced outside its own import in \
            the module. Confidence: certain in a reachable module with no dynamic \
            sink (auto-fixable); likely when the module itself is unreachable or \
            vendored (fixture/extern trees — never auto-edited) or the imported \
            module registers handlers at import time (framework/dispatch \
            decorators); uncertain in `__init__.py` (likely a re-export), inside \
            `try`/`except` or a module-level `if` (availability probes), and \
            when the module's `__all__` is dynamic. Never flagged: `__future__` \
            imports, redundant-alias re-exports (`import x as x`, PEP 484), \
            names another module imports from here, names in quoted TypeAlias \
            values, and `# noqa`-suppressed lines (any line of a multi-line \
            import). Action: remove the import."
        }
        "unused-variable" => {
            "A local variable assigned but never read in its function (ruff F841). \
            Confidence: likely. Not auto-fixed (the right-hand side may have side \
            effects). Action: remove it, or prefix with `_`."
        }
        "unused-parameter" => {
            "A function parameter never used in the body. Confidence: uncertain \
            (it may satisfy an interface/override/callback signature). Interface-bound \
            parameters are never flagged: dunder methods, `@abstractmethod`/`@overload`/\
            `@override` methods, overrides of an in-project base-class method, methods \
            of classes with external bases, and decorated top-level functions (their \
            signature may be dictated by the framework). Action: remove it or prefix \
            with `_`."
        }
        "unused-export" => {
            "A top-level function/class never referenced outside its own \
            module and not listed in `__all__`. Confidence: likely (dynamic access via \
            getattr downgrades it; private symbols are certain only in reachable \
            modules — unreachable files are often fixture data and never \
            auto-edited). Reachability roots are exempt: framework-registered \
            symbols, pytest `test_*`/`Test*` in test paths (honoring \
            `[tool.pytest.ini_options].testpaths`), and functions named by a \
            `[project.scripts]` entry point. Action: remove it or make it private."
        }
        "unused-method" => {
            "A class method never referenced anywhere as an attribute \
            (`obj.m`/`self.m`/`Class.m`). Confidence: likely for private (`_m`), \
            uncertain for public (may be an override/duck-typed/external API). \
            Skips dunders, properties, static/class/abstract methods, and \
            framework-registered methods. Action: remove it, or confirm the API use."
        }
        "unused-attribute" => {
            "A class-level attribute/constant never referenced as an attribute \
            and never read as a bare name. Confidence: likely for private, uncertain \
            for public. Skips dataclass/Pydantic/NamedTuple/TypedDict fields. \
            Action: remove it, or confirm dynamic use."
        }
        "unused-enum-member" => {
            "An `enum.Enum` member never referenced. Confidence: uncertain — enums \
            are often accessed dynamically (`Color[name]`, `Color(value)`, iteration, \
            serialization). Action: remove it, or confirm dynamic/serialized use."
        }
        "unreachable-code" => {
            "A statement that can never execute because it follows an \
            unconditional terminator (`return`/`raise`/`break`/`continue`/`sys.exit()`) \
            in the same block. Confidence: certain — provable syntactically. \
            Action: remove the dead statement."
        }
        "private-type-leak" => {
            "A public function/method whose signature references a private \
            (`_Name`) type a caller cannot name (intentional `TypeVar`s are \
            excluded). Confidence: likely. Action: make the type public, or stop \
            exposing it in the public signature."
        }
        "unused-dependency" => {
            "A distribution declared in pyproject/requirements but never \
            imported. Lazy imports inside function bodies and modules referenced by \
            `[project.scripts]` entry points count as usage. Confidence: likely. \
            Action: remove it from your dependency list."
        }
        "transitive-dependency" => {
            "A package imported and installed, but only because another dependency \
            pulls it in (not declared directly). Confidence: likely. Action: add \
            it to your direct dependencies so it survives the transitive dep changing."
        }
        "missing-dependency" => {
            "A third-party module imported but absent from your declared \
            dependencies (not stdlib, not first-party). First-party test helpers \
            imported by bare leaf name (`conftest`, sibling modules on a test path) \
            are treated as internal, not missing. Action: add it to your project metadata."
        }
        "misplaced-dev-dependency" => {
            "A distribution declared only in a dev/test group (PEP 735 \
            `dependency-groups`, Poetry/uv/pdm dev deps) but imported from \
            production (non-test) code (deptry DEP004). Confidence: likely. \
            Action: move it to your runtime dependencies."
        }
        "unresolved-import" => {
            "An import that looks internal — relative (`from . import x`) or under \
            a first-party top-level package — but resolves to no module in the \
            project. Confidence: likely — a relative import may still resolve to \
            an in-tree C extension or a build-generated module the `.py` walk \
            can't see. Action: fix the module path or remove the broken import."
        }
        "duplicate-export" => {
            "An `__init__.py` re-exports the same name from two different \
            modules; the later import silently shadows the earlier, so one \
            re-export is dead and the public API is ambiguous. Confidence: likely. \
            Action: keep a single source for the name."
        }
        "private-import" => {
            "A module imports another *package*'s private (`_name`) symbol, \
            reaching past its public API (tach/knip interface enforcement). \
            Intra-package and relative imports are not flagged. Confidence: likely. \
            Action: import via the package's public API, or make the name public."
        }
        "circular-dependency" => {
            "A cycle of modules that import one another (Tarjan SCC). \
            Confidence: certain — provable from static imports. Action: extract shared code \
            to a lower module, or defer one import into function scope."
        }
        "layer-violation" => {
            "A module imports a higher architectural layer than its own \
            (per `architecture.layers`). Confidence: certain. Action: invert or relocate \
            the dependency so lower layers never depend on higher ones."
        }
        "forbidden-import" => {
            "An import that violates a declarative `contracts.forbidden` rule in \
            `.mollifyrc` (module must not depend on another). Confidence: certain. \
            Action: invert or relocate the dependency."
        }
        "independence-violation" => {
            "Two modules declared independent (`contracts.independent`) import each \
            other. Confidence: certain. Action: extract shared code to a common \
            lower module."
        }
        "high-complexity" => {
            "A function whose cyclomatic or cognitive complexity exceeds the \
            configured threshold. Action: decompose it; extract helpers and flatten branches."
        }
        "duplication" => {
            "A token sequence repeated across locations (exact clone found via a \
            suffix array + LCP). Action: extract the shared logic into one definition."
        }
        "cold-code" => {
            "A statically reachable function with zero executed lines in the \
            supplied coverage report. Confidence: likely. Action: verify it is dead, then remove."
        }
        "commented-code" => {
            "A comment whose text parses as Python code (dead code left in a \
            comment). Prose is excluded — a trailing period or a long, wordy line \
            is treated as English even when it opens with a keyword like `from`/`for`. \
            Confidence: likely. Action: delete it — version control remembers it."
        }
        "low-cohesion" => {
            "A class whose methods share few instance attributes (high LCOM*) — \
            it likely does several unrelated jobs. Confidence: uncertain. Action: \
            split it into cohesive smaller classes."
        }
        "hotspot" => {
            "A file that is both high-churn (git history) and high-complexity — the \
            riskiest code to change. Action: prioritize it for refactoring and test coverage."
        }
        "untyped-function" | "untyped-public" => {
            "A public function with no parameter or \
            return type annotations. When a top-level package is deliberately untyped \
            (20+ eligible public functions, 60%+ of them untyped) the package gets one \
            `likely` package-level finding and its per-function findings are demoted to \
            `uncertain` — evidence preserved, default reports stay readable. Action: add \
            type hints to harden the public surface."
        }
        "respect-policy" | "policy-violation" => {
            "A declarative `.mollifyrc` policy was \
            violated (a forbidden import or call appeared). Policy findings carry the \
            *configured policy id* as their rule (e.g. `no-print`), so look for that id \
            in your `.mollifyrc` policies. Confidence: certain. Action: remove \
            or relocate the forbidden construct."
        }
        "dangerous-eval" => {
            "A call to the `eval`/`exec`/`compile` builtins on a non-literal \
            argument. Only the bare builtins match — methods named `.exec()`/`.eval()` \
            (ORM/driver APIs such as SQLModel's `session.exec`) are not flagged. \
            Action: replace with an explicit, safe parser or dispatch table."
        }
        "subprocess-shell-true" => {
            "A subprocess call with `shell=True`. Action: pass an argv \
            list instead of a shell string to avoid injection."
        }
        "unsafe-yaml-load" => "`yaml.load` without a safe loader. Action: use `yaml.safe_load`.",
        "unsafe-deserialization" => {
            "Deserializing untrusted data with pickle/marshal/shelve. \
            Action: use a safe format such as JSON."
        }
        "tls-verify-disabled" => {
            "TLS verification disabled (`verify=False`). Action: keep \
            verification on; pin a CA bundle if needed."
        }
        "vulnerable-dependency" => {
            "A pinned/locked dependency version falls in a known-vulnerable range \
            from the local advisory DB (`.mollify/advisories.json`). Confidence: \
            certain given the DB. Action: upgrade out of the affected range; refresh \
            the DB with scripts/fetch-advisories.py."
        }
        "hardcoded-secret" => {
            "A literal that looks like a credential assigned to a \
            secret-named variable. Action: load it from the environment or a secret manager."
        }
        "weak-hash" => {
            "Use of a broken hash (md5/sha1) (CWE-327). Action: use sha256+ \
            or pass usedforsecurity=False if it's a non-security checksum."
        }
        "weak-cipher" => {
            "A broken/weak cipher or ECB mode (CWE-327). Action: use an \
            authenticated cipher such as AES-GCM or ChaCha20-Poly1305."
        }
        "insecure-random" => {
            "`random` is not cryptographically secure (CWE-330). Action: use \
            the `secrets` module for tokens/keys/nonces."
        }
        "sql-injection" => {
            "SQL built from an f-string/concatenation/.format passed to an \
            execute-style sink (CWE-89). Action: use parameterized queries."
        }
        "request-without-timeout" => {
            "An HTTP request without a timeout can block indefinitely \
            (CWE-400). Action: pass timeout=."
        }
        "flask-debug-true" => {
            "A web app run with debug=True ships the interactive debugger — \
            remote code execution in production (CWE-94). Action: drive debug \
            from config/env and never enable it in production."
        }
        "jinja2-autoescape-false" => {
            "A Jinja2 Environment created with autoescape=False risks XSS \
            (CWE-79). Action: enable autoescaping (or use select_autoescape)."
        }
        "try-except-pass" => {
            "A broad `except: pass` (bare or Exception/BaseException) silently \
            swallows all errors (CWE-703). Confidence: uncertain. Action: log or \
            handle the error, or narrow the exception type."
        }
        _ => return None,
    };
    Some(t)
}

/// Every rule id mollify can emit, for `mollify explain` with no argument.
pub const RULES: &[&str] = &[
    "engine-panic",
    "unused-file",
    "unused-export",
    "unused-import",
    "unused-variable",
    "unused-parameter",
    "unused-method",
    "unused-attribute",
    "unused-enum-member",
    "unreachable-code",
    "unused-dependency",
    "missing-dependency",
    "transitive-dependency",
    "misplaced-dev-dependency",
    "unresolved-import",
    "duplicate-export",
    "private-import",
    "circular-dependency",
    "layer-violation",
    "forbidden-import",
    "independence-violation",
    "high-complexity",
    "duplication",
    "cold-code",
    "commented-code",
    "hotspot",
    "low-cohesion",
    "untyped-function",
    "private-type-leak",
    "policy-violation",
    "dangerous-eval",
    "subprocess-shell-true",
    "unsafe-yaml-load",
    "unsafe-deserialization",
    "tls-verify-disabled",
    "hardcoded-secret",
    "weak-hash",
    "weak-cipher",
    "insecure-random",
    "sql-injection",
    "request-without-timeout",
    "flask-debug-true",
    "jinja2-autoescape-false",
    "try-except-pass",
    "vulnerable-dependency",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_rules_explain_and_unknown_is_none() {
        assert!(text("circular-dependency").unwrap().contains("cycle"));
        assert!(text("layer-violation").is_some());
        assert!(text("not-a-rule").is_none());
        // Every advertised rule has prose.
        for r in RULES {
            assert!(text(r).is_some(), "no explanation for {r}");
        }
    }
}
