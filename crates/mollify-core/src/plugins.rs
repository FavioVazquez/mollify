//! Framework awareness — the dominant false-positive killer for Python
//! dead-code analysis (RESEARCH.md §4). A symbol registered with a framework via
//! a decorator (a Flask/FastAPI route, a Celery task, a pytest fixture, a Django
//! signal receiver, a click/typer command, a Pydantic validator, …) is *reached*
//! even with zero in-repo callers.
//!
//! Matching is by the decorator's final path segment, so `app.route`,
//! `router.get`, `bp.cli.command`, and `pytest.fixture` all match. This is a
//! curated data table — extend it freely; it ships as pure data.

use mollify_parse::Definition;

/// Decorator final-segments that mark a definition as a framework entry point.
const ENTRY_DECORATORS: &[&str] = &[
    // Web routes (Flask, FastAPI, Starlette, Sanic, AIOHTTP, Bottle, Quart)
    "route",
    "get",
    "post",
    "put",
    "patch",
    "delete",
    "head",
    "options",
    "websocket",
    "websocket_route",
    "middleware",
    "exception_handler",
    "on_event",
    "before_request",
    "after_request",
    "errorhandler",
    // Task queues (Celery, RQ, Dramatiq, Huey, APScheduler)
    "task",
    "shared_task",
    "periodic_task",
    "actor",
    "scheduled_job",
    "on_message",
    "subscribe",
    // Tests (pytest) — fixtures are injected by name; hooks register implicitly
    "fixture",
    "hookimpl",
    // Django (signals, admin, template tags/filters, management)
    "receiver",
    "register",
    "display",
    "action",
    "simple_tag",
    "filter",
    "inclusion_tag",
    "admin",
    // CLI (click, typer)
    "command",
    "group",
    "callback",
    // Pydantic / dataclasses validation hooks
    "validator",
    "field_validator",
    "root_validator",
    "model_validator",
    "field_serializer",
    "model_serializer",
    "computed_field",
    // Generic plugin/registry/dispatch patterns
    "hook",
    "plugin",
    "rule",
    "event",
    "listener",
    "handler",
    "provides",
    "implementer",
    "setup",
    "teardown",
];

/// True if any of this definition's decorators marks it as a framework entry.
pub fn is_framework_entry(def: &Definition) -> bool {
    def.decorators.iter().any(|d| {
        let seg = d.rsplit('.').next().unwrap_or(d);
        ENTRY_DECORATORS.contains(&seg)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mollify_parse::DefKind;

    fn def(decorators: &[&str]) -> Definition {
        Definition {
            name: "x".into(),
            kind: DefKind::Function,
            line: 1,
            end_line: 2,
            private_by_convention: false,
            decorators: decorators.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn recognizes_framework_decorators() {
        assert!(is_framework_entry(&def(&["app.route"])));
        assert!(is_framework_entry(&def(&["router.get"])));
        assert!(is_framework_entry(&def(&["pytest.fixture"])));
        assert!(is_framework_entry(&def(&["shared_task"])));
        assert!(is_framework_entry(&def(&["receiver"])));
        assert!(is_framework_entry(&def(&["cli.command"])));
        assert!(is_framework_entry(&def(&["field_validator"])));
    }

    #[test]
    fn ignores_plain_decorators() {
        assert!(!is_framework_entry(&def(&["staticmethod"])));
        assert!(!is_framework_entry(&def(&["functools.lru_cache"])));
        assert!(!is_framework_entry(&def(&[])));
    }
}
