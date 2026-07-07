//! Shared path/test-discovery helpers used by the dead-code and dependency
//! engines, so "what counts as a test module" has a single definition.

use camino::Utf8Path;

/// True if a module path is test/dev code (so importing dev deps there is fine,
/// and pytest collection roots inside it are reachable). `test_dirs` are extra
/// directory names beyond the `tests/` convention — typically a project's
/// `[tool.pytest.ini_options].testpaths`.
pub fn is_test_module(path: &Utf8Path, test_dirs: &[String]) -> bool {
    let p = normalize_separators(path);
    let p = p.as_str();
    let name = p.rsplit('/').next().unwrap_or("");
    if p.contains("/tests/")
        || p.contains("/test/")
        || p.starts_with("tests/")
        || p.starts_with("test/")
        || name.starts_with("test_")
        || name.ends_with("_test.py")
        || name == "conftest.py"
    {
        return true;
    }
    test_dirs.iter().any(|d| {
        let d = d.trim_matches('/');
        !d.is_empty() && (p.starts_with(&format!("{d}/")) || p.contains(&format!("/{d}/")))
    })
}

/// All path-class heuristics match on `/`-separated strings; Windows paths
/// arrive with `\` separators, which would silently disable every heuristic
/// (`p.contains("/tests/")` never matches `pkg\tests\x.py`).
fn normalize_separators(path: &Utf8Path) -> String {
    path.as_str().replace('\\', "/")
}

/// True if a module lives in a non-production tree: test code (per
/// [`is_test_module`]) or documentation/example/benchmark directories. Used to
/// down-weight findings whose risk model assumes production code — a missing
/// request timeout in a test or a doc snippet is evidence of a very different
/// weight than one on a serving path.
pub fn is_dev_tree(path: &Utf8Path, test_dirs: &[String]) -> bool {
    if is_test_module(path, test_dirs) {
        return true;
    }
    let p = normalize_separators(path);
    ["docs", "doc", "examples", "example", "benchmarks"]
        .iter()
        .any(|d| p.starts_with(&format!("{d}/")) || p.contains(&format!("/{d}/")))
}

/// True if a module lives in a fixture/data tree — `.py` files that are tool
/// *inputs* (formatter test cases, mypy golden files, snapshots), not code.
/// Findings there are often technically correct but must never be auto-fixed:
/// editing black's `tests/data/cases/*.py` "fixes" the fixtures and breaks the
/// suite. Reachability alone can't catch these (sample code may contain a
/// `__main__` guard, which reads as an entry point).
pub fn is_fixture_tree(path: &Utf8Path) -> bool {
    let p = normalize_separators(path);
    [
        "data",
        "fixtures",
        "testdata",
        "test_data",
        "golden",
        "snapshots",
        // Vendored trees are code you don't own: auto-editing them breaks
        // upstream-sync hygiene even when the finding is true (astropy's
        // `extern/`, black's `blib2to3`-style vendoring).
        "extern",
        "vendor",
        "vendored",
        "_vendor",
        "third_party",
        "thirdparty",
    ]
    .iter()
    .any(|d| p.starts_with(&format!("{d}/")) || p.contains(&format!("/{d}/")))
}

/// True if a top-level name is a pytest collection root: a `test_*` function or
/// a `Test*` class. Such symbols are invoked by the test runner, not by in-repo
/// callers, so they must not be reported as `unused-export` inside test modules.
pub fn is_pytest_entity(name: &str) -> bool {
    name.starts_with("test_") || name.starts_with("Test")
}

/// Read `[tool.pytest.ini_options].testpaths` from a project's `pyproject.toml`,
/// returning the configured test directories (empty if absent/unparseable). Lets
/// dead-code/dep analysis honor non-conventional test layouts.
pub fn pytest_testpaths(root: &Utf8Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("pyproject.toml")) else {
        return Vec::new();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    table
        .get("tool")
        .and_then(|t| t.get("pytest"))
        .and_then(|p| p.get("ini_options"))
        .and_then(|i| i.get("testpaths"))
        .and_then(|tp| tp.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8Path;

    #[test]
    fn test_module_detection() {
        assert!(is_test_module(Utf8Path::new("tests/test_x.py"), &[]));
        assert!(is_test_module(Utf8Path::new("pkg/conftest.py"), &[]));
        assert!(is_test_module(Utf8Path::new("a/test/b.py"), &[]));
        assert!(!is_test_module(Utf8Path::new("pkg/core.py"), &[]));
        // Non-conventional dir via testpaths.
        assert!(is_test_module(
            Utf8Path::new("suite/check_a.py"),
            &["suite".into()]
        ));
        assert!(!is_test_module(
            Utf8Path::new("pkg/core.py"),
            &["suite".into()]
        ));
    }

    #[test]
    fn windows_separators_match_all_heuristics() {
        // On Windows the graph hands these helpers `\`-separated paths; the
        // `/`-based patterns must still classify them.
        assert!(is_test_module(Utf8Path::new(r"pkg\tests\test_x.py"), &[]));
        assert!(is_test_module(Utf8Path::new(r"pkg\sub\conftest.py"), &[]));
        assert!(is_dev_tree(Utf8Path::new(r"docs\conf.py"), &[]));
        assert!(is_fixture_tree(Utf8Path::new(r"tests\data\cases\a.py")));
        assert!(!is_test_module(Utf8Path::new(r"pkg\core.py"), &[]));
    }

    #[test]
    fn pytest_entity_detection() {
        assert!(is_pytest_entity("test_addition"));
        assert!(is_pytest_entity("TestScorecard"));
        assert!(!is_pytest_entity("helper"));
        assert!(!is_pytest_entity("compute_total"));
    }
}
