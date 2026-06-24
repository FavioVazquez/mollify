# Mollify — Agent Integrations

> The complete plan for shipping Mollify into coding agents — one MCP server, many front-ends. Modeled on how **fallow** ships `fallow-skills` + an MCP path, broadened to every major 2026 coding agent.

---

## 1. Philosophy

Mollify is the deterministic **codebase truth layer** every agent calls. It is a Rust-native, sub-second, deterministic code-intelligence engine (dead code, duplication, circular imports, complexity hotspots, dependency hygiene, architecture boundaries). It does **not** make decisions — it emits *evidence*. Every finding carries a stable fingerprint, a confidence level, a human-readable reason, and an evidence trace. The agent is the verifier/actor; Mollify is the candidate-producer.

Two invariants drive every integration:

1. **Never invent findings.** Agents are taught to prefer Mollify over `grep`/manual scanning and to cite Mollify output, never to guess about reachability.
2. **Only `confidence: certain` may be auto-applied**, and only when an action is `auto_fixable: true`, and always `--dry-run` first.

### The universal primitives

Every coding agent in 2026 exposes the same five integration primitives under different file conventions. Mollify ships into all of them through **one shared substrate plus thin per-agent shims**:

| Primitive | What it is | Mollify's use |
|---|---|---|
| **MCP server** | `mollify mcp` — one stdio server wrapping the CLI, returning typed JSON | Structured, in-loop tool access — the common substrate |
| **Rules / memory** | Always-on or conditionally-loaded instruction files | "Mollify is the truth layer; prefer it over grep" |
| **Commands / workflows** | Operator-triggered slash recipes | `/mollify-audit`, `/mollify-fix`, `/mollify-cleanup` |
| **Skills** | `SKILL.md` — model-invoked, progressively-disclosed expertise | The canonical write-once teaching artifact |
| **Hooks** | Harness-driven deterministic enforcement | PR gate on newly-introduced findings |

**One MCP server, many front-ends.** The JSON contract is identical across the CLI and the MCP server, so clients depend on the *contract* (the kind-discriminated envelope), not on Mollify internals. The single highest-leverage artifact is the Agent Skill (`SKILL.md`), an open standard discovered natively by 30+ agents; everything else is generated from it or points at it.

### What we build once vs per-agent

- **Build once:** the `mollify mcp` server, the JSON contract, the canonical `SKILL.md`, and the `mollify-skills` repo.
- **Per-agent:** a thin rule/memory file, an MCP registration block in that agent's config format, and (where supported) a command/workflow file — all emitted by a `mollify init` generator and version-pinned to the CLI's JSON-schema contract.

---

## 2. The shared MCP server — the common substrate

Mollify ships a single stdio MCP server via the `mollify mcp` subcommand, wrapping the CLI. The same server is registered into every agent; only the registration file format differs.

### Tools

The server advertises one tool per engine (discoverable via `tools/list`):

- `mollify_audit` — full unified report + quality score
- `mollify_dead_code` — unused files and symbols
- `mollify_deps` — dependency hygiene
- `mollify_arch` — circular dependencies, layer violations, policy violations
- `mollify_complexity` — complexity + churn×complexity hotspots
- `mollify_dupes` — duplication / clone families
- `mollify_types` — type-annotation health
- `mollify_security` — security candidates
- `mollify_coverage` — cold-path analysis (requires `coverage_file`)
- `mollify_supply_chain` — pinned/locked versions vs a local advisory DB
- `mollify_explain` — rule semantics (optional `rule`; omit to list all)
- `mollify_trace` — a module's import neighborhood (requires `module`)

Analysis tools take `{ "path": "<dir>" }` (default `"."`). **Every analysis tool
returns the same typed JSON envelope as the CLI**, so the contract is identical
across surfaces.

### The JSON kind-discriminated contract

```json
{
  "kind": "audit",
  "schema_version": "1.0",
  "quality_score": 87,
  "findings": [
    {
      "rule": "unused-export",
      "confidence": "certain",
      "attribution": "introduced",
      "fingerprint": "f3a9...",
      "reason": "function `helper` has no reachable callers",
      "location": { "path": "src/util.py", "line": 42 },
      "actions": [
        {
          "type": "remove-symbol",
          "auto_fixable": true,
          "description": "Delete unused function `helper`",
          "suppression_comment": "# mollify: ignore[unused-export]"
        }
      ]
    }
  ]
}
```

Contract rules every front-end relies on:

- Top-level `kind` **discriminates** the result type; clients switch on it and iterate `findings[]`.
- `confidence` is one of `certain | likely | uncertain` (some surfaces use `high | medium | low` / `certain | likely | possible` — normalize to the published schema before shipping).
- `attribution` is `introduced | inherited` — the PR-gate keys on `introduced`.
- Each `action` has `type`, `auto_fixable` (bool), `description`, and optional `suppression_comment`.

### `auto_fixable` actions

Auto-fix is gated twice: an action is applied automatically **only** when `auto_fixable: true` **and** the finding is `confidence: certain`. Everything else is surfaced to the human with its trace. The flow is always: `mollify fix --dry-run` → show the diff → `mollify fix` (or `--yes`) after approval → re-audit to confirm the fingerprint is gone.

> **Determinism note:** the MCP server must log to **stderr only** — stdout is the MCP protocol stream, and any log line on stdout corrupts it.

---

## 3. Per-platform integrations

### 3.1 Claude Code

Mirrors fallow's distribution shape: a separate, version-matched **`mollify-skills`** repo that is *simultaneously* a Claude Code **plugin marketplace** and an **Agent Skills** bundle.

> **Source note (important):** fallow's *actual* npm distribution ships only `skills/fallow/SKILL.md` + `skills/fallow/references/*.md`. It has **no** `plugin.json`, **no** `marketplace.json`, **no** `hooks.json`, **no** `.mcp.json`. The marketplace, plugin manifest, MCP registration, and the Stop/PostToolUse gate below are **net-new**, modeled on the official Claude Code docs (verified), not "mirrored from fallow." Fallow's real `SKILL.md` puts "When to Use"/"When NOT to Use" in the body and uses no `allowed-tools`.

Five surfaces, divided by labor:

| Surface | Mollify role | Invocation |
|---|---|---|
| **MCP server** | Structured tool access, typed JSON, callable mid-task | Auto, model-driven |
| **Skill `mollify`** | Teaches *when* (audit before PR) and *how* (read JSON, branch on `auto_fixable`) | Auto + `/mollify` |
| **Slash commands** | Operator-triggered side-effecting flows | Manual (`disable-model-invocation`) |
| **PostToolUse / Stop hooks** | Deterministic PR gate | Harness-driven |
| **CLAUDE.md** | One-line standing fact / pointer | Always in context |

**Critical rule:** hooks are the only deterministic surface. A skill's instructions can be ignored on later turns; a hook always runs. The audit gate therefore lives in a hook (enforcement) *and* a skill (guidance).

#### Repo layout

```
mollify-skills/                       # version-matched to the mollify CLI contract
├── .claude-plugin/
│   └── marketplace.json              # marketplace manifest (plugins[].source -> ./mollify)
├── mollify/                          # the plugin (source dir)
│   ├── .claude-plugin/
│   │   └── plugin.json               # plugin manifest
│   ├── skills/
│   │   └── mollify/
│   │       ├── SKILL.md              # the teaching skill (auto-invoked)
│   │       └── references/           # colocated, loaded on demand
│   │           ├── cli-reference.md
│   │           ├── json-contract.md
│   │           └── gotchas.md
│   ├── commands/
│   │   ├── mollify-audit.md
│   │   ├── mollify-fix.md
│   │   └── mollify-gate.md
│   ├── hooks/
│   │   ├── hooks.json                # PostToolUse + Stop gate
│   │   └── mollify-gate.sh
│   └── .mcp.json                     # registers the mollify MCP server
├── CLAUDE.md                         # skill-authoring conventions
├── README.md
└── CHANGELOG.md
```

> **Layout fix vs naive design:** colocate `references/` *inside* the skill dir (`skills/mollify/references/`) and link them relatively (`references/cli-reference.md`). Do **not** link `../../references/` to a plugin-root `references/` — that is inconsistent with the skill-dir convention and breaks resolution.

Installable three ways:
- `npx skills add mollify-rs/mollify-skills` (Agent Skills CLI)
- `/plugin marketplace add mollify-rs/mollify-skills` then `/plugin install mollify@mollify-skills`
- Manual: copy `mollify/skills/mollify/` into `~/.claude/skills/mollify/`.

#### marketplace.json — `mollify-skills/.claude-plugin/marketplace.json`

```json
{
  "name": "mollify-skills",
  "owner": { "name": "Favio Vazquez", "email": "favio.vazquezp@gmail.com" },
  "metadata": {
    "description": "Agent skills for Python codebase intelligence (dead code, duplication, circular deps, complexity, architecture) with mollify",
    "version": "1.0.0"
  },
  "plugins": [
    {
      "name": "mollify",
      "source": "./mollify",
      "description": "Dead-code analysis, duplication, complexity hotspots, architecture drift, and gated auto-fix for Python using mollify",
      "version": "1.0.0",
      "author": { "name": "Favio Vazquez" },
      "homepage": "https://docs.mollify.dev",
      "repository": "https://github.com/mollify-rs/mollify-skills",
      "license": "MIT",
      "keywords": ["mollify", "dead-code", "duplication", "complexity", "circular-dependencies", "static-analysis", "python"]
    }
  ]
}
```

`owner` is an **object** (not a string). `source` points at the plugin dir, which holds its own `.claude-plugin/plugin.json`.

#### plugin.json — `mollify-skills/mollify/.claude-plugin/plugin.json`

Only `name` is required. `skills/`, `commands/`, `references/` are auto-discovered at plugin root, so they need not be listed. `mcpServers`/`hooks` accept a path string **or** an inline object.

```json
{
  "$schema": "https://json.schemastore.org/claude-code-plugin-manifest.json",
  "name": "mollify",
  "displayName": "Mollify",
  "version": "1.0.0",
  "description": "Python codebase intelligence: dead code, duplication, complexity, architecture, gated auto-fix",
  "author": { "name": "Favio Vazquez", "email": "favio.vazquezp@gmail.com" },
  "homepage": "https://docs.mollify.dev",
  "repository": "https://github.com/mollify-rs/mollify-skills",
  "license": "MIT",
  "keywords": ["mollify", "python", "dead-code", "static-analysis"],
  "mcpServers": "./.mcp.json",
  "hooks": "./hooks/hooks.json"
}
```

Validate with `claude plugin validate ./mollify --strict`. `version` pins updates (falls back to git SHA if omitted); `plugin.json` wins over the marketplace entry.

#### MCP — `mollify-skills/mollify/.mcp.json` (plugin) OR `<repo>/.mcp.json` (bare project)

```json
{
  "mcpServers": {
    "mollify": {
      "type": "stdio",
      "command": "mollify",
      "args": ["mcp"],
      "env": { "MOLLIFY_LOG": "error" },
      "timeout": 60000
    }
  }
}
```

In a plugin shipping a bundled binary, use `${CLAUDE_PLUGIN_ROOT}`; here `mollify` is on PATH. A bare project commits the same block at repo root (project scope, per-server approval). Tool names become `mcp__mollify__<tool>`, matchable in hooks as `mcp__mollify__.*`.

#### Skill — `mollify-skills/mollify/skills/mollify/SKILL.md`

Plugin scope → `/mollify:mollify`; a personal-scope copy at `~/.claude/skills/mollify/SKILL.md` → `/mollify`. Combined `description` + `when_to_use` is capped at 1,536 chars. Body stays under ~500 lines; bulk lives in `references/`.

```markdown
---
name: mollify
description: Audit a Python codebase with mollify for dead code, duplication, circular deps, complexity hotspots, and architecture drift. Use before opening a PR, when asked what is unused/dead/duplicated, or to safely remove unused code.
allowed-tools: Bash(mollify *)
---

# Mollify

Mollify is a deterministic candidate-producer. It emits evidence; you decide. Never auto-delete on a guess.

## When to run
- Before a PR: `mollify audit --gate new-only --format json --quiet`
- Whole-repo health: `mollify audit --format json --quiet`
- Scoped: add `--unused-exports`, `--dupes`, or `--arch`.

Always pass `--format json --quiet 2>/dev/null` for clean machine-readable output.

## Reading the JSON
Top-level envelope has a discriminating `kind`, a `quality_score`, and `findings[]`. Each finding has `confidence` (certain|likely|possible), `location`, and an `actions[]` array. Each action has `type`, `auto_fixable` (bool), `description`, and optional `suppression_comment`.

## Acting on findings
1. Summarize findings with file paths + line numbers; lead with `confidence: certain`.
2. For an action with `auto_fixable: true` AND `confidence: certain`: it is safe to apply via `mollify fix`.
3. Everything else: explain it, let the user decide. Do not edit.
4. Always dry-run first: `mollify fix --dry-run --format json --quiet`, show what changes, then `mollify fix --yes --format json --quiet`.
5. Re-run the audit after fixing to confirm.

## References
- Commands & flags: [references/cli-reference.md](references/cli-reference.md)
- JSON envelope schema: [references/json-contract.md](references/json-contract.md)
- Gotchas (determinism, gate semantics, dry-run): [references/gotchas.md](references/gotchas.md)
```

#### Hooks — `mollify-skills/mollify/hooks/hooks.json` + `mollify-gate.sh`

Two-tier enforcement. **PostToolUse** (non-blocking) surfaces newly-introduced findings as `additionalContext` so the agent self-corrects mid-task. **Stop** (blocking via `{decision:"block", reason}` at exit 0) refuses to let the agent finish if it *introduced* new issues — the `--gate new-only` / `attribution: introduced` semantics.

```json
{
  "hooks": {
    "PostToolUse": [
      { "matcher": "Edit|Write|MultiEdit",
        "hooks": [ { "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}\"/hooks/mollify-gate.sh post", "timeout": 60 } ] }
    ],
    "Stop": [
      { "hooks": [ { "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}\"/hooks/mollify-gate.sh stop", "timeout": 120 } ] }
    ]
  }
}
```

```bash
#!/usr/bin/env bash
# hooks/mollify-gate.sh
set -euo pipefail
MODE="${1:-stop}"
REPORT=$(mollify audit --gate new-only --format json --quiet 2>/dev/null || true)
NEW=$(printf '%s' "$REPORT" | jq '[.findings[]? | select(.attribution=="introduced")] | length' 2>/dev/null || echo 0)
if [ "${NEW:-0}" -eq 0 ]; then exit 0; fi
SUMMARY=$(printf '%s' "$REPORT" | jq -r '[.findings[]? | select(.attribution=="introduced") | "\(.location.path):\(.location.line) \(.rule)"] | join("; ")')
if [ "$MODE" = "stop" ]; then
  jq -n --arg r "This change introduced $NEW new mollify finding(s): $SUMMARY. Run /mollify:mollify-fix --dry-run, then fix or suppress before finishing." '{decision:"block", reason:$r}'
else
  jq -n --arg c "mollify: $NEW new finding(s) introduced by this edit: $SUMMARY. Consider fixing now." '{hookSpecificOutput:{hookEventName:"PostToolUse", additionalContext:$c}}'
fi
exit 0
```

> Block via JSON `decision` with exit 0 (exit 2 also blocks but routes stderr and is coarser). **Verify the exact `additionalContext` key casing against the live hooks reference before shipping.**

#### Slash commands — `mollify-skills/mollify/commands/*.md`

Side-effecting flows are `disable-model-invocation: true` so the model can't fire them on its own. Plugin namespacing makes them `/mollify:mollify-audit` etc.

```markdown
---
description: Run a full mollify audit and report findings
argument-hint: "[--unused-exports|--dupes|--arch]"
disable-model-invocation: true
allowed-tools: Bash(mollify *)
---

## Mollify report
!`mollify audit $ARGUMENTS --format json --quiet 2>/dev/null`

## Task
Summarize the findings above: lead with `confidence: certain`, give file:line for each, and group by rule. For any action with `auto_fixable:true` and `confidence:certain`, offer to run /mollify:mollify-fix. Do not edit files in this command.
```

`/mollify:mollify-fix` mirrors this but runs `mollify fix --dry-run` then `--yes`. `/mollify:mollify-gate` runs `mollify audit --gate new-only`.

#### CLAUDE.md (consumer project)

A fact, not a procedure (procedures belong in the skill, which loads lazily):

```markdown
## Code quality
This repo uses Mollify for dead-code/duplication/architecture audits. Run `/mollify:mollify-audit` before opening a PR. A Stop hook gates on newly-introduced findings (`mollify audit --gate new-only`).
```

#### Version pinning

Pin `plugin.json` `version` to the CLI's JSON-contract minor. CI gate in the skills repo: `claude plugin validate ./mollify --strict` + validate `references/json-contract.md` against the published JSON Schema.

---

### 3.2 OpenAI Codex (CLI + IDE + Desktop/Cloud)

Codex is a four-surface system (CLI, VS Code extension, Desktop, Cloud) that **all read the same config layer**: `~/.codex/config.toml`, the `AGENTS.md` chain, registered MCP servers, and Skills. One set of artifacts lights Mollify up everywhere.

> Known bug: the VS Code extension sometimes fails to detect MCP servers from `config.toml` (issues #6465/#7820), so the AGENTS.md CLI-invocation path is the load-bearing fallback for the IDE.

#### AGENTS.md (insert into `<git-root>/AGENTS.md`)

Codex builds its instruction chain on every run, walking home → cwd, **closest-to-cwd wins**, 32 KiB cap (`project_doc_max_bytes`, default 32768). At each level it reads `AGENTS.override.md` else `AGENTS.md` (else configurable `project_doc_fallback_filenames`), at most one file per dir. Keep the Mollify block under ~1.5 KiB; `mollify init` inserts/refreshes a delimited block idempotently.

```markdown
<!-- BEGIN MOLLIFY v1 -->
## Codebase truth: Mollify

This repo has `mollify`, a deterministic Rust codebase-intelligence engine.
Prefer it over `grep`/manual scanning for dead code, duplication, circular
imports, complexity hotspots, dependency hygiene, and architecture boundaries.
Findings are deterministic evidence — never invent or guess findings; cite Mollify.

When to run (always with `--json` so you consume structured output, not prose):
- "is X used / can I delete X / find dead code"   -> `mollify dead-code --json`
- "what's wrong with this repo / health check"     -> `mollify audit --json`
- "unused/missing/transitive deps"                 -> `mollify deps --json`
- "duplication / copy-paste"                        -> `mollify dupes --json`
- "who calls / what does X reach"                   -> `mollify trace <symbol> --json`

Reading the JSON envelope:
- Top-level `kind` discriminates the result type; iterate `findings[]`.
- Each finding has `confidence` (certain|likely|uncertain), a stable
  `fingerprint`, a human `reason`, and `location` (file/line).
- Only act on `confidence: "certain"` without confirming with the user.
- If a finding has `auto_fixable: true`, you may apply `mollify fix --dry-run`
  first, show the diff, then `mollify fix` after the user approves.
- Never delete code on `likely`/`uncertain` without explaining the trace and asking.

Exit codes: 0 = clean / warn-only; non-zero = `error`-severity findings (CI gate).
<!-- END MOLLIFY v1 -->
```

#### MCP registration

CLI (recommended; writes `config.toml`):

```
codex mcp add mollify -- mollify mcp
codex mcp list
```

Hand-edit `~/.codex/config.toml` (user) or `.codex/config.toml` (project, trusted only):

```toml
[mcp_servers.mollify]
command = "mollify"
args = ["mcp"]
startup_timeout_sec = 20      # default 10
tool_timeout_sec = 120        # default 60
enabled = true

[mcp_servers.mollify.env]
MOLLIFY_CACHE_DIR = ".mollify-cache"
```

pip/uvx install variant:

```toml
[mcp_servers.mollify]
command = "uvx"
args = ["--from", "mollify", "mollify", "mcp"]
```

Streamable-HTTP variant uses `url` + `bearer_token_env_var` + `http_headers`. The server must log to stderr only.

> The exact spelling/enum of `enabled_tools`, `disabled_tools`, and `default_tools_approval_mode` is **not** verified against primary docs here — confirm against `developers.openai.com/codex/config-reference` before relying on them.

#### Skill — `.agents/skills/mollify/SKILL.md`

> **Location correction:** the canonical 2026 Codex skill dirs are **`.agents/skills/`** (project, committed; scanned from cwd up to repo root) and **`~/.agents/skills/`** (user). The older `.codex/skills/` / `~/.codex/skills/` paths still work and are what the bundled skill-installer defaults to, so ship there as a compatibility fallback. Frontmatter `name` **must equal the parent folder name**.

```markdown
---
name: mollify
description: >
  Run Mollify (deterministic Python codebase intelligence) to find dead code,
  duplication, circular imports, complexity hotspots, dependency hygiene issues,
  and architecture-boundary violations. Use whenever the user asks whether code
  is used, what to delete, what's duplicated, or for a repo health/quality report.
---

# Using Mollify

Mollify is a Rust-native, sub-second, deterministic engine. Findings are evidence,
not opinions — every finding carries a confidence level, a stable fingerprint, and
a reason. Do not invent findings; always cite Mollify output.

## Workflow
1. Pick the narrowest command for the question and ALWAYS pass `--json`:
   - dead code / "is X used"      -> `mollify dead-code --json`
   - full health report           -> `mollify audit --json`
   - dependency hygiene           -> `mollify deps --json`
   - duplication families         -> `mollify dupes --json`
   - call/reachability of symbol  -> `mollify trace <symbol> --json`
2. Parse the envelope: switch on top-level `kind`; iterate `findings[]`.
3. Act on `confidence: "certain"` only; surface `likely`/`uncertain` with their
   trace and ask the user before changing code.
4. For `auto_fixable: true` findings: `mollify fix --dry-run`, show diff, then
   `mollify fix` after approval.

See references/envelope.md for the full JSON schema.
```

Disable without deletion via `[[skills.config]]` (`path`, `enabled = false`) in `config.toml`.

> **Custom prompts (`~/.codex/prompts/*.md`, `/name`) are DEPRECATED** in favor of Skills (regression #15941; macOS app stopped surfacing them, #14459). Provide only as legacy convenience.

#### Hooks / notify (automation)

> **Hooks are now fully documented** (`developers.openai.com/codex/hooks`). Events: SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, PermissionRequest, PreCompact, PostCompact, SubagentStart, SubagentStop, Stop. Payload is JSON on stdin (`session_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`; turn events add `turn_id`).

```toml
# config.toml
[[hooks.Stop]]
command = ["mollify", "check", "--json"]   # template — verify field shape per your Codex version
```

Enterprise enforcement: `[features].hooks = false`, `allow_managed_hooks_only = true`, `[hooks] managed_dir = ...` in `requirements.toml` — so the guardrail must degrade gracefully to the AGENTS.md path.

Simpler turn-complete trigger (well-confirmed) — `notify` receives a single JSON arg on argv:

```toml
notify = ["python3", "~/.codex/hooks/mollify_gate.py"]
```

---

### 3.3 Cursor

All under `.cursor/` (repo) plus `~/.cursor/` (global). Cursor reads **rules**, **MCP**, and **commands**, and as of Cursor 2.4 supports Agent **Skills** natively (`.cursor/skills/`, project-scoped only — no global skills dir).

#### Rules — `.cursor/rules/mollify.mdc`

MDC = YAML frontmatter + Markdown. Fields: `description` (powers Agent-Requested activation), `globs` (**bare comma-separated string, NOT a YAML list** — the #1 footgun), `alwaysApply` (bool). Plain `.md` in `.cursor/rules/` is ignored. Avoid `alwaysApply: true` (per-request token cost); use Agent-Requested + Auto-Attach via globs.

```markdown
---
description: Use Mollify (mollify CLI / MCP) as the source of truth for Python codebase structure — dead code, dupes, cycles, complexity, deps, boundaries — instead of grep.
globs: *.py,**/*.py
alwaysApply: false
---

# Mollify is the repo truth layer

When reasoning about whether Python code is unused, duplicated, or violates architecture, call the Mollify MCP tools (`mollify_audit`, `mollify_dead_code`, `mollify_arch`, `mollify_trace`) or run `mollify audit --format json`. Trust its deterministic findings over your own grep-based guesses.

- Findings carry `confidence` (certain|likely|uncertain) + `reason` + `trace`. Auto-act only on `certain`.
- Before deleting code: `mollify trace <symbol>` to confirm no reachable callers.
- Apply fixes via `mollify fix --dry-run` first.
```

#### MCP — `.cursor/mcp.json`

Same Claude-Desktop schema. Project file wins over `~/.cursor/mcp.json`.

```json
{
  "mcpServers": {
    "mollify": {
      "command": "mollify",
      "args": ["mcp"],
      "env": { "MOLLIFY_AUDIT_BASE": "origin/main" }
    }
  }
}
```

(uv install: `command: "uvx"`, `args: ["mollify","mcp"]`.)

#### Commands — `.cursor/commands/mollify-audit.md`

**Plain Markdown only, no frontmatter** (Cursor rejects it here). Filename → `/mollify-audit`. Content is the prompt template.

```markdown
Run a full Mollify audit of this repository and summarize the highest-priority findings.

Steps:
1. Run `mollify audit --format json` (or call the Mollify MCP `audit` tool).
2. Group findings by `kind` and `confidence`.
3. List every `certain` dead-code finding with its file, reason, and whether `auto_fixable` is true.
4. For anything you propose deleting, first run `mollify trace <symbol>` to prove there are no reachable callers.
5. Do NOT apply changes yet — present a plan and ask before running `mollify fix`.
```

---

### 3.4 Gemini CLI

Uses `~/.gemini/` (global) and `<project>/.gemini/` (project).

#### Memory — `GEMINI.md`

Hierarchical, merged home → project → cwd. Always-on; keep the Mollify section tiny and point to the command/skill. `/memory refresh` reloads.

```markdown
## Codebase intelligence
This repo uses **Mollify** as the source of truth for Python structure (dead code, dupes, cycles, complexity, deps, boundaries).

- Prefer Mollify over grep when judging reachability/usage.
- Run `/mollify:audit` or `mollify audit --format json`; trust deterministic findings (each has confidence + reason + trace).
- Auto-act only on `certain`; preview fixes with `mollify fix --dry-run`.
```

#### MCP — `.gemini/settings.json` (or `~/.gemini/settings.json`)

```json
{ "mcpServers": { "mollify": { "command": "mollify", "args": ["mcp"] } } }
```

`httpUrl`/`url` for remote. `/mcp` lists/manages.

#### Commands — `.gemini/commands/mollify/audit.toml` → `/mollify:audit`

TOML with `description` + `prompt`. Subdir = namespace. `{{args}}` substitution and `!{...}` shell injection (requires shell-tool permission). `/commands reload` picks up changes.

```toml
description = "Run a Mollify audit and summarize prioritized findings"
prompt = """
You are reviewing this repository with Mollify, a deterministic codebase-intelligence engine.

Here is the current audit output:
!{mollify audit --format json {{args}}}

Using ONLY this output (do not grep or guess):
1. Summarize findings grouped by kind and confidence.
2. Call out every certain finding and whether it is auto_fixable.
3. For proposed deletions, recommend running `mollify trace <symbol>` to confirm no reachable callers.
4. Propose a fix plan; do not run `mollify fix` without confirmation.
"""
```

The `!{}` injection means the command itself runs Mollify and feeds JSON to the model — works even without MCP.

#### Skills — `.gemini/skills/` and `~/.gemini/skills/` (also `.agents/skills/`)

Native SKILL.md discovery — Gemini CLI's first-class on-demand expertise mechanism; ship the canonical skill here.

---

### 3.5 Others (matrix)

| Agent | Rule / memory | MCP registration | Command | Native SKILL.md |
|---|---|---|---|---|
| **GitHub Copilot** | `.github/copilot-instructions.md` / `AGENTS.md` / `*.instructions.md` (`applyTo`) | `.vscode/mcp.json` (key: **`servers`**) | `.github/prompts/*.prompt.md` | **yes** — `.github/skills/` |
| **Cline** | `.clinerules` or `.clinerules/` dir | `cline_mcp_settings.json` (global, VS Code globalStorage) | — (use rules) | **yes** — `.cline/skills/`, `~/.cline/skills/` |
| **Aider** | `CONVENTIONS.md` (via `.aider.conf.yml` `read:`) | `.aider.conf.yml` `mcp-servers` | — | **no** (rules + MCP only) |
| **Continue.dev** | `.continue/rules/*.md` or inline `config.yaml` | `config.yaml mcpServers:` or `.continue/mcpServers/*.yaml` | `.continue/prompts/` | **no** (rules + MCP only) |
| **Codex CLI** | `AGENTS.md` | `~/.codex/config.toml` `[mcp_servers.*]` | (custom prompts deprecated) | yes — `.agents/skills/` |

> **Corrections from verification:** **Roo Code shut down (~May 2026)** — do not ship a Roo target; migrate Cline users use `.cline/skills/`. **Aider and Continue do NOT natively support the Agent Skills standard** in 2026 — feed `SKILL.md` only as a plain context file; they are rules + MCP only. Copilot's skills dir is `.github/skills` (also reads `.claude/skills`, `.agents/skills`). Continue MCP standalone blocks are **YAML** under `.continue/mcpServers/` (e.g. `mollify.yaml`), not JSON — verify the extension.

Per-platform MCP examples:

```yaml
# Aider .aider.conf.yml
read: CONVENTIONS.md
mcp-servers:
  - name: mollify
    command: mollify
    args: ["mcp"]
```

```json
// Copilot .vscode/mcp.json  (note: key is "servers", not "mcpServers")
{ "servers": { "mollify": { "command": "mollify", "args": ["mcp"] } } }
```

---

## 4. Devin Desktop / Cascade (FEATURED)

> **Now sourced from the authoritative docs.** This section was rewritten against `docs.devin.ai/llms-full.txt` (the full Devin documentation, supplied by the user after the domain was egress-blocked). Confidence is **HIGH** for everything below except the few items flagged inline. This supersedes the earlier WebSearch-reconstructed draft.

**Devin Desktop** is the rebranded Windsurf IDE (OTA update June 2, 2026 — "same IDE, same editor, same features, unified under the Devin brand"). Two local agent surfaces matter, and they **share the same on-disk config**:

- **Cascade** — the in-IDE agent the org uses *today*. Remains available **through July 2026**.
- **Devin Local** — the successor (Rust harness, ~30% more token-efficient, **subagents**, sandboxing). Runs the **same architecture as Devin CLI**, inherits your Windsurf settings, speaks **ACP** to the editor.

Because both read the same files, **everything Mollify ships now carries forward to Devin Local with no rework.** Devin Desktop reads `.devin/` as the **preferred, higher-precedence** workspace dir and falls back to `.windsurf/` for backward compatibility (authoritative — FAQ "workspace-level directories" table):

| Artifact | Preferred (read+write) | Legacy fallback (read) | Mollify ships |
|---|---|---|---|
| **Skills** | `.devin/skills/<name>/SKILL.md` | `.windsurf/skills/`, `.agents/skills/` (portable), `.claude/skills/` | `.devin/skills/mollify/SKILL.md` (+ `.agents/skills/mollify/` for portability) |
| **Rules** | `.devin/rules/*.md` | `.windsurf/rules/`, `.windsurfrules`, `AGENTS.md` | `.devin/rules/mollify.md` |
| **Workflows** | `.devin/workflows/*.md` | `.windsurf/workflows/*.md` (currently the documented IDE path) | `.windsurf/workflows/mollify-*.md` (+ mirror to `.devin/workflows/`) |
| **Hooks (Cascade IDE)** | `.windsurf/hooks.json` | system/user levels | `.windsurf/hooks.json` |
| **Hooks (Devin CLI/Local)** | `.devin/hooks.v1.json` (Claude-Code-compatible) | `.devin/config.json` `"hooks"`, `.claude/settings.json` | `.devin/hooks.v1.json` |
| **MCP (Cascade IDE)** | `~/.codeium/windsurf/mcp_config.json` | — (user/global file) | snippet + `/mollify-bootstrap` |
| **MCP (Devin CLI/Local)** | `.devin/config.json` / `.devin/config.local.json` | `~/.config/devin/config.json` | committed `.devin/config.json` block |
| **Memories** | `~/.codeium/windsurf/memories/` | — | not committed (local only) |

> **Why two of several rows split IDE vs CLI:** Cascade (the IDE agent) and Devin CLI/Local are different harnesses with slightly different config surfaces for **hooks** and **MCP**. The org is on Cascade → ship the IDE variants now; add the `.devin/` CLI variants so the Devin Local migration is zero-effort. Skills and rules are unified (`.devin/`) across both.

### 4.1 Skills — `.devin/skills/mollify/SKILL.md` (the priority, future-facing artifact)

Skills are the investment Devin's own docs tell you to make ("**invest here**"; "use a Skill instead" of rules/workflows when the agent should pick it up on its own). They use **progressive disclosure**: only `name` + `description` sit in context until the skill is invoked, so many skills stay cheap. A skill is a folder (`SKILL.md` + any bundled scripts/templates). **Invocation differs by surface — this matters:**

- **Cascade (IDE):** auto-invoked when your request matches the `description`, or explicitly via **`@mollify`** (`@mention`, *not* a slash command). Required frontmatter is just `name` + `description`.
- **Devin CLI / Devin Local:** invoked as **`/mollify`** or autonomously; supports **richer frontmatter** — `model`, `allowed-tools`, `permissions`, `triggers`, `argument-hint`, and crucially **`subagent: true`** / **`agent: <profile>`** to run the skill as an independent subagent. A Devin Local coordinator can spawn a dedicated Mollify "audit" subagent — exactly the parallelism Mollify wants.

One `SKILL.md` serves both (the IDE ignores the extra CLI fields). Ship to `.devin/skills/mollify/` (preferred) and copy to `.agents/skills/mollify/` for cross-tool portability.

```markdown
---
name: mollify
description: >
  Run Mollify — a Rust-native, deterministic Python code-intelligence CLI + MCP
  server — to find dead code, duplication, circular imports, complexity
  hotspots, dependency-hygiene issues, and architecture-boundary violations.
  Use whenever the user asks whether code is used, what to delete, what's
  duplicated, wants a repo health/quality report, or before opening a PR.
# --- fields below are honored by Devin CLI / Devin Local; ignored by Cascade IDE ---
allowed-tools: [read, grep, glob, exec]
subagent: true          # Devin Local: run audits as an isolated subagent
---

# Mollify code intelligence

Mollify is a deterministic candidate-producer: it emits evidence (each finding has
a stable fingerprint, a confidence level, and a reason), not decisions. You are the
verifier. Never invent findings or hand-delete code on a guess.

## Prefer the MCP server
If the `mollify` MCP server is connected, call its tools (`audit`, `find_dead_code`,
`find_duplication`, `trace`, `inspect_target`) directly. Otherwise use the CLI.

## Running an audit (CLI)
1. `mollify audit --format json` (changed files only: `mollify audit --gate new-only --format json`).
2. Parse the envelope: switch on top-level `kind`; iterate `findings[]`.
   Each finding: `{ fingerprint, category, confidence, reason, location{path,line}, actions[] }`.
3. Lead with `confidence: certain`; cite `path:line` and the fingerprint.

## Acting on findings
- `auto_fixable: true` AND `confidence: certain` → safe to apply via `mollify fix`.
- Everything else → explain the trace, let the user decide; do not edit.
- Always `mollify fix --dry-run` first, show the diff, then `mollify fix --yes` after approval.
- Re-run the audit afterward to confirm the fingerprint is gone.

## Resources
- references/json-contract.md — the full JSON envelope schema.
- references/cli-reference.md — all commands & flags.
```

### 4.2 Rules — `.devin/rules/mollify.md` (preferred; `.windsurf/rules/` fallback)

One `.md` per rule, **12,000 char** limit each, with a `trigger:` frontmatter activation mode (authoritative — Memories & Rules doc): `always_on` (every message), `model_decision` (only the `description` is in-context; full file pulled when relevant), `glob` (applied when a file matching `globs:` is read/edited), `manual` (`@mollify-rule`). Global rules live in `~/.codeium/windsurf/memories/global_rules.md` (6,000 char, always-on). Root `AGENTS.md` is processed by the same engine (root = always-on, subdir = auto-glob). Keep rules tiny and have them **point at the skill** (vendor-recommended pattern).

```markdown
---
trigger: glob
globs: **/*.py
---

# Mollify is the codebase truth layer
- Before finalizing changes to Python files (and before any PR), run the `mollify`
  skill / `mollify audit --format json`. Treat findings as ground truth — never
  hand-delete code without a Mollify high-confidence fingerprint.
- Prefer the `mollify` MCP tools when connected.
- Auto-act only on `confidence: certain`; surface `likely`/`uncertain` and ask.
```

### 4.3 Workflows — `.windsurf/workflows/mollify-*.md` → `/slash` commands

Markdown recipes invoked **manually** as `/<name>` (Cascade never auto-runs a workflow — that's what skills are for). **12,000 char** limit. Discovered in `.windsurf/workflows/` up to the git root; global in `~/.codeium/windsurf/global_workflows/`; **`.devin/workflows/` is the new preferred location per the FAQ** (mirror there too). A workflow can call other workflows. The body is a title + description + numbered steps — **no special frontmatter is required** (the earlier `auto_execute_steps` field is NOT in the authoritative docs; do not rely on it).

`.windsurf/workflows/mollify-audit.md` (read-only triage):

```markdown
# Mollify audit
Read-only Mollify triage of the repo / changed files.

1. If files are staged/changed, scope to those; else audit the whole repo.
2. Prefer the `mollify` MCP `audit` tool; if MCP is unavailable, run
   `mollify audit --format json` in the terminal.
3. Group findings by category (dead-code, duplication, circular-deps, complexity,
   package-hygiene) with counts + confidence.
4. For each high-confidence finding in changed files, show file:line, reason, fingerprint.
5. Do NOT modify files. End with a verdict: PR-ready / needs cleanup (→ /mollify-cleanup).
```

`.windsurf/workflows/mollify-cleanup.md` (guided remediation; edits gated on approval) and `.windsurf/workflows/mollify-bootstrap.md` (one-time per dev: writes the `~/.codeium/windsurf/mcp_config.json` block) follow the same shape; cleanup must re-audit after each removal and never auto-run destructive commands.

### 4.4 Hooks — deterministic enforcement (two systems; ship both)

Hooks are the only deterministic surface. **Cascade (IDE) and Devin CLI/Local use *different* hook formats** — ship both so the gate survives the Devin Local migration:

**(a) Cascade IDE — `.windsurf/hooks.json`** (authoritative — Cascade Hooks doc). Wrapper `{"hooks": {...}}`, **12 events** with lowercase names (`pre_read_code`, `post_read_code`, `pre_write_code`, `post_write_code`, `pre_run_command`, `post_run_command`, `pre_mcp_tool_use`, `post_mcp_tool_use`, `pre_user_prompt`, `post_cascade_response`, `post_cascade_response_with_transcript`, `post_setup_worktree`). Each handler: `command` (bash -c), optional `powershell`, `show_output`, `working_directory`. **No `matcher`** — the event *is* the filter; you inspect `tool_info` (JSON on stdin) inside the script. **Pre-hooks block via exit code 2** (post-hooks can't block). Merged system → user → workspace.

```json
{
  "hooks": {
    "post_write_code": [
      { "command": "scripts/mollify-postwrite.sh", "show_output": false }
    ],
    "pre_run_command": [
      { "command": "scripts/mollify-guard.sh", "show_output": true }
    ]
  }
}
```

`mollify-postwrite.sh` reads `tool_info.file_path` from stdin, and if it's a `*.py`, records newly-introduced findings (`mollify audit --gate new-only --format json`). `mollify-guard.sh` can `exit 2` to block a disallowed command. Keep auto-fix OFF in hooks — hooks gather/gate; the skill or a workflow applies fixes.

**(b) Devin CLI / Devin Local — `.devin/hooks.v1.json`** (authoritative — Devin CLI Hooks doc). **Claude-Code-compatible**: PascalCase events (`PreToolUse`/`PostToolUse`/`Stop`/`SessionStart`/…), a regex `matcher` on `tool_name`, JSON stdin, and `{"decision":"block","reason":"…"}` on stdout (exit 2 also blocks). **The same file works in Claude Code** (`.claude/settings.json` `"hooks"`), so Mollify's Claude Code Stop/PostToolUse gate (§3.1) is reused verbatim here — one gate, three harnesses.

### 4.5 MCP — `~/.codeium/windsurf/mcp_config.json` (Cascade) + `.devin/config.json` (CLI/Local)

**Cascade IDE** reads `~/.codeium/windsurf/mcp_config.json` (path unchanged after the rebrand). `mcpServers` map; transports **stdio / Streamable HTTP / SSE** (+ OAuth). Interpolation in `command`/`args`/`env`/`serverUrl`/`url`/`headers` via **`${env:VAR}`** and **`${file:/path}`**. **100-tool cap** across all servers; per-server tools are toggled in the MCP settings UI (`disabledTools` array), and admins can whitelist/registry-restrict servers. There is also a one-click marketplace + `windsurf://windsurf-mcp-registry?serverName=mollify` deeplink we can publish.

```json
{
  "mcpServers": {
    "mollify": {
      "command": "mollify",
      "args": ["mcp", "--stdio"],
      "env": { "MOLLIFY_LICENSE": "${env:MOLLIFY_LICENSE}", "MOLLIFY_LOG": "warn" }
    }
  }
}
```

**Devin CLI / Devin Local** uses `.devin/config.json` (committed, shared) / `.devin/config.local.json` (gitignored secrets), or `devin mcp add mollify -- mollify mcp`. Same `mcpServers` schema; tools namespaced `mcp__mollify__<tool>` and governed by `permissions.allow/deny/ask`. Because it's committed, the project MCP registration ships in-repo — no per-dev bootstrap needed on the Devin Local path.

```json
// .devin/config.json
{
  "mcpServers": { "mollify": { "command": "mollify", "args": ["mcp"] } },
  "permissions": { "allow": ["mcp__mollify__audit", "mcp__mollify__find_dead_code"] }
}
```

### 4.6 Memories

`~/.codeium/windsurf/memories/` — Cascade auto-generates these per workspace, local, not committed, **no credit cost**. Use once as reinforcement ("Mollify audit JSON is ground truth; never hand-delete without a high-confidence fingerprint"), but the durable, shareable home for that is the rule/skill, not a memory.

### 4.7 Enterprise / org-wide rollout (no per-repo work)

Devin Desktop reads **system-level** rules, workflows, skills, and hooks that IT deploys once per machine (MDM/Ansible/Jamf/Intune) and users can't modify — ideal for pushing Mollify org-wide:

- Rules: `/Library/Application Support/Devin/rules/` · `/etc/devin/rules/` · `C:\ProgramData\Devin\rules\` (Windsurf paths = legacy fallback).
- Workflows: `…/Windsurf/workflows/` (and the Devin equivalents) — System precedence > Workspace > Global > Built-in.
- Skills: `…/Windsurf/skills/` etc.
- Hooks: system `hooks.json` **and** the **cloud dashboard** (Team Settings → Cascade Hooks) — distributed to all members, can't be disabled by users.

Plus `mollify init --agent devin` to scaffold the per-repo `.devin/` artifacts in one command.

### 4.8 Cascade EOL → Devin Local + ACP (decisive, authoritative)

- **Cascade is available through July 2026**, then superseded by **Devin Local** (the FAQ wording is "remains available through July"; treat end-of-July, not a hard July-1 date — *correction to the earlier draft*).
- **Everything carries forward.** Devin Desktop "continues to read all your existing Windsurf rules and adds the `.devin/` equivalents; nothing you have today needs to change," and **retains MCP**. Devin Local inherits Windsurf settings. So the Mollify rules/skills/workflows/hooks/MCP shipped now keep working.
- **ACP ⟂ MCP.** ACP (Agent Client Protocol, JSON-RPC over stdio — Devin CLI/Local ↔ editors like Zed/JetBrains) is *agent↔editor*; MCP is *agent↔tools*. **Mollify is an MCP server and does not implement ACP.** Devin Local's **subagents** make Mollify *more* useful — a coordinator can spawn an isolated Mollify audit subagent (mirror it as a `subagent: true` skill, §4.1).
- **Net hedge:** ship `.devin/` primary + `.windsurf/` fallback; keep a CLI fallback in every workflow/skill so nothing breaks if MCP registration drifts; provide both `.windsurf/hooks.json` (Cascade) and `.devin/hooks.v1.json` (Devin Local/CLI). Zero rework expected at the cutover.

## 5. Cross-platform primitive matrix

| Agent | Rules / memory | Commands / workflows | Hooks | MCP | Memory |
|---|---|---|---|---|---|
| **Claude Code** | `CLAUDE.md` + skill | `commands/*.md` (`/plugin:cmd`) | **yes** — PostToolUse + Stop gate | `.mcp.json` / `plugin.json` | `CLAUDE.md` |
| **Codex** | `AGENTS.md` chain (32 KiB) | custom prompts (deprecated) | **yes** — `[hooks]` + `notify` | `config.toml` `[mcp_servers.*]` | `AGENTS.md` |
| **Cursor** | `.cursor/rules/*.mdc` | `.cursor/commands/*.md` | no | `.cursor/mcp.json` | (rules) |
| **Gemini CLI** | `GEMINI.md` | `.gemini/commands/*.toml` (`!{}`) | no | `.gemini/settings.json` | `GEMINI.md` (`/memory`) |
| **Devin Desktop / Cascade** | `.devin/rules/*.md` (`trigger`) **+ `.devin/skills/SKILL.md`** (skills via `@mention`/auto, not slash) | `.windsurf/workflows/*.md` (`/slash`, manual-only) | **yes** — Cascade `.windsurf/hooks.json` (12 lowercase events, no matcher) · Devin CLI/Local `.devin/hooks.v1.json` (Claude-compatible) | Cascade `~/.codeium/windsurf/mcp_config.json` · CLI/Local `.devin/config.json` | `~/.codeium/windsurf/memories/` |
| **GitHub Copilot** | `*.instructions.md` / `AGENTS.md` | `.github/prompts/*.prompt.md` | no | `.vscode/mcp.json` (`servers`) | instructions files |
| **Cline** | `.clinerules` | — | no | `cline_mcp_settings.json` | (rules) |
| **Aider** | `CONVENTIONS.md` | — | no | `.aider.conf.yml` `mcp-servers` | `CONVENTIONS.md` |
| **Continue** | `.continue/rules/*.md` | `.continue/prompts/` | no | `.continue/mcpServers/*.yaml` | (rules) |

### What we build once vs per-agent

**Build once (the substrate):**
- `mollify mcp` — the single stdio MCP server.
- The kind-discriminated JSON contract + published JSON Schema.
- The canonical `SKILL.md` (Agent Skills open standard; natively discovered by Claude Code, Codex, Gemini CLI, Copilot, Cline, Cursor 2.4+).
- The `mollify-skills` repo (Claude Code marketplace + plugin + skill bundle).

**Build per-agent (thin shims, generated by `mollify init --agents`):**
- A rule/memory file in each agent's format (`.mdc`, `GEMINI.md`, `AGENTS.md`, `.devin/rules/*.md`, …).
- An MCP registration block in each agent's config (`.mcp.json`, TOML `[mcp_servers.*]`, `mcp_config.json`, `servers`, YAML `mcp-servers`).
- A command/workflow file where the agent supports one (`.cursor/commands`, `.gemini/commands/*.toml`, `.devin/workflows/*`).
- Hook artifacts only where deterministic enforcement exists (Claude Code, Codex, and Devin/Cascade — note Devin CLI/Local reuses the Claude-Code-format hooks file verbatim).

**Always version-pin** every generated artifact to the CLI's JSON-schema version, with a CI drift gate, since clients depend on the envelope `kind`, not on Mollify internals.

---

## 6. Confidence / source caveats appendix

### Mollify-specific items are DESIGN, not shipped

Mollify is a design-stage product (per its own `PLAN.md`/`RESEARCH.md`, which target parity with fallow's contract). The following are design targets that must be matched to the real CLI once built:

- CLI subcommands (`mollify mcp`, `mollify audit --gate new-only`, `mollify fix --dry-run/--yes`, `dead-code`/`deps`/`dupes`/`trace`).
- The JSON envelope field names (`kind`, `quality_score`, `findings`, `confidence`, `attribution`, `fingerprint`, `reason`, `actions[]`, `auto_fixable`, `suppression_comment`).
- MCP tool names beyond `inspect_target`/`security_candidates`.
- **The `attribution: introduced` field the Stop/PR gate keys on is a v2 deliverable** in the plan — confirm it exists before the gate can attribute introduced-vs-inherited.
- Confidence vocabulary is inconsistent across the source material (`certain|likely|uncertain` vs `certain|likely|possible` vs `high|medium|low`) — normalize to the published schema.
- fallow is TS/JS; Mollify is Python — flag names like `--unused-exports` are illustrative and must map to Mollify's real Python-oriented flags.

### Per-platform source confidence

**Claude Code — HIGH (verified against primary docs).** marketplace.json shape (owner as object, plugins[] fields), plugin.json (name-only required, mcpServers/hooks as path|object, version-pins-else-SHA, `--strict` validate, `defaultEnabled`), MCP stdio schema + `${CLAUDE_PLUGIN_ROOT}` + `mcp__server__tool` naming, SKILL.md frontmatter fields + 1,536-char cap, hook events + Stop blocking via `{decision:"block"}` at exit 0 + PostToolUse `additionalContext` — all confirmed against `code.claude.com/docs`.
*Caveats:* the claim that this "mirrors fallow exactly" is **false** — fallow ships only `SKILL.md` + `references/`, with no plugin/marketplace/hooks/.mcp.json; those are net-new. Verify the exact `additionalContext` key casing against the live hooks reference. Colocate `references/` inside the skill dir (not plugin root).

**Codex — MEDIUM.** `developers.openai.com` and `openai/codex` GitHub were egress-blocked; claims are from search summaries of primary docs + the official GitHub install guides. Confirmed with high consistency: AGENTS.md precedence + 32 KiB cap, `[mcp_servers.*]` TOML schema + timeouts + `codex mcp add`, custom-prompts deprecation, Skills format, shared config across surfaces, hooks now documented (events incl. SubagentStart; enterprise `managed_dir`/`allow_managed_hooks_only`).
*Not confirmable here:* exact enum of `enabled_tools`/`disabled_tools`/`default_tools_approval_mode`. **Skill dir corrected to `.agents/skills/` (canonical) with `~/.codex/skills/` as legacy fallback; `name` must equal folder name.**

**Cursor / Gemini CLI / others — MEDIUM.** Vendor domains (cursor.com, geminicli.com, agentskills.io) returned 403 to the fetch tool; corroborated via search excerpts + reputable 2025–2026 guides + the Agent Skills open standard. SKILL.md limits (name ≤64, description ≤1024 portable / 1,536 in Claude Code), Cursor MDC fields (globs-as-comma-string footgun), Gemini TOML commands (`{{args}}`, `!{}`), MCP JSON/TOML schemas — confirmed.
*Corrections:* Roo Code shut down (~May 2026) — dropped. Cline skills dir is `.cline/skills/`. Aider + Continue do **not** natively support Agent Skills — rules + MCP only. Copilot skills dir is `.github/skills`. Continue MCP standalone blocks are YAML. Cursor `.cursor/skills/` is native (2.4+, project-scoped only). `allowed-tools` is EXPERIMENTAL in the portable spec; support varies per agent.

**Devin Desktop / Cascade — HIGH (now sourced from authoritative docs).** §4 was rewritten against `docs.devin.ai/llms-full.txt` (the full Devin documentation, user-supplied after the domain was egress-blocked). Confirmed verbatim from primary docs: `.devin/` preferred over `.windsurf/` (FAQ workspace-directory table); rule `trigger` modes + 12,000/6,000-char limits + global `~/.codeium/windsurf/memories/global_rules.md`; skills (`.devin/skills/`, `.agents/skills/`, progressive disclosure, `@mention`/auto in IDE vs `/slash`+`subagent:true` in Devin CLI); workflows `.windsurf/workflows/*.md` `/slash` manual-only 12k; **two hook systems** — Cascade `.windsurf/hooks.json` (12 lowercase events, `command`/`powershell`/`show_output`/`working_directory`, **no matcher**, exit-2 blocks) and Devin CLI `.devin/hooks.v1.json` (Claude-Code-compatible); MCP `~/.codeium/windsurf/mcp_config.json` (Cascade) vs `.devin/config.json`/`devin mcp add` (CLI) with `${env:}`/`${file:}` interpolation + 100-tool cap; enterprise system-level `rules/workflows/skills/hooks` paths; ACP ⟂ MCP.
*Corrections folded in from the authoritative pass (these retire earlier WebSearch guesses):* the **`auto_execute_steps`** workflow frontmatter field is **not in the docs — removed** (workflows are plain title+steps); MCP **`alwaysAllow` is not a Cascade field** — tools are toggled in the UI / `disabledTools` (removed); skills are **`@mention`, not slash**, in the IDE; hooks have **no `matcher`** in Cascade (the event is the filter); Cascade EOL is **"available through July 2026"**, not a hard `2026-07-01`. *Still worth re-confirming against your installed build:* whether `.devin/workflows/` and `.devin/hooks.json` are honored yet by your Cascade channel (docs show `.windsurf/` as the live IDE path for both, with `.devin/` as the stated direction).
