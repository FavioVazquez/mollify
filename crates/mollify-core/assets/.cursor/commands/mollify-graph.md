Build the module dependency graph with Mollify and summarize its structure. Mollify computes the graph deterministically from static imports. You are the verifier.

Steps:
1. Run `mollify graph --format json` (or call the mollify MCP — note: `graph` is a CLI command; use `mollify_arch` for architecture findings). Add `--path <dir>` if a subproject was specified. Add `--mermaid` to emit a Mermaid diagram instead of JSON.
2. Summarize the graph: number of modules and edges, highly-connected hubs, and any cycles (cross-reference `mollify arch` for `circular-dependency` findings).
3. If the user wants a visual, run `mollify graph --mermaid` and present the Mermaid diagram.
4. Do NOT modify any files — this is read-only analysis.

Notes:
- The graph is built from static imports; dynamic imports (`importlib`/`getattr`) may not appear.
- For layer/boundary/cycle *findings* (not just the raw graph), use `mollify arch`.
