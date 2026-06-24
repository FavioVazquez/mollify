# mollify

**Deterministic codebase intelligence for Python.**

Dead code, duplication, circular dependencies, complexity & hotspots,
architecture, dependency hygiene, type health, and security — as evidence, not
guesses. Rust-native, one deterministic pass, with a CLI, an LSP server, an MCP
server, and version-matched agent integrations.

> Python users should prefer [`uv`](https://docs.astral.sh/uv/) or `pip`:
> `uvx mollify audit` / `uv tool install mollify` / `pip install mollify`.
> This npm package exists for JS/TS-centric repos and toolchains.

## Installation

```bash
npm install --save-dev mollify   # or: pnpm add -D mollify / yarn add -D mollify / bun add -d mollify
```

Installs the `mollify` CLI plus the companion `mollify-lsp` and `mollify-mcp`
binaries (the same Rust binary, dispatched to its `lsp` / `mcp` subcommands).
The platform-specific binary is pulled in automatically as an optional
dependency (`@mollify-cli/<platform>`).

For one-off use without adding a dependency, run `npx mollify`.

## Quick start

```bash
npx mollify audit                  # unified report + 0–100 quality score
npx mollify audit --format json    # machine-readable, kind-discriminated
npx mollify dead-code              # reachability-based unused files/symbols
npx mollify fix --dry-run          # preview safe auto-fixes
```

## Agent integrations

Mollify ships ready-to-commit skills, rules, hooks, slash-commands, and
workflows for several coding agents. Install them into your repo with the CLI
(works regardless of how mollify was installed):

```bash
npx mollify init --agent claude     # or cursor / gemini / codex / cascade
npx mollify init --all              # every supported agent
```

## Typed output contract

Parsing `mollify --format json` in TypeScript? Import the typed shapes,
version-pinned to your installed CLI:

```ts
import type { MollifyReport, Finding, AuditReport } from "mollify/types";
```

## MCP server

Agents that speak MCP can launch the bundled server. As a devDependency it
lives in `node_modules/.bin/`, so launch it through your runner:

```json
{
  "mcpServers": {
    "mollify": {
      "command": "npx",
      "args": ["mollify-mcp"]
    }
  }
}
```

Swap `npx` for `pnpm exec` / `yarn` / `bunx` to match your package manager.

## License

[MIT](https://github.com/FavioVazquez/mollify/blob/main/LICENSE) © Favio Vázquez
