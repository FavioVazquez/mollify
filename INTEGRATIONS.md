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

- `mollify_audit` — full unified report
- `mollify_inspect_target` — inspect a specific symbol/file
- `mollify_security_candidates` — security-relevant candidates
- `mollify_fix` — apply fixes (dry-run by default)
- plus the dead-code / deps / dupes / trace surface mirroring the CLI

In some agents the tools are exposed under shorter names (`audit`, `dead_code`, `trace_*`, `fix_preview`/`fix_apply`, `project_info`). Whatever the naming, **every tool returns the same typed JSON envelope as the CLI**, so the contract is identical across surfaces.

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

When reasoning about whether Python code is unused, duplicated, or violates architecture, call the Mollify MCP tools (`inspect_target`, `dead_code`, `audit`, `trace_export`) or run `mollify audit --format json`. Trust its deterministic findings over your own grep-based guesses.

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

Cascade is the in-IDE coding agent in Windsurf, **rebranded Devin Desktop by Cognition on 2026-06-02** (OTA). Since the org runs Cascade, this is the primary target. It is customized through four repo-shippable mechanisms plus one user-level config.

The `.devin/` directory is the modern convention that bundles **skills + rules + hooks**; `.windsurf/` carries **workflows** (and is the legacy fallback for rules/skills). Per the org's direction, ship `.devin/` as primary and treat **skills as the crucial future-facing artifact**.

| Cascade mechanism | Mollify use | Ships in repo? |
|---|---|---|
| `.devin/skills/mollify/SKILL.md` (preferred) + `.agents/skills/` (portable) | Model-invoked teaching artifact: when/how to run mollify, read JSON, gate fixes | Yes (committed) — **priority** |
| `.devin/rules/*.md` (preferred) + `.windsurf/rules/` (fallback) | Run `mollify audit` before PRs; how to read findings; point at the skill | Yes (committed) |
| `.devin/hooks.json` / `.windsurf/hooks.json` | Deterministic pre/post enforcement (audit on write, block on disallowed cmd) | Yes (committed) — see caveat |
| `.windsurf/workflows/*.md` (slash commands) | `/mollify-audit`, `/mollify-cleanup`, `/mollify-bootstrap` | Yes (committed) |
| `mcp_config.json` → `mcpServers.mollify` | Register the MCP server (~25 tools) | No — user/global file; ship a snippet + bootstrap workflow |
| Memories | Auto-remember Mollify conventions per workspace | Not committed (local only) |
| Planning mode | Glob rule injects "audit first" into the plan preview | Indirect |

> **Workflows path nuance:** my dedicated Devin deep-dive found workflows are still confirmed at `.windsurf/workflows/` (no confirmed move to `.devin/workflows/`); the auto-generated section above optimistically lists `.devin/workflows/` as preferred. **Ship `.windsurf/workflows/` as the confirmed path**, and only mirror to `.devin/workflows/` once verified. Skills/rules/hooks are the confirmed `.devin/` members.

### 4.0 Skills — `.devin/skills/mollify/SKILL.md` (the priority, future-facing artifact)

Cascade/Devin Desktop has first-class **Skills** (the open `SKILL.md` standard): a folder per skill whose frontmatter is *always* in context (so the agent knows the skill exists) while the body loads *only when invoked* — cheaper and more durable than always-on rules. Vendor guidance explicitly recommends **skills over rules** where possible, and rules that merely *point at* skills. Skills are read from three locations: **`.devin/skills/`** (primary), **`.agents/skills/`** (portable cross-tool standard — also read by Claude Code, Codex, Gemini, Copilot, Cline), and `.windsurf/skills/` (fallback). Ship the same canonical `SKILL.md` to `.devin/skills/mollify/` and symlink/copy to `.agents/skills/mollify/` for portability. Folder name must equal the frontmatter `name`. Bundle helper scripts/resources in the folder and reference them by relative path.

```markdown
---
name: mollify
description: >
  Run Mollify — a Rust-native, deterministic Python code-intelligence CLI + MCP
  server — to find dead code, duplication, circular imports, complexity
  hotspots, dependency-hygiene issues, and architecture-boundary violations.
  Use whenever the user asks whether code is used, what to delete, what's
  duplicated, or wants a repo health/quality report, or before opening a PR.
license: MIT
---

# Mollify code intelligence

Mollify is a deterministic candidate-producer: it emits evidence (each finding
has a stable fingerprint, a confidence level, and a reason), not decisions. You
are the verifier. Never invent findings or hand-delete code on a guess.

## Prefer the MCP server
If the `mollify` MCP server is connected, call its tools (`audit`,
`find_dead_code`, `find_duplication`, `trace`, `inspect_target`) directly.
Otherwise use the CLI below.

## Running an audit (CLI)
1. Run: `mollify audit --format json` (changed files: `mollify audit --gate new-only --format json`).
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

> **Precedence correction (important):** as of the Devin Desktop rebrand, **`.devin/` is the PREFERRED, higher-precedence dir** and `.windsurf/` is the backward-compat fallback — *not* the other way around. Ship `.devin/rules/mollify.md` and `.devin/workflows/*` as primary; keep `.windsurf/` copies as fallback. Cascade reads both, so **do not put divergent content in both** (`.devin` wins silently). Markdown rules/workflows are forward-compatible: everything shipped now survives EOL.

### 4.1 Rules — `.devin/rules/mollify.md` (+ `.windsurf/rules/mollify.md` fallback)

Markdown + YAML frontmatter. `trigger` modes: `always_on` (every prompt — use for short universal facts), `manual` (`@`-mention), `model_decision` (Cascade reads `description` and decides), `glob` (applied to files matching `globs`). **Char limits:** workspace rule files 12,000 chars each; global rules 6,000 chars (over-limit content is silently dropped).

Precedence: `.devin/rules` > `.windsurf/rules` > legacy `.windsurfrules` > `AGENTS.md` (nearest file wins) > global/system. An explicit user prompt overrides all.

Glob-triggered rule (the requested "audit before PRs on Python files" enforcement):

```markdown
---
trigger: glob
globs: ["**/*.py"]
description: Run a Mollify audit before opening any PR that touches Python and read findings as ground truth.
---

# Mollify is the codebase truth layer

Mollify is a deterministic, Rust-native code-intelligence engine. Its findings are evidence-backed (each has a stable fingerprint, a confidence level, and a reason). NEVER invent findings or hand-delete code based on a guess.

## Before opening a PR
1. Run `mollify audit --format json` (or call the `mollify` MCP `audit` tool). Do this as the FIRST step in your plan whenever Python files changed.
2. Surface counts by category: dead-code, duplication, circular-deps, complexity hotspots, package-hygiene.
3. If `dead-code` or `circular-deps` findings appear in files this PR touches, fix them or call out why not in the PR description.

## Reading findings
- Each finding = { fingerprint, category, confidence (high/medium/low), reason, location }.
- Treat `confidence: high` dead-code as safe to remove ONLY after confirming reachability via Mollify; never delete `confidence: low` without human sign-off.
- Cite the fingerprint in commits so the finding is traceable.
```

Pair with a tiny `trigger: always_on` rule (<300 chars): *"Mollify findings are ground truth; never hand-delete code without a Mollify fingerprint."*

### 4.2 Workflows — `.devin/workflows/*.md` (+ `.windsurf/workflows/` fallback) → `/slash-commands`

Markdown recipes invoked as `/<filename>`. Frontmatter: `name`, `description`, and `auto_execute_steps`. Body = numbered natural-language steps. **Manual-only** (don't auto-fire). 12,000 char limit.

> **Highest-risk field:** `auto_execute_steps` (step-type allowlist like `read_file`/`run_command`) is corroborated only by secondary sources; historically Windsurf used `auto_execution_mode` (an integer tier). **Verify the field name/shape against `docs.devin.ai/desktop/cascade/workflows` before shipping** — if wrong, the frontmatter is silently ignored and steps still prompt. Also, `run_command` auto-execution is governed by the app-level Auto-Execution/Turbo setting and allow/deny lists, so a frontmatter hint may not fully suppress prompts.

**`/mollify-audit`** (read-only triage):

```markdown
---
name: mollify-audit
description: Read-only Mollify triage of the current repo / changed files.
auto_execute_steps: [read_file, run_command]
---

# /mollify-audit

1. Detect scope: if there are staged/changed files, audit those; otherwise audit the whole repo.
2. Prefer the `mollify` MCP server if registered: call the `audit` tool with format=json. If MCP is unavailable, run `mollify audit --format json` in a terminal.
3. Parse findings and produce a table grouped by category (dead-code, duplication, circular-deps, complexity, package-hygiene) with counts and confidence.
4. For each high-confidence finding in changed files, show file:line, the reason, and the fingerprint.
5. Do NOT modify any files. End with a short verdict: PR-ready / needs cleanup, and link to /mollify-cleanup if needed.
```

**`/mollify-cleanup`** (guided remediation — edits gated behind explicit approval):

```markdown
---
name: mollify-cleanup
description: Guided remediation of high-confidence Mollify findings.
auto_execute_steps: [read_file]
---

# /mollify-cleanup

1. Run /mollify-audit (or the `audit` MCP tool) to get current findings as JSON.
2. Filter to confidence: high findings in files relevant to the current change.
3. Present a remediation plan (one line per finding: fingerprint, action) and WAIT for user approval before editing.
4. For approved dead-code findings: remove the code, then re-run `mollify audit` (or the MCP tool) on the affected files to confirm the fingerprint is gone and no new findings were introduced.
5. For duplication/circular-deps: propose a refactor, apply only after approval, then re-audit.
6. Summarize: resolved fingerprints, remaining findings, and run the test suite before finishing.
```

> Do **not** add `run_command` to `auto_execute_steps` on the destructive path. Always re-audit (step 4) so removal is verified deterministically.

**`/mollify-bootstrap`** (one-time per developer — writes the home-dir MCP block, since that file is not committed):

```markdown
---
name: mollify-bootstrap
description: Register the Mollify MCP server in the user's Windsurf/Devin Desktop config.
auto_execute_steps: [read_file]
---

# /mollify-bootstrap

1. Locate `~/.codeium/windsurf/mcp_config.json` (Windows: %USERPROFILE%\.codeium\windsurf\mcp_config.json). Create it with `{ "mcpServers": {} }` if absent.
2. Merge the `mollify` server block (see project docs) into mcpServers WITHOUT clobbering existing servers. WAIT for user confirmation before writing.
3. Tell the user to reload MCP servers from the Cascade MCP panel and confirm the `mollify` tools appear (Cascade caps total tools at 100; Mollify uses ~25).
4. Verify by calling the `audit` MCP tool on a small path.
```

### 4.3 MCP — `~/.codeium/windsurf/mcp_config.json`

Path **unchanged after the rebrand** (`~/.codeium/` retained). Windows: `%USERPROFILE%\.codeium\windsurf\mcp_config.json`. Top-level key `mcpServers`. Transports: stdio, Streamable HTTP, SSE. Per-server fields include `disabled` (bool) and `alwaysAllow` (array of tool names to auto-approve). Env interpolation via `${env:VAR}`. **Hard cap of 100 tools** total across all servers.

```json
{
  "mcpServers": {
    "mollify": {
      "command": "mollify",
      "args": ["mcp", "--stdio"],
      "env": {
        "MOLLIFY_LICENSE": "${env:MOLLIFY_LICENSE}",
        "MOLLIFY_LOG": "warn"
      },
      "disabled": false,
      "alwaysAllow": ["audit", "find_dead_code", "find_duplication", "find_circular_deps"]
    }
  }
}
```

Auto-approve only read-only query tools via `alwaysAllow`; leave any mutating tool to manual approval. Never hardcode secrets — use `${env:...}`. Remote HTTP variant:

```json
{ "serverUrl": "https://mollify.internal/mcp", "headers": { "Authorization": "Bearer ${env:MOLLIFY_TOKEN}" } }
```

### 4.3b Hooks — `.windsurf/hooks.json` (deterministic enforcement)

Cascade supports **hooks** (the only deterministic surface — rules/skills can be ignored on later turns, a hook always fires). Confirmed file: **`.windsurf/hooks.json`** at repo root, merged across global + project levels; a `.devin/hooks.json` equivalent is *inferred by symmetry but not confirmed* — ship `.windsurf/hooks.json` and verify the `.devin/` path before relying on it. There are ~12 events (e.g. `pre_user_prompt`, `pre_read_code`, `pre_write_code`, `post_write_code`, `pre_run_command`, `pre_mcp_tool_use`). Each hook has an event, an optional matcher, and handler `command`(s); the handler receives action context as **JSON on stdin** and replies via exit code + stdout. **Pre-hooks can block via exit code 2**; post-hooks cannot block.

```json
{
  "hooks": {
    "post_write_code": [
      { "matcher": { "globs": ["**/*.py"] },
        "command": "mollify audit --gate new-only --format json --quiet 2>/dev/null > .mollify/last-audit.json || true" }
    ],
    "pre_run_command": [
      { "matcher": {}, "command": "scripts/mollify-guard.sh" }
    ]
  }
}
```

`post_write_code` records newly-introduced findings after each Python edit (non-blocking, surfaced to the agent); `pre_run_command`'s `mollify-guard.sh` can `exit 2` to block a disallowed command. Keep auto-fix OFF in hooks — hooks gather/gate evidence; the human or a workflow applies fixes. **Caveats:** the full event list and exact matcher schema were not fully confirmable past the egress block (see appendix) — verify against `docs.windsurf.com/windsurf/cascade/hooks` before operational reliance.

### 4.4 Memories & planning mode

- **Memories** live in `~/.codeium/windsurf/memories/`, workspace-scoped, local, **not committed**, no credit cost. Prompt Cascade once: *"Create a memory: Mollify audit JSON is the ground truth for dead code and circular deps in this repo. Never hand-delete code without a Mollify high-confidence fingerprint; always re-run mollify audit after removals."* Reinforces the rule across sessions without spending the 12k rule budget. This is reinforcement, not a distribution channel.
- **Planning mode** shows a plan preview before acting; the glob rule's job is to make "run Mollify audit first" appear in that plan whenever Python files are in scope.

### 4.5 Repo-wide rollout

- **Committed to every repo (PR-enforced):** `.devin/rules/mollify.md` (+ `.windsurf/rules/mollify.md` fallback), `.devin/workflows/{mollify-audit,mollify-cleanup,mollify-bootstrap}.md` (+ `.windsurf/workflows/` fallback).
- **Per-developer (one-time):** the `mcpServers.mollify` block in `~/.codeium/windsurf/mcp_config.json`, applied via `/mollify-bootstrap` or pushed centrally via enterprise system-level MCP config.
- **Scaffolding:** `mollify init --agent windsurf` writes all files + prints the MCP snippet — adoption in one command.

### 4.6 Cascade EOL + Devin Local (ACP) caveat

- **Cascade EOL is reported as 2026-07-01.** Successor agent is **Devin Local** — a Rust rewrite (~30% more token-efficient), with **sub-agents** (parallel specialized sessions reporting to a coordinator) and **ACP (Agent Client Protocol)** support so any ACP agent (Codex, Claude Agent, OpenCode) runs in the editor.
- **What carries forward (decisive):** our markdown rules and workflows are format-stable and forward-compatible — Devin Desktop continues to read existing Windsurf rules/workflows with no forced migration and **retains MCP**. Everything Mollify ships now survives the EOL. Build now; don't wait.
- **ACP vs MCP are orthogonal, not competing.** ACP is *agent ↔ editor* (how the IDE/sub-agents drive an agent). MCP is *agent ↔ tools* (how the agent calls Mollify). Mollify stays an **MCP server + CLI** and does **not** need to speak ACP. What ACP *would add*: sub-agents make Mollify *more* valuable — a coordinator can spawn a dedicated "audit" sub-agent that hammers the Mollify MCP server in parallel.
- **Hedges baked in:** (1) ship `.devin/` as primary with `.windsurf/` as fallback; (2) keep a CLI fallback in every workflow so nothing breaks if MCP registration drifts; (3) document both the `windsurf/`-scoped and legacy MCP config paths. Net: no rework expected at EOL.

---

## 5. Cross-platform primitive matrix

| Agent | Rules / memory | Commands / workflows | Hooks | MCP | Memory |
|---|---|---|---|---|---|
| **Claude Code** | `CLAUDE.md` + skill | `commands/*.md` (`/plugin:cmd`) | **yes** — PostToolUse + Stop gate | `.mcp.json` / `plugin.json` | `CLAUDE.md` |
| **Codex** | `AGENTS.md` chain (32 KiB) | custom prompts (deprecated) | **yes** — `[hooks]` + `notify` | `config.toml` `[mcp_servers.*]` | `AGENTS.md` |
| **Cursor** | `.cursor/rules/*.mdc` | `.cursor/commands/*.md` | no | `.cursor/mcp.json` | (rules) |
| **Gemini CLI** | `GEMINI.md` | `.gemini/commands/*.toml` (`!{}`) | no | `.gemini/settings.json` | `GEMINI.md` (`/memory`) |
| **Devin Desktop / Cascade** | `.devin/rules/*.md` (`trigger`) **+ `.devin/skills/` (SKILL.md)** | `.windsurf/workflows/*.md` (`/slash`) | **yes** — `.windsurf/hooks.json` (pre/post; pre blocks via exit 2) | `~/.codeium/windsurf/mcp_config.json` | `~/.codeium/windsurf/memories/` |
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
- Hook artifacts only where deterministic enforcement exists (Claude Code, Codex).

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

**Devin Desktop / Cascade — MEDIUM.** `docs.windsurf.com` and `devin.ai` were egress-blocked (403); reached via search snippets + secondary guides + the Windsurf-Samples catalog. Confirmed: rule trigger modes, 12,000 / 6,000-char limits, workflow `/slash` invocation, MCP path (unchanged after rebrand) + transports + `alwaysAllow` + `${env:VAR}` + 100-tool cap, memories.
*Not confirmable here / highest-risk:* the exact `2026-07-01` EOL date; precise `.devin/rules` precedence internals; and especially **the `auto_execute_steps` frontmatter field name/shape** (historically `auto_execution_mode`, an integer) — verify against the live Devin Desktop docs before operational reliance. **Precedence corrected: `.devin/` is preferred/primary, `.windsurf/` is the fallback.**
