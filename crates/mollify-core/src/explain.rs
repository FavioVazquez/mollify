//! `mollify explain <rule>` — human-readable semantics for a rule id, with no
//! analysis run. Keeps the "evidence, not decisions" contract legible: every
//! rule states what it proves, its confidence ceiling, and how to act on it.

/// Return the explanation for a rule id, or `None` if unknown.
pub fn text(rule: &str) -> Option<&'static str> {
    let t = match rule {
        "unused-file" => {
            "A module that nothing reachable from an entry point imports. \
            Confidence: certain when there is no dynamic import sink in the project. \
            Action: delete the file, or mark its module as an entry point."
        }
        "unused-import" => {
            "An imported name that is never referenced outside its own import in \
            the module. Confidence: certain in a regular module with no dynamic \
            sink (auto-fixable); uncertain in `__init__.py` (likely a re-export). \
            Action: remove the import."
        }
        "unused-export" => {
            "A top-level function/class never referenced outside its own \
            module and not listed in `__all__`. Confidence: likely (dynamic access via \
            getattr downgrades it). Action: remove it or make it private."
        }
        "unused-dependency" => {
            "A distribution declared in pyproject/requirements but never \
            imported. Confidence: likely. Action: remove it from your dependency list."
        }
        "missing-dependency" => {
            "A third-party module imported but absent from your declared \
            dependencies (not stdlib, not first-party). Action: add it to your project metadata."
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
        "high-complexity" => {
            "A function whose cyclomatic or cognitive complexity exceeds the \
            configured threshold. Action: decompose it; extract helpers and flatten branches."
        }
        "duplication" => {
            "A token sequence repeated across locations (Rabin-Karp clone). \
            Action: extract the shared logic into one definition."
        }
        "cold-code" => {
            "A statically reachable function with zero executed lines in the \
            supplied coverage report. Confidence: likely. Action: verify it is dead, then remove."
        }
        "hotspot" => {
            "A file that is both high-churn (git history) and high-complexity — the \
            riskiest code to change. Action: prioritize it for refactoring and test coverage."
        }
        "untyped-function" | "untyped-public" => {
            "A public function with no parameter or \
            return type annotations. Action: add type hints to harden the public surface."
        }
        "respect-policy" | "policy-violation" => {
            "A declarative `.mollifyrc` policy was \
            violated (a forbidden import or call appeared). Confidence: certain. Action: remove \
            or relocate the forbidden construct."
        }
        "dangerous-eval" => {
            "A call to `eval`/`exec` on a non-literal argument. Action: replace \
            with an explicit, safe parser or dispatch table."
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
        _ => return None,
    };
    Some(t)
}

/// Every rule id mollify can emit, for `mollify explain` with no argument.
pub const RULES: &[&str] = &[
    "unused-file",
    "unused-export",
    "unused-import",
    "unused-dependency",
    "missing-dependency",
    "circular-dependency",
    "layer-violation",
    "high-complexity",
    "duplication",
    "cold-code",
    "hotspot",
    "untyped-function",
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
