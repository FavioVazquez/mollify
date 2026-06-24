Inspect a single file with Mollify. `mollify inspect` returns an evidence bundle for one file: its findings plus its import neighborhood.

Steps:
1. Run `mollify inspect <file>` for the file the user specified (e.g. `mollify inspect app/services/billing.py`; or call the mollify MCP `mollify_inspect` tool with `file`). Add `--path <dir>` if a subproject was specified.
2. Summarize the file's findings (rule, confidence, reason, fingerprint, location) and its import neighborhood (what it imports and what imports it).
3. Do NOT modify the file. Present findings and let the user decide on any action.
