//! Chaos corpus: generated hostile inputs that every engine must survive.
//! Each case is distilled from something a real-world repo could contain;
//! the invariant is "no panic, no hang, sane summary" — not specific findings.

use camino::{Utf8Path, Utf8PathBuf};

fn temp() -> Utf8PathBuf {
    let base = std::env::temp_dir().join(format!("mollify-chaos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    Utf8PathBuf::from_path_buf(base).unwrap()
}

fn write_bytes(dir: &Utf8Path, rel: &str, bytes: &[u8]) {
    std::fs::write(dir.join(rel), bytes).unwrap();
}

#[test]
fn engines_survive_the_chaos_corpus() {
    let d = temp();
    write_bytes(&d, "__main__.py", b"print('hi')\n");
    // Deeply nested expression (classic linter stack-overflow trigger).
    let deep = format!("x = {}1{}\n", "(".repeat(5000), ")".repeat(5000));
    write_bytes(&d, "deep.py", deep.as_bytes());
    // One enormous line (tokenizer/LCP stress).
    let huge = format!(
        "data = [{}]\n",
        (0..20_000)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    write_bytes(&d, "huge.py", huge.as_bytes());
    // NUL byte mid-file.
    write_bytes(&d, "nul.py", b"x = 1\n\x00y = 2\n");
    // Latin-1 with a PEP 263 coding cookie (not valid UTF-8).
    write_bytes(
        &d,
        "latin.py",
        b"# -*- coding: latin-1 -*-\ns = '\xe9\xe8'\n",
    );
    // UTF-8 BOM + CRLF line endings.
    write_bytes(&d, "bom.py", b"\xef\xbb\xbfimport os\r\nprint(os.name)\r\n");
    // Unterminated triple-quoted string.
    write_bytes(&d, "unterm.py", b"s = \"\"\"never closed\nimport os\n");
    // Unicode identifiers + emoji in strings (the D3 OOM class).
    write_bytes(
        &d,
        "uni.py",
        "ß = 1\nüname = ß\nprint('🎉', üname)\n".as_bytes(),
    );
    // Self-referential symlink in the tree (discovery must not loop).
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(d.join("loop"), d.join("loop"));

    let audit = mollify_core::audit_report(&d);
    // No engine-panic findings: every engine ran to completion.
    assert!(
        !audit.findings.iter().any(|f| f.rule == "engine-panic"),
        "engine panicked on chaos corpus: {:?}",
        audit
            .findings
            .iter()
            .filter(|f| f.rule == "engine-panic")
            .collect::<Vec<_>>()
    );
    // The valid-UTF-8 files were all analyzed (latin.py is known-skipped —
    // documented limitation: non-UTF-8 sources are not decoded).
    assert!(
        audit.summary.files_analyzed >= 7,
        "summary: {:?}",
        audit.summary
    );
    // Per-engine reports survive the same tree.
    let _ = mollify_core::dead_code_report(&d);
    let _ = mollify_core::dupes_report(&d);
    let _ = mollify_core::security_report(&d);
    let _ = mollify_core::complexity_report(&d);
    let _ = mollify_core::deps_report(&d);
    let _ = mollify_core::arch_report(&d);
    let _ = mollify_core::types_report(&d);
    std::fs::remove_dir_all(&d).ok();
}
