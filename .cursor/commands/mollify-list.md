List project topology with Mollify. `mollify list` reports entry points, files, or detected frameworks.

Steps:
1. Run `mollify list [entry-points|files|frameworks]` (default lists all topology; or call the mollify MCP `mollify_list` tool with optional `kind`). Add `--path <dir>` if a subproject was specified.
2. Summarize the requested topology: entry points (reachability roots), analyzed files, and/or detected frameworks.
3. This is read-only context — use it to understand the project before running deeper analysis commands.
