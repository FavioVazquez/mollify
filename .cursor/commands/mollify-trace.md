Trace the import neighborhood of a module with Mollify. `mollify trace` shows what a module imports and what imports it.

Steps:
1. Run `mollify trace <module>` for the module the user specified (e.g. `mollify trace app.services.billing`; or call the mollify MCP `mollify_trace` tool with `module`). Add `--path <dir>` if a subproject was specified.
2. Summarize the import neighborhood: the module's imports (outgoing) and importers (incoming), and any cycle it participates in.
3. Use this to verify reachability before treating a dead-code finding as safe to delete.

Notes:
- Reachability is static; dynamic imports may not appear in the neighborhood.
