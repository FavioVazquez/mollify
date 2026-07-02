//! # mollify-lsp
//!
//! A minimal, dependency-light **Language Server Protocol** server over stdio
//! (`Content-Length`-framed JSON-RPC 2.0). It gives editors real-time mollify
//! diagnostics: on open/save it runs the unified audit for the workspace and
//! publishes per-file diagnostics; on **didChange** it runs fast file-local
//! analysis on the live (unsaved) buffer for keystroke-latency feedback.
//!
//! Protocol invariant: **all logging goes to stderr**; stdout carries only
//! framed protocol messages. Determinism is inherited from `mollify-core`.

use camino::Utf8PathBuf;
use mollify_types::{Finding, Severity};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// Run the stdio LSP loop until `exit`. Returns on clean shutdown.
pub fn run() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut server = Server::default();

    while let Some(msg) = read_message(&mut reader)? {
        let responses = server.handle(&msg);
        for resp in responses {
            write_message(&mut out, &resp)?;
        }
        if server.exit {
            break;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Server {
    /// Workspace root resolved from `initialize`.
    root: Option<Utf8PathBuf>,
    exit: bool,
}

/// Extract the full document text from a didOpen/didChange notification (we
/// advertise Full sync, so `contentChanges[0].text` is the whole buffer).
fn doc_text(msg: &Value) -> Option<String> {
    let params = msg.get("params")?;
    if let Some(t) = params
        .get("textDocument")
        .and_then(|d| d.get("text"))
        .and_then(|t| t.as_str())
    {
        return Some(t.to_string());
    }
    params
        .get("contentChanges")
        .and_then(|c| c.as_array())
        .and_then(|a| a.last())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}

/// The `textDocument.uri` of a request/notification's params.
fn doc_uri(msg: &Value) -> Option<&str> {
    msg.get("params")?.get("textDocument")?.get("uri")?.as_str()
}

impl Server {
    /// Handle one incoming message, returning zero or more messages to send.
    fn handle(&mut self, msg: &Value) -> Vec<Value> {
        let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
            return vec![]; // a response (we send no requests) or garbage
        };
        let id = msg.get("id").cloned();
        match method {
            "initialize" => {
                self.root = workspace_root(msg);
                vec![result(
                    id.unwrap_or(Value::Null),
                    json!({
                        "capabilities": {
                            // 1 = Full document sync; we (re)analyze on open/save.
                            "textDocumentSync": { "openClose": true, "change": 1, "save": true }
                        },
                        "serverInfo": { "name": "mollify", "version": env!("CARGO_PKG_VERSION") }
                    }),
                )]
            }
            "shutdown" => vec![result(id.unwrap_or(Value::Null), Value::Null)],
            "exit" => {
                self.exit = true;
                vec![]
            }
            // Open/save → full workspace audit (cross-file rules).
            "textDocument/didOpen" | "textDocument/didSave" => self.diagnose(msg),
            // Change → fast file-local diagnostics from the live buffer.
            "textDocument/didChange" => self.diagnose_buffer(msg),
            // Close → publish an empty set so the client drops stale diagnostics.
            "textDocument/didClose" => match doc_uri(msg) {
                Some(uri) => vec![notification(
                    "textDocument/publishDiagnostics",
                    json!({ "uri": uri, "diagnostics": [] }),
                )],
                None => vec![],
            },
            // initialized and other notifications: no response. Unknown
            // *requests* (they carry an id) must be answered, not dropped.
            _ => match id {
                Some(id) => vec![error(id, -32601, &format!("method not found: {method}"))],
                None => vec![],
            },
        }
    }

    /// Run the audit for the workspace and publish diagnostics for the document
    /// referenced by the notification.
    fn diagnose(&self, msg: &Value) -> Vec<Value> {
        let Some(uri) = doc_uri(msg) else {
            return vec![];
        };
        let Some(file_abs) = uri_to_path(uri) else {
            return vec![];
        };
        // Resolve the project root: the workspace root, else the file's parent.
        let root = self
            .root
            .clone()
            .unwrap_or_else(|| Utf8PathBuf::from(file_abs.parent().unwrap_or(&file_abs)));
        let report = mollify_core::audit_report(&root);
        let diagnostics: Vec<Value> = report
            .findings
            .iter()
            .filter(|f| same_file(&root, &f.location.path, &file_abs))
            .map(to_diagnostic)
            .collect();
        vec![notification(
            "textDocument/publishDiagnostics",
            json!({ "uri": uri, "diagnostics": diagnostics }),
        )]
    }
}

impl Server {
    /// Live file-local diagnostics from the edited buffer (no disk read).
    fn diagnose_buffer(&self, msg: &Value) -> Vec<Value> {
        let Some(uri) = doc_uri(msg) else {
            return vec![];
        };
        let (Some(path), Some(text)) = (uri_to_path(uri), doc_text(msg)) else {
            return vec![];
        };
        let diagnostics: Vec<Value> = mollify_core::analyze_text(&path, &text)
            .iter()
            .map(to_diagnostic)
            .collect();
        vec![notification(
            "textDocument/publishDiagnostics",
            json!({ "uri": uri, "diagnostics": diagnostics }),
        )]
    }
}

/// Does a finding's workspace-relative path refer to the open absolute file?
fn same_file(root: &Utf8PathBuf, finding_path: &camino::Utf8Path, file_abs: &Utf8PathBuf) -> bool {
    let joined = root.join(finding_path);
    let fa = file_abs.as_str();
    joined.as_str() == fa || fa.ends_with(finding_path.as_str()) || finding_path.as_str() == fa
}

fn to_diagnostic(f: &Finding) -> Value {
    let line = f.location.line.saturating_sub(1);
    let end_line = f
        .location
        .end_line
        .unwrap_or(f.location.line)
        .saturating_sub(1);
    // `column` is 1-based (0 = unknown) → 0-based. LSP counts UTF-16 code
    // units; engines emit column 0 today, so no byte→UTF-16 mapping is needed.
    let character = f.location.column.saturating_sub(1);
    let severity = match f.severity {
        Severity::Error => 1,
        Severity::Warn => 2,
        // `Off` and any future severity (#[non_exhaustive]) → Hint.
        _ => 4,
    };
    json!({
        "range": {
            "start": { "line": line, "character": character },
            // End at the start of the line *after* the last finding line, so
            // the last line's content is covered and the range never reverses.
            "end": { "line": end_line.max(line) + 1, "character": 0 }
        },
        "severity": severity,
        "source": "mollify",
        "code": f.rule,
        "message": f.reason,
    })
}

/// Resolve the workspace root from `initialize` params (`rootUri` /
/// `workspaceFolders[0].uri` / `rootPath`).
fn workspace_root(msg: &Value) -> Option<Utf8PathBuf> {
    let params = msg.get("params")?;
    if let Some(uri) = params.get("rootUri").and_then(|u| u.as_str()) {
        if let Some(p) = uri_to_path(uri) {
            return Some(p);
        }
    }
    if let Some(uri) = params
        .get("workspaceFolders")
        .and_then(|w| w.as_array())
        .and_then(|a| a.first())
        .and_then(|f| f.get("uri"))
        .and_then(|u| u.as_str())
    {
        if let Some(p) = uri_to_path(uri) {
            return Some(p);
        }
    }
    params
        .get("rootPath")
        .and_then(|p| p.as_str())
        .map(Utf8PathBuf::from)
}

/// Convert a `file://` URI to a filesystem path (basic percent-decoding).
fn uri_to_path(uri: &str) -> Option<Utf8PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Strip an authority component if present (file://host/path → /path).
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(i) => &rest[i..],
        None => rest,
    };
    Some(Utf8PathBuf::from(percent_decode(path)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// Read one `Content-Length`-framed JSON-RPC message. `Ok(None)` only at clean
/// EOF; a header block without a parseable `Content-Length` is logged and
/// skipped (an editor hiccup must not look like EOF and end the session).
fn read_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Value>> {
    loop {
        let mut content_length: Option<usize> = None;
        let mut saw_header = false;
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                return Ok(None); // clean EOF
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                if !saw_header {
                    continue; // stray blank line between messages
                }
                break; // end of headers
            }
            saw_header = true;
            if let Some(v) = trimmed
                .strip_prefix("Content-Length:")
                .or_else(|| trimmed.strip_prefix("content-length:"))
            {
                content_length = v.trim().parse().ok();
            }
        }
        let Some(len) = content_length else {
            eprintln!("mollify-lsp: header block without a valid Content-Length; skipping");
            continue;
        };
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)?;
        match serde_json::from_slice(&buf) {
            Ok(v) => return Ok(Some(v)),
            Err(e) => {
                eprintln!("mollify-lsp: bad JSON body: {e}");
                return Ok(Some(json!({})));
            }
        }
    }
}

/// Write one `Content-Length`-framed JSON-RPC message.
fn write_message<W: Write>(out: &mut W, msg: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(out, "Content-Length: {}\r\n\r\n", body.len())?;
    out.write_all(&body)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_advertises_capabilities() {
        let mut s = Server::default();
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///tmp/x"}});
        let resp = s.handle(&req);
        assert_eq!(resp[0]["result"]["serverInfo"]["name"], "mollify");
        assert!(resp[0]["result"]["capabilities"]["textDocumentSync"].is_object());
        assert_eq!(s.root.as_deref().map(|p| p.as_str()), Some("/tmp/x"));
    }

    #[test]
    fn unknown_request_gets_method_not_found_but_notifications_are_ignored() {
        let mut s = Server::default();
        let req = json!({"jsonrpc":"2.0","id":9,"method":"textDocument/hover","params":{}});
        let resp = s.handle(&req);
        assert_eq!(resp.len(), 1);
        assert_eq!(resp[0]["id"], 9);
        assert_eq!(resp[0]["error"]["code"], -32601);
        let note = json!({"jsonrpc":"2.0","method":"$/setTrace","params":{}});
        assert!(s.handle(&note).is_empty());
        // A response-shaped message (no method) is also not answered.
        let response = json!({"jsonrpc":"2.0","id":1,"result":{}});
        assert!(s.handle(&response).is_empty());
    }

    #[test]
    fn did_close_publishes_empty_diagnostics() {
        let mut s = Server::default();
        let note = json!({"jsonrpc":"2.0","method":"textDocument/didClose",
            "params":{"textDocument":{"uri":"file:///tmp/x.py"}}});
        let out = s.handle(&note);
        assert_eq!(out[0]["method"], "textDocument/publishDiagnostics");
        assert_eq!(out[0]["params"]["uri"], "file:///tmp/x.py");
        assert!(out[0]["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn diagnostic_range_is_never_reversed_and_covers_last_line() {
        let finding = |line: u32, column: u32, end_line: Option<u32>| Finding {
            fingerprint: "r:0000".into(),
            rule: "r".into(),
            category: mollify_types::Category::DeadCode,
            severity: Severity::Warn,
            confidence: mollify_types::Confidence::Certain,
            attribution: None,
            reason: "test".into(),
            location: mollify_types::Location {
                path: "a.py".into(),
                line,
                column,
                end_line,
            },
            actions: vec![],
        };
        // 1-based column 5 → 0-based character 4; end covers the whole line.
        let d = to_diagnostic(&finding(3, 5, None));
        assert_eq!(d["range"]["start"], json!({"line": 2, "character": 4}));
        assert_eq!(d["range"]["end"], json!({"line": 3, "character": 0}));
        // Unknown column (0) stays 0; multi-line end covers the final line.
        let d = to_diagnostic(&finding(3, 0, Some(5)));
        assert_eq!(d["range"]["start"], json!({"line": 2, "character": 0}));
        assert_eq!(d["range"]["end"], json!({"line": 5, "character": 0}));
        // end_line < line can never reverse the range.
        let d = to_diagnostic(&finding(3, 0, Some(1)));
        assert_eq!(d["range"]["end"], json!({"line": 3, "character": 0}));
    }

    #[test]
    fn read_message_skips_malformed_headers_instead_of_eof() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"shutdown"}"#;
        let stream = format!(
            "X-Broken-Header: yes\r\n\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut reader = std::io::BufReader::new(stream.as_bytes());
        let msg = read_message(&mut reader)
            .unwrap()
            .expect("must survive the bad header block");
        assert_eq!(msg["method"], "shutdown");
        // The stream end is still a clean EOF.
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn uri_roundtrip_and_decode() {
        assert_eq!(
            uri_to_path("file:///home/u/a%20b.py").unwrap().as_str(),
            "/home/u/a b.py"
        );
    }

    #[test]
    fn diagnose_buffer_reports_file_local_findings_live() {
        let s = Server::default();
        // Unsaved buffer with a hardcoded secret + an unused local — no disk.
        let text =
            "def f():\n    api_key = \"sk-abcdefghij\"\n    dead = compute()\n    return 1\n";
        let note = json!({"jsonrpc":"2.0","method":"textDocument/didChange",
            "params":{"textDocument":{"uri":"file:///tmp/buf.py"},
                      "contentChanges":[{"text": text}]}});
        let out = s.diagnose_buffer(&note);
        let diags = out[0]["params"]["diagnostics"].as_array().unwrap();
        let codes: Vec<&str> = diags.iter().filter_map(|d| d["code"].as_str()).collect();
        assert!(codes.contains(&"hardcoded-secret"), "got {codes:?}");
        assert!(codes.contains(&"unused-variable"), "got {codes:?}");
    }

    #[test]
    fn publishes_diagnostics_for_open_file() {
        let d = std::env::temp_dir().join(format!("mollify-lsp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        std::fs::write(d.join("lib.py"), "def _dead():\n    return 1\n").unwrap();
        let s = Server {
            root: Utf8PathBuf::from_path_buf(d.clone()).ok(),
            exit: false,
        };
        let uri = format!("file://{}/lib.py", d.to_string_lossy());
        let note = json!({"jsonrpc":"2.0","method":"textDocument/didOpen",
            "params":{"textDocument":{"uri": uri}}});
        let out = s.diagnose(&note);
        assert_eq!(out[0]["method"], "textDocument/publishDiagnostics");
        let diags = out[0]["params"]["diagnostics"].as_array().unwrap();
        assert!(!diags.is_empty(), "expected diagnostics for lib.py");
        assert_eq!(diags[0]["source"], "mollify");
        std::fs::remove_dir_all(&d).ok();
    }
}
