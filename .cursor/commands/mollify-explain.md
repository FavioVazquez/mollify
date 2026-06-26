Explain a Mollify rule. Mollify documents each rule's semantics, confidence behavior, and suggested action.

Steps:
1. Run `mollify explain <rule>` for the rule the user asked about (e.g. `mollify explain unused-export`). With no argument, `mollify explain` lists all rules.
2. Relay the rule's meaning, what triggers it, how confidence is assigned, and the recommended action.

Valid rule ids include: `unused-file`, `unused-export`, `unused-import`, `unused-variable`, `unused-parameter`, `unused-method`, `unused-attribute`, `unused-enum-member`, `unreachable-code`, `commented-code`, `unused-dependency`, `missing-dependency`, `transitive-dependency`, `misplaced-dev-dependency`, `unresolved-import`, `duplicate-export`, `private-import`, `circular-dependency`, `layer-violation`, `forbidden-import`, `independence-violation`, `high-complexity`, `duplication`, `untyped-function`, `private-type-leak`, `cold-code`, `hotspot`, `low-cohesion`, `dangerous-eval`, `subprocess-shell-true`, `sql-injection`, `unsafe-yaml-load`, `unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret`, `weak-hash`, `weak-cipher`, `insecure-random`, `request-without-timeout`, `flask-debug-true`, `jinja2-autoescape-false`, `try-except-pass`, `vulnerable-dependency`, plus custom policy ids.
