//! # mollify-mcp
//!
//! A minimal, dependency-light **Model Context Protocol** server over stdio
//! (newline-delimited JSON-RPC 2.0). This is the single server every agent
//! front-end (Claude Code, Codex, Cursor, Gemini CLI, Devin/Cascade) registers —
//! "one MCP server, many front-ends".
//!
//! Determinism/protocol invariant: **all logging goes to stderr**; stdout
//! carries only protocol messages.
//!
//! Tools exposed: `mollify_audit`, `mollify_dead_code`, `mollify_deps`. Each
//! accepts `{ "path": "<dir>" }` (default ".") and returns the kind-discriminated
//! JSON report as text content.

use camino::Utf8PathBuf;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "mollify";

/// Run the stdio server loop until EOF. Returns on clean stdin close.
pub fn run() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("mollify-mcp: bad JSON on stdin: {e}");
                continue;
            }
        };
        if let Some(resp) = dispatch(&req) {
            let s = serde_json::to_string(&resp)?;
            out.write_all(s.as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Pure request→response dispatch (testable). Returns `None` for notifications
/// (messages without an `id`), which must not be answered.
pub fn dispatch(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

    // Notifications (no id) get no response.
    let id = id?;

    match method {
        "initialize" => {
            let client_proto = req
                .get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str())
                .unwrap_or(PROTOCOL_VERSION)
                .to_string();
            Some(result(
                id,
                json!({
                    "protocolVersion": client_proto,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") }
                }),
            ))
        }
        "ping" => Some(result(id, json!({}))),
        "tools/list" => Some(result(id, json!({ "tools": tool_list() }))),
        "tools/call" => Some(handle_tool_call(id, req)),
        other => Some(error(id, -32601, &format!("method not found: {other}"))),
    }
}

fn tool_list() -> Value {
    let path_schema = json!({
        "type": "object",
        "properties": { "path": { "type": "string", "description": "Project root to analyze (default \".\")." } }
    });
    json!([
        { "name": "mollify_audit", "description": "Full unified report (dead code + dependency hygiene) with a 0-100 quality score, as deterministic JSON.", "inputSchema": path_schema },
        { "name": "mollify_dead_code", "description": "Reachability-based unused files and top-level symbols, with confidence tiers.", "inputSchema": path_schema },
        { "name": "mollify_deps", "description": "Dependency hygiene: unused and missing distributions from pyproject.toml.", "inputSchema": path_schema },
    ])
}

fn handle_tool_call(id: Value, req: &Value) -> Value {
    let params = req.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let path = params
        .and_then(|p| p.get("arguments"))
        .and_then(|a| a.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let root = Utf8PathBuf::from(path);

    let report_json = match name {
        "mollify_audit" => {
            let r = mollify_core::audit_report(&root);
            serde_json::to_string_pretty(&mollify_types::Report::Audit(r))
        }
        "mollify_dead_code" => {
            let r = mollify_core::dead_code_report(&root);
            serde_json::to_string_pretty(&mollify_types::Report::DeadCode(r))
        }
        "mollify_deps" => {
            let r = mollify_core::deps_report(&root);
            serde_json::to_string_pretty(&mollify_types::Report::Deps(r))
        }
        other => return error(id, -32602, &format!("unknown tool: {other}")),
    };

    match report_json {
        Ok(text) => result(
            id,
            json!({ "content": [ { "type": "text", "text": text } ], "isError": false }),
        ),
        Err(e) => error(id, -32603, &format!("analysis failed: {e}")),
    }
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_echoes_protocol_and_advertises_tools_capability() {
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}});
        let resp = dispatch(&req).unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "mollify");
        assert_eq!(resp["result"]["protocolVersion"], "2025-06-18");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn notifications_get_no_response() {
        let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert!(dispatch(&note).is_none());
    }

    #[test]
    fn tools_list_has_the_three_tools() {
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
        let resp = dispatch(&req).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"mollify_audit"));
        assert!(names.contains(&"mollify_dead_code"));
        assert!(names.contains(&"mollify_deps"));
    }

    #[test]
    fn unknown_method_is_jsonrpc_error() {
        let req = json!({"jsonrpc":"2.0","id":3,"method":"does/not/exist"});
        let resp = dispatch(&req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn tool_call_returns_kind_discriminated_text() {
        // Run against a tiny on-disk project.
        let d = std::env::temp_dir().join(format!("mollify-mcp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("__main__.py"), "print('hi')\n").unwrap();
        std::fs::write(d.join("lib.py"), "def dead():\n    return 1\n").unwrap();
        let req = json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"mollify_audit","arguments":{"path": d.to_str().unwrap()}}});
        let resp = dispatch(&req).unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"kind\": \"audit\""));
        assert!(text.contains("unused-export"));
        std::fs::remove_dir_all(&d).ok();
    }
}
