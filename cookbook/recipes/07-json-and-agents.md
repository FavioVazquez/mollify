# Recipe 07 — JSON for scripts & AI agents

**Goal:** drive Mollify from a script or hand its evidence to a coding agent.
Every command emits the **same** machine-readable contract, so you build the
integration once.

## The `kind`-discriminated envelope

Add `--format json` to any command. The shape is stable and versioned — clients
switch on `kind` and iterate `findings[]`:

```json
{
  "kind": "audit", "schema_version": "0.1", "quality_score": 80,
  "summary": { "total": 21, "errors": 0, "warnings": 21, "files_analyzed": 7 },
  "findings": [{
    "fingerprint": "unused-export:93948eee", "rule": "unused-export",
    "category": "dead-code", "severity": "warn", "confidence": "certain",
    "reason": "function `_legacy_helper` has no reachable references in the project",
    "location": { "path": "./billing/app.py", "line": 12, "end_line": 14 },
    "actions": [{ "type": "remove-symbol",
                  "description": "Delete unused function `_legacy_helper`",
                  "auto_fixable": true,
                  "suppression_comment": "# mollify: ignore[unused-export]" }]
  }]
}
```

Two invariants worth leaning on: the JSON shape is the public API (clients depend
on `kind`, not Rust internals), and identical input → **byte-identical** output
(sorted before emit) — so it diffs cleanly and caches well.

## Slice it with `jq`

List every *provable* dead-code finding (the safe-to-remove set):

```bash
cd cookbook/sample-project
mollify dead-code --format json \
  | jq -c '.findings[] | select(.confidence=="certain")
           | {rule, path: .location.path, line: .location.line}'
```

```text
{"rule":"unused-import","path":"./billing/app.py","line":1}
{"rule":"unused-export","path":"./billing/app.py","line":12}
{"rule":"unused-import","path":"./billing/services/invoice.py","line":1}
{"rule":"unused-import","path":"./billing/services/invoice.py","line":2}
{"rule":"unused-import","path":"./billing/services/invoice.py","line":3}
```

Other handy one-liners:

```bash
# Just the quality score, for a dashboard or badge
mollify audit --format json | jq '.quality_score'

# Count findings by rule
mollify audit --format json | jq -r '.findings[].rule' | sort | uniq -c | sort -rn

# Every finding's exact suppression comment (no guessing the syntax)
mollify audit --format json \
  | jq -r '.findings[] | "\(.location.path):\(.location.line)  \(.actions[0].suppression_comment // "—")"'
```

## Hand it to a coding agent (MCP)

This is where Mollify shines for AI workflows: instead of an agent reconstructing
"what's unused?" from `grep` and guesswork, it reads **repo truth** over the Model
Context Protocol.

```bash
mollify mcp        # Model Context Protocol server over stdio
```

Mollify ships first-class integrations for **Claude Code, Codex, Cursor, Gemini
CLI, and Devin/Cascade**. Scaffold the version-matched skills, rules, and hooks
straight into a repo:

```bash
mollify init --agent claude     # or: cursor / gemini / codex / cascade
mollify init --all              # every supported agent at once
```

Because findings are deterministic evidence with stable fingerprints and
confidence tiers — not LLM opinions — an agent can act on `certain` findings
automatically and surface `likely`/`uncertain` ones for a human. The agent reads;
it doesn't invent.

## Editor diagnostics (LSP)

Same engine, real-time, in your editor:

```bash
mollify lsp        # Language Server over stdio; point any LSP client at it
```

Results match CI exactly, because it's the same deterministic audit underneath.

---

That's the tour. You've gone from one `audit` command to a CI gate, a cleanup
workflow, a codebase map, and an agent integration — all from a single binary.
Point it at your own project next:

```bash
mollify audit --path /path/to/your/project
```
