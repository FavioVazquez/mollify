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

impl Server {
    /// Handle one incoming message, returning zero or more messages to send.
    fn handle(&mut self, msg: &Value) -> Vec<Value> {
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
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
            // initialized and other notifications: no response.
            _ => vec![],
        }
    }

    /// Run the audit for the workspace and publish diagnostics for the document
    /// referenced by the notification.
    fn diagnose(&self, msg: &Value) -> Vec<Value> {
        let Some(uri) = msg
            .get("params")
            .and_then(|p| p.get("textDocument"))
            .and_then(|d| d.get("uri"))
            .and_then(|u| u.as_str())
        else {
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
        let Some(uri) = msg
            .get("params")
            .and_then(|p| p.get("textDocument"))
            .and_then(|d| d.get("uri"))
            .and_then(|u| u.as_str())
        else {
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
    let severity = match f.severity {
        Severity::Error => 1,
        Severity::Warn => 2,
        Severity::Off => 4,
    };
    json!({
        "range": {
            "start": { "line": line, "character": f.location.column },
            "end": { "line": end_line.max(line), "character": 0 }
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

fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// Read one `Content-Length`-framed JSON-RPC message. `Ok(None)` at clean EOF.
fn read_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(v) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            content_length = v.trim().parse().ok();
        }
    }
    let Some(len) = content_length else {
        return Ok(None);
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    match serde_json::from_slice(&buf) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            eprintln!("mollify-lsp: bad JSON body: {e}");
            Ok(Some(json!({})))
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
