# Mollify JSON contract (schema_version 0.1)

Every `--format json` invocation prints one envelope. Clients **switch on `kind`**.
The MCP server (`mollify mcp`) returns the same envelope.

```jsonc
{
  "kind": "audit",            // "audit" | "dead-code" | "deps"
  "schema_version": "0.1",
  "quality_score": 77,         // audit only, 0-100
  "summary": {
    "total": 7,
    "errors": 0,
    "warnings": 7,
    "files_analyzed": 3
  },
  "findings": [
    {
      "fingerprint": "unused-export:931a82e6",
      "rule": "unused-export",
      "category": "dead-code",                 // dead-code | dependency-hygiene
      "severity": "warn",                       // error | warn | off
      "confidence": "certain",                  // certain | likely | uncertain
      "reason": "function `_x` has no reachable references in the project",
      "location": { "path": "app.py", "line": 6, "end_line": 7 },
      "actions": [
        {
          "type": "remove-symbol",
          "description": "Delete unused function `_x`",
          "auto_fixable": true,                 // act only when confidence==certain
          "suppression_comment": "# mollify: ignore[unused-export]"
        }
      ]
    }
  ]
}
```

Guarantees: findings are **sorted deterministically** (path, line, rule,
fingerprint); identical input -> byte-identical output. Pin agent skills to
`schema_version`. Only act automatically on `confidence: certain`; surface
`likely`/`uncertain` for human review.
