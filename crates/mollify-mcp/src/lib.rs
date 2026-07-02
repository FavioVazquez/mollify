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
//! Tools exposed: `mollify_audit`, `mollify_dead_code`, `mollify_deps`,
//! `mollify_arch`, `mollify_complexity`, `mollify_dupes`, `mollify_types`,
//! `mollify_security`, `mollify_coverage`, `mollify_supply_chain`,
//! `mollify_explain`, `mollify_trace`, `mollify_inspect`, `mollify_list`,
//! `mollify_metrics`, `mollify_fix`.
//! Analysis tools accept `{ "path": "<dir>" }` (default ".") and return the
//! kind-discriminated JSON report as text content. (`watch` is a long-running
//! loop and stays CLI-only.)

use camino::Utf8PathBuf;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2025-06-18";
/// Protocol revisions this server can serve. The surface we implement
/// (initialize / ping / tools) is identical across these revisions.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];
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
        if let Some(resp) = handle_line(&line) {
            let s = serde_json::to_string(&resp)?;
            out.write_all(s.as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Parse one stdin line and produce the response, if any. Malformed JSON is a
/// JSON-RPC Parse Error with a null id (per spec), not silence.
pub fn handle_line(line: &str) -> Option<Value> {
    match serde_json::from_str::<Value>(line) {
        Ok(req) => dispatch(&req),
        Err(e) => {
            eprintln!("mollify-mcp: bad JSON on stdin: {e}");
            Some(error(Value::Null, -32700, &format!("parse error: {e}")))
        }
    }
}

/// Pure request→response dispatch (testable). Returns `None` for notifications
/// (messages without an `id`), which must not be answered.
pub fn dispatch(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();

    // Notifications (no id) get no response.
    let id = id?;

    let Some(method) = req.get("method").and_then(|m| m.as_str()) else {
        return Some(error(id, -32600, "invalid request: missing method"));
    };

    match method {
        "initialize" => {
            let client_proto = req
                .get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str());
            // Version negotiation: echo the client's revision only if we can
            // serve it; otherwise answer with ours (per MCP lifecycle spec).
            let proto = match client_proto {
                Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => v,
                _ => PROTOCOL_VERSION,
            };
            Some(result(
                id,
                json!({
                    "protocolVersion": proto,
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
    let coverage_schema = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Project root to analyze (default \".\")." },
            "coverage_file": { "type": "string", "description": "Path to a coverage.py JSON report (`coverage json`)." }
        },
        "required": ["coverage_file"]
    });
    let explain_schema = json!({
        "type": "object",
        "properties": { "rule": { "type": "string", "description": "Rule id to explain (omit to list all rules)." } }
    });
    let trace_schema = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Project root to analyze (default \".\")." },
            "module": { "type": "string", "description": "Module to trace (dotted name or trailing segment)." }
        },
        "required": ["module"]
    });
    let inspect_schema = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Project root to analyze (default \".\")." },
            "file": { "type": "string", "description": "File to inspect (path or trailing fragment)." }
        },
        "required": ["file"]
    });
    let list_schema = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Project root to analyze (default \".\")." },
            "kind": { "type": "string", "enum": ["entry-points", "files", "frameworks"], "description": "What to list (default entry-points)." }
        }
    });
    let fix_schema = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Project root (default \".\")." },
            "apply": { "type": "boolean", "description": "Write the changes (default false = dry-run preview)." }
        }
    });
    let supply_chain_schema = json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Project root to analyze (default \".\")." },
            "advisory_db": { "type": "string", "description": "Advisory DB JSON path (default `.mollify/advisories.json`)." }
        }
    });
    json!([
        { "name": "mollify_audit", "description": "Full unified report across all engines with a 0-100 quality score, as deterministic JSON.", "inputSchema": path_schema },
        { "name": "mollify_dead_code", "description": "Reachability-based unused files and top-level symbols, with confidence tiers.", "inputSchema": path_schema },
        { "name": "mollify_deps", "description": "Dependency hygiene: unused and missing distributions.", "inputSchema": path_schema },
        { "name": "mollify_arch", "description": "Architecture: circular dependencies, layer-boundary violations, and policy violations.", "inputSchema": path_schema },
        { "name": "mollify_complexity", "description": "Cyclomatic + cognitive complexity and churn x complexity hotspots.", "inputSchema": path_schema },
        { "name": "mollify_dupes", "description": "Duplication / clone families (token-based).", "inputSchema": path_schema },
        { "name": "mollify_types", "description": "Type-annotation health: fully-untyped public functions.", "inputSchema": path_schema },
        { "name": "mollify_security", "description": "Security candidates (eval/exec, shell=True, unsafe deserialization, hardcoded secrets, ...).", "inputSchema": path_schema },
        { "name": "mollify_coverage", "description": "Cold-path analysis: reachable functions never executed in a coverage.py JSON report.", "inputSchema": coverage_schema },
        { "name": "mollify_supply_chain", "description": "Match pinned/locked dependency versions against a local advisory DB (vulnerable-dependency).", "inputSchema": supply_chain_schema },
        { "name": "mollify_explain", "description": "Explain a rule id (semantics, confidence, action). Omit `rule` to list all rules.", "inputSchema": explain_schema },
        { "name": "mollify_trace", "description": "A module's import neighborhood: what it imports and what imports it.", "inputSchema": trace_schema },
        { "name": "mollify_inspect", "description": "Per-file evidence bundle: that file's findings plus its import neighborhood.", "inputSchema": inspect_schema },
        { "name": "mollify_list", "description": "Project topology: entry-points, files, or detected frameworks.", "inputSchema": list_schema },
        { "name": "mollify_metrics", "description": "Code metrics: Maintainability Index, Halstead, raw LOC, per-file complexity.", "inputSchema": path_schema },
        { "name": "mollify_fix", "description": "Preview (default) or apply safe auto-fixes — certain unused symbols/imports. Set apply=true to write.", "inputSchema": fix_schema },
    ])
}

fn handle_tool_call(id: Value, req: &Value) -> Value {
    let params = req.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = params.and_then(|p| p.get("arguments"));
    let arg_str = |key: &str| args.and_then(|a| a.get(key)).and_then(|v| v.as_str());
    let path = arg_str("path").unwrap_or(".");
    let root = Utf8PathBuf::from(path);

    // Every tool but `mollify_explain` resolves files under `path`; analyzing a
    // nonexistent root must be a visible tool error, never a clean empty report.
    let uses_root = matches!(
        name,
        "mollify_audit"
            | "mollify_dead_code"
            | "mollify_deps"
            | "mollify_arch"
            | "mollify_complexity"
            | "mollify_dupes"
            | "mollify_types"
            | "mollify_security"
            | "mollify_coverage"
            | "mollify_supply_chain"
            | "mollify_trace"
            | "mollify_inspect"
            | "mollify_list"
            | "mollify_metrics"
            | "mollify_fix"
    );
    if uses_root && !root.is_dir() {
        return tool_error(
            id,
            &format!("project root `{root}` does not exist or is not a directory"),
        );
    }

    use mollify_types::Report;
    let report_json = match name {
        "mollify_audit" => {
            serde_json::to_string_pretty(&Report::Audit(mollify_core::audit_report(&root)))
        }
        "mollify_dead_code" => {
            serde_json::to_string_pretty(&Report::DeadCode(mollify_core::dead_code_report(&root)))
        }
        "mollify_deps" => {
            serde_json::to_string_pretty(&Report::Deps(mollify_core::deps_report(&root)))
        }
        "mollify_arch" => {
            serde_json::to_string_pretty(&Report::Arch(mollify_core::arch_report(&root)))
        }
        "mollify_complexity" => serde_json::to_string_pretty(&Report::Complexity(
            mollify_core::complexity_report(&root),
        )),
        "mollify_dupes" => {
            serde_json::to_string_pretty(&Report::Dupes(mollify_core::dupes_report(&root)))
        }
        "mollify_types" => {
            serde_json::to_string_pretty(&Report::Types(mollify_core::types_report(&root)))
        }
        "mollify_security" => {
            serde_json::to_string_pretty(&Report::Security(mollify_core::security_report(&root)))
        }
        "mollify_coverage" => {
            let Some(cov) = arg_str("coverage_file") else {
                return error(id, -32602, "mollify_coverage requires `coverage_file`");
            };
            serde_json::to_string_pretty(&Report::Coverage(mollify_core::coverage_report(
                &root,
                &Utf8PathBuf::from(cov),
            )))
        }
        "mollify_supply_chain" => {
            let db = arg_str("advisory_db")
                .map(Utf8PathBuf::from)
                .unwrap_or_else(|| root.join(mollify_core::DEFAULT_ADVISORY_DB));
            serde_json::to_string_pretty(&Report::Security(mollify_core::supply_chain_report(
                &root, &db,
            )))
        }
        "mollify_inspect" => {
            let Some(file) = arg_str("file") else {
                return error(id, -32602, "mollify_inspect requires `file`");
            };
            let ins = mollify_core::inspect(&root, file);
            let body = json!({
                "kind": "inspect",
                "file": ins.file,
                "module": ins.module,
                "findings": ins.findings,
                "imports": ins.imports,
                "imported_by": ins.imported_by,
            });
            serde_json::to_string_pretty(&body)
        }
        "mollify_list" => {
            let kind = arg_str("kind").unwrap_or("entry-points");
            let rows = mollify_core::list_topology(&root, kind);
            serde_json::to_string_pretty(&json!({ "kind": "list", "of": kind, "items": rows }))
        }
        "mollify_metrics" => {
            serde_json::to_string_pretty(&Report::Metrics(mollify_core::metrics::report(&root)))
        }
        "mollify_fix" => {
            let do_apply = args
                .and_then(|a| a.get("apply"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let edits = mollify_core::fix::plan(&root);
            let items: Vec<Value> = edits
                .iter()
                .map(|e| {
                    json!({ "path": e.path, "start_line": e.start_line, "end_line": e.end_line, "description": e.description })
                })
                .collect();
            let written = if do_apply {
                match mollify_core::fix::apply(&edits) {
                    Ok(n) => n,
                    Err(e) => {
                        return tool_error(id, &format!("mollify_fix: applying fixes failed: {e}"))
                    }
                }
            } else {
                0
            };
            serde_json::to_string_pretty(&json!({
                "kind": "fix", "applied": do_apply, "count": edits.len(),
                "fixes": items, "written": written,
            }))
        }
        "mollify_explain" => {
            let body = match arg_str("rule") {
                Some(rule) => match mollify_core::explain::text(rule) {
                    Some(t) => json!({ "rule": rule, "explanation": t }),
                    None => json!({ "rule": rule, "error": "unknown rule" }),
                },
                None => json!({ "rules": mollify_core::explain::RULES }),
            };
            serde_json::to_string_pretty(&body)
        }
        "mollify_trace" => {
            let Some(module) = arg_str("module") else {
                return error(id, -32602, "mollify_trace requires `module`");
            };
            let graph = mollify_core::build_graph(&root);
            let body = match mollify_core::trace::module(&graph, module) {
                Some(t) => json!({
                    "kind": "trace", "target": t.target,
                    "imports": t.imports, "imported_by": t.imported_by,
                }),
                None => {
                    json!({ "kind": "trace", "error": format!("no module matching `{module}`") })
                }
            };
            serde_json::to_string_pretty(&body)
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

/// Tool-execution failure: an `isError` result (so the model sees the failure
/// text), not a protocol-level error — per the MCP tools spec.
fn tool_error(id: Value, message: &str) -> Value {
    result(
        id,
        json!({ "content": [ { "type": "text", "text": message } ], "isError": true }),
    )
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
    fn initialize_with_unknown_protocol_answers_with_server_version() {
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"1999-01-01"}});
        let resp = dispatch(&req).unwrap();
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        // A known-but-older revision is echoed back.
        let req = json!({"jsonrpc":"2.0","id":2,"method":"initialize",
            "params":{"protocolVersion":"2024-11-05"}});
        let resp = dispatch(&req).unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn malformed_json_line_is_parse_error_with_null_id() {
        let resp = handle_line("{ not json").unwrap();
        assert_eq!(resp["error"]["code"], -32700);
        assert!(resp["id"].is_null());
    }

    #[test]
    fn missing_or_nonstring_method_is_invalid_request() {
        let resp = dispatch(&json!({"jsonrpc":"2.0","id":5})).unwrap();
        assert_eq!(resp["error"]["code"], -32600);
        let resp = dispatch(&json!({"jsonrpc":"2.0","id":6,"method":42})).unwrap();
        assert_eq!(resp["error"]["code"], -32600);
    }

    #[test]
    fn nonexistent_path_is_tool_error() {
        let req = json!({"jsonrpc":"2.0","id":8,"method":"tools/call",
            "params":{"name":"mollify_audit","arguments":{"path":"/no/such/mollify-root"}}});
        let resp = dispatch(&req).unwrap();
        assert_eq!(resp["result"]["isError"], true);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("/no/such/mollify-root"), "got {text}");
    }

    #[test]
    fn notifications_get_no_response() {
        let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert!(dispatch(&note).is_none());
    }

    #[test]
    fn tools_list_advertises_all_engines() {
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
        let resp = dispatch(&req).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in [
            "mollify_audit",
            "mollify_dead_code",
            "mollify_deps",
            "mollify_arch",
            "mollify_complexity",
            "mollify_dupes",
            "mollify_types",
            "mollify_security",
            "mollify_coverage",
            "mollify_supply_chain",
            "mollify_explain",
            "mollify_trace",
            "mollify_inspect",
            "mollify_list",
            "mollify_metrics",
            "mollify_fix",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
    }

    #[test]
    fn explain_tool_returns_rule_text() {
        let req = json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
            "params":{"name":"mollify_explain","arguments":{"rule":"circular-dependency"}}});
        let resp = dispatch(&req).unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("cycle"));
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
