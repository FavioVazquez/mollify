# Mollify JSON contract (schema_version 0.1)

Every `--format json` invocation prints one envelope. Clients **switch on `kind`**.

```jsonc
{
  "kind": "audit",            // "audit" | "dead-code" | "deps"
  "schema_version": "0.1",
  "quality_score": 77,         // audit only, 0–100
  "summary": {
    "total": 7,
    "errors": 0,
    "warnings": 7,
    "files_analyzed": 3,
    "introduced": 0            // present with --gate new-only (file-level attribution)
  },
  "findings": [
    {
      "fingerprint": "unused-export:931a82e6",
      "rule": "unused-export",
      "category": "dead-code",                 // dead-code | duplication |
                                                // circular-dependency | complexity |
                                                // architecture | dependency-hygiene
      "severity": "warn",                       // error | warn | off
      "confidence": "certain",                  // certain | likely | uncertain
      "attribution": "introduced",              // optional: introduced | inherited
      "reason": "function `_x` has no reachable references in the project",
      "location": { "path": "app.py", "line": 6, "end_line": 7 },
      "actions": [
        {
          "type": "remove-symbol",
          "description": "Delete unused function `_x`",
          "auto_fixable": true,                 // safe to apply only when confidence==certain
          "suppression_comment": "# mollify: ignore[unused-export]"
        }
      ]
    }
  ]
}
```

Guarantees: findings are **sorted deterministically** (path, line, rule,
fingerprint); identical input → byte-identical output. Pin agent skills to
`schema_version`.
