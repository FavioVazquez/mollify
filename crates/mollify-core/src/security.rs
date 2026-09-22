//! Security engine — a deterministic **candidate producer** (bandit-style).
//! It emits syntactic candidates; it never decides exploitability (the
//! candidate/verifier split). Maps parser `SecurityHit`s to
//! findings with per-rule confidence.

use crate::fingerprint::fingerprint;
use mollify_graph::ModuleGraph;
use mollify_types::{Action, Category, Confidence, Finding, Location, Severity};

fn confidence_for(rule: &str) -> Confidence {
    match rule {
        // Provable-ish footguns.
        "subprocess-shell-true"
        | "tls-verify-disabled"
        | "unsafe-yaml-load"
        | "weak-hash"
        | "weak-cipher"
        | "flask-debug-true"
        | "jinja2-autoescape-false" => Confidence::Likely,
        // Depends on whether input is trusted / context.
        "dangerous-eval" | "unsafe-deserialization" | "sql-injection" => Confidence::Uncertain,
        // Noisy without context: stdlib random is fine for non-security use.
        "insecure-random" | "request-without-timeout" | "try-except-pass" => Confidence::Uncertain,
        // Could be a placeholder / test fixture.
        "hardcoded-secret" => Confidence::Likely,
        _ => Confidence::Likely,
    }
}

/// Best-effort CWE id for a rule, surfaced in the reason for compliance/SARIF.
fn cwe_for(rule: &str) -> Option<&'static str> {
    Some(match rule {
        "dangerous-eval" => "CWE-95",
        "subprocess-shell-true" => "CWE-78",
        "sql-injection" => "CWE-89",
        "unsafe-yaml-load" => "CWE-20",
        "unsafe-deserialization" => "CWE-502",
        "tls-verify-disabled" => "CWE-295",
        "hardcoded-secret" => "CWE-798",
        "weak-hash" | "weak-cipher" => "CWE-327",
        "insecure-random" => "CWE-330",
        "request-without-timeout" => "CWE-400",
        "flask-debug-true" => "CWE-94",
        "jinja2-autoescape-false" => "CWE-79",
        "try-except-pass" => "CWE-703",
        _ => return None,
    })
}

pub fn analyze(graph: &ModuleGraph, test_dirs: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in &graph.modules {
        let dev_tree = crate::paths::is_dev_tree(&m.rel, test_dirs);
        findings.extend(analyze_parsed_ids(&m.path, &m.rel, &m.parsed, dev_tree));
    }
    findings
}

/// Security findings for a single parsed module (also used by the live LSP
/// path, where the display path doubles as the fingerprint identity).
pub fn analyze_parsed(
    path: &camino::Utf8Path,
    parsed: &mollify_parse::ParsedModule,
) -> Vec<Finding> {
    analyze_parsed_ids(path, path, parsed, crate::paths::is_dev_tree(path, &[]))
}

/// `path` is what findings display; `rel` (root-relative) is the stable
/// fingerprint identity. The hit's detail text anchors the fingerprint, so
/// unrelated edits above it don't churn baselines. `dev_tree` caps confidence
/// at `uncertain` — a security candidate in tests/docs/examples is still
/// evidence, but its risk model assumes production code, so it must not
/// dominate reports or `--min-confidence likely` runs.
fn analyze_parsed_ids(
    path: &camino::Utf8Path,
    rel: &camino::Utf8Path,
    parsed: &mollify_parse::ParsedModule,
    dev_tree: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut occ = crate::fingerprint::Occurrences::default();
    for hit in &parsed.security_hits {
        let occ_key = format!("{}\u{1f}{}", hit.rule, hit.detail);
        let confidence = if dev_tree {
            Confidence::Uncertain
        } else {
            confidence_for(hit.rule)
        };
        let mut reason = match cwe_for(hit.rule) {
            Some(cwe) => format!("{} [{cwe}]", hit.detail),
            None => hit.detail.clone(),
        };
        if dev_tree {
            reason.push_str(" (in test/docs/example code)");
        }
        findings.push(Finding {
            fingerprint: fingerprint(hit.rule, &[rel.as_str(), &hit.detail, &occ.next(&occ_key)]),
            rule: hit.rule.to_string(),
            category: Category::Security,
            severity: Severity::Warn,
            confidence,
            attribution: None,
            reason,
            location: Location {
                path: path.to_owned(),
                line: hit.line,
                column: 0,
                end_line: None,
            },
            actions: vec![Action {
                kind: "review-security".into(),
                description: "Review this security candidate; confirm before acting".into(),
                auto_fixable: false,
                suppression_comment: Some(format!("# mollify: ignore[{}]", hit.rule)),
            }],
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};
    use mollify_graph::discover_python_files;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-core-sec-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Utf8PathBuf::from_path_buf(base).unwrap()
    }
    fn write(dir: &Utf8Path, rel: &str, src: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, src).unwrap();
    }

    #[test]
    fn identifier_equal_to_its_literal_is_not_a_secret() {
        // OpenAPI enums assign the member name to itself (`apiKey = "apiKey"`).
        // That is a schema token, not a credential. A different literal still is.
        let d = temp("seceq");
        write(
            &d,
            "models.py",
            "class SecuritySchemeType:\n    apiKey = \"apiKey\"\npassword = \"s3cret-value\"\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let secrets: Vec<_> = f
            .iter()
            .filter(|x| x.rule == "hardcoded-secret")
            .map(|x| x.reason.clone())
            .collect();
        assert!(
            !secrets.iter().any(|r| r.contains("apiKey")),
            "enum token flagged as a secret"
        );
        assert!(
            secrets.iter().any(|r| r.contains("password")),
            "real secret not flagged"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn url_and_sentence_are_not_hardcoded_secrets() {
        // A URL whose name contains "token", and an error sentence whose name
        // contains "secret", are not credentials. A token-like literal still is.
        let d = temp("securl");
        write(
            &d,
            "models.py",
            "_TOKEN_URL = \"https://sso.example/token\"\n_SECRETSTORAGE_UNAVAILABLE_REASON = \"as the secretstorage module is not installed\"\npassword = \"s3cret-value\"\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let secrets: Vec<_> = f
            .iter()
            .filter(|x| x.rule == "hardcoded-secret")
            .map(|x| x.reason.clone())
            .collect();
        assert!(
            !secrets.iter().any(|r| r.contains("TOKEN_URL")),
            "URL flagged as a secret"
        );
        assert!(
            !secrets
                .iter()
                .any(|r| r.contains("SECRETSTORAGE_UNAVAILABLE_REASON")),
            "error sentence flagged as a secret"
        );
        assert!(
            secrets.iter().any(|r| r.contains("password")),
            "real secret not flagged"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn surfaces_candidates() {
        let d = temp("sec");
        write(
            &d,
            "__init__.py",
            "import subprocess\napi_key = \"sk-abcdef123\"\nsubprocess.run(c, shell=True)\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let rules: Vec<_> = f.iter().map(|x| x.rule.as_str()).collect();
        assert!(rules.contains(&"hardcoded-secret"), "got {rules:?}");
        assert!(rules.contains(&"subprocess-shell-true"), "got {rules:?}");
        assert!(f.iter().all(|x| x.category == Category::Security));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn surfaces_expanded_rules_with_cwe() {
        let d = temp("sec2");
        write(
            &d,
            "__init__.py",
            "import hashlib, os, random\nhashlib.md5(b'x')\nos.system(cmd)\nrandom.random()\ncur.execute(f\"select {x}\")\nrequests.get(url)\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let rules: Vec<_> = f.iter().map(|x| x.rule.as_str()).collect();
        for expected in [
            "weak-hash",
            "subprocess-shell-true",
            "insecure-random",
            "sql-injection",
            "request-without-timeout",
        ] {
            assert!(rules.contains(&expected), "missing {expected}: {rules:?}");
        }
        // CWE is surfaced in the reason.
        assert!(f
            .iter()
            .find(|x| x.rule == "weak-hash")
            .unwrap()
            .reason
            .contains("CWE-327"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn surfaces_weak_cipher_with_cwe() {
        let d = temp("sec3");
        // Import-aliased weak cipher — the real-world (bandit) idiom that the
        // previous call-only matcher missed entirely.
        write(
            &d,
            "__init__.py",
            "from Crypto.Cipher import DES as d\ncipher = d.new(key, d.MODE_ECB)\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let wc = f
            .iter()
            .find(|x| x.rule == "weak-cipher")
            .expect("weak-cipher should be flagged");
        assert_eq!(wc.category, Category::Security);
        assert!(wc.reason.contains("CWE-327"), "got {}", wc.reason);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn cli_tester_execute_is_not_sql_injection() {
        // `tester.execute(f"cache clear {name}")` is a CLI harness. A database
        // handle's `cursor.execute` of a dynamic string still is sql-injection.
        let d = temp("sqlexec");
        write(
            &d,
            "app.py",
            "def test_clear(tester, name):\n    tester.execute(f\"cache clear {name}\")\n\ndef query(cursor, name):\n    cursor.execute(f\"select {name}\")\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let sql: Vec<_> = f.iter().filter(|x| x.rule == "sql-injection").collect();
        assert!(
            !sql.iter().any(|x| x.location.line == 2),
            "CLI tester flagged as SQL: {sql:?}"
        );
        assert!(
            sql.iter().any(|x| x.location.line == 5),
            "cursor.execute not flagged: {sql:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn quote_sanitized_identifier_interpolation_is_not_sql_injection() {
        // DuckDB cannot parameterize identifiers. An f-string that only
        // interpolates a name quote-escaped in the same function, inside
        // ATTACH / COPY / CREATE, is not value injection. A raw identifier
        // and a WHERE value still are, even when the value was quote-escaped.
        let d = temp("sqlident");
        write(
            &d,
            "__init__.py",
            "def ddl(con, target, table, dest):\n\
    \ttarget = target.replace(\"'\", \"''\")\n\
    \ttable = table.replace(\"'\", \"''\")\n\
    \tdest = dest.replace(\"'\", \"''\")\n\
    \tcon.execute(f\"ATTACH '{target}'\")\n\
    \tcon.execute(f\"CREATE TABLE {table} (id INT)\")\n\
    \tcon.execute(f\"COPY {table} TO '{dest}'\")\n\
\n\
def raw_attach(con, target):\n\
    \tcon.execute(f\"ATTACH '{target}'\")\n\
\n\
def query(cur, user_id):\n\
    \tcur.execute(f\"SELECT * FROM t WHERE id = {user_id}\")\n\
\n\
def escaped_value(cur, user_id):\n\
    \tuser_id = user_id.replace(\"'\", \"''\")\n\
    \tcur.execute(f\"SELECT * FROM t WHERE id = '{user_id}'\")\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let sql: Vec<_> = f.iter().filter(|x| x.rule == "sql-injection").collect();
        assert_eq!(sql.len(), 3, "got {sql:?}");
        let lines: Vec<u32> = sql.iter().map(|x| x.location.line).collect();
        assert!(
            !lines.contains(&5) && !lines.contains(&6) && !lines.contains(&7),
            "sanitized ATTACH/CREATE/COPY should be quiet: {sql:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn quote_rejected_identifier_interpolation_is_not_sql_injection() {
        // The field sanitizer is `if "'" in target: raise`, not str.replace.
        // That name, used only as an identifier, is not sql-injection. A name
        // that was not checked, and a WHERE value that was checked, still are.
        // A check that does not abort the function does not count.
        let d = temp("sqlreject");
        write(
            &d,
            "__init__.py",
            "def snapshot(con, target, catalog):\n\
    \ttarget = str(target)\n\
    \tif \"'\" in target or \";\" in target:\n\
    \t\traise ValueError(\"bad\")\n\
    \tcon.execute(f\"ATTACH '{target}' AS snap\")\n\
    \tcon.execute(f\"COPY FROM DATABASE {catalog} TO snap\")\n\
\n\
def query(cur, user_id):\n\
    \tif \"'\" in user_id:\n\
    \t\traise ValueError(\"bad\")\n\
    \tcur.execute(f\"SELECT * FROM t WHERE id = '{user_id}'\")\n\
\n\
def logged(con, target):\n\
    \tif \"'\" in target:\n\
    \t\tprint(\"nope\")\n\
    \tcon.execute(f\"ATTACH '{target}'\")\n",
        );
        let files = discover_python_files(&d);
        let g = ModuleGraph::build(&d, &files);
        let f = analyze(&g, &[]);
        let sql: Vec<_> = f.iter().filter(|x| x.rule == "sql-injection").collect();
        assert_eq!(sql.len(), 3, "got {sql:?}");
        let lines: Vec<u32> = sql.iter().map(|x| x.location.line).collect();
        assert!(
            !lines.contains(&5),
            "quote-rejected ATTACH should be quiet: {sql:?}"
        );
        assert!(lines.contains(&6), "unchecked catalog should stay: {sql:?}");
        assert!(lines.contains(&11), "WHERE value should stay: {sql:?}");
        assert!(
            lines.contains(&16),
            "a check that does not abort should stay: {sql:?}"
        );
        std::fs::remove_dir_all(&d).ok();
    }
}
