# Configuration — `.mollifyrc.json`

Place a `.mollifyrc.json` at your project root (or run `mollify init`). All keys
are optional.

```json
{
  "severity": {
    "dead-code": "error",
    "unused-dependency": "warn",
    "duplication": "off"
  },
  "ignore": ["tests/", "migrations/", "generated/"],
  "max_cyclomatic": 10,
  "max_cognitive": 15
}
```

## `severity`

Map of **rule id** or **category** → `error` | `warn` | `off`.

- A rule id wins over its category (e.g. `unused-export` overrides `dead-code`).
- `error` findings make the CLI exit non-zero — this is how you gate CI and make
  agent hooks blocking.
- `off` drops the finding entirely.

Rule ids: `unused-file`, `unused-export`, `unused-dependency`,
`missing-dependency`, `circular-dependency`, `high-complexity`, `duplication`.

Categories: `dead-code`, `dependency-hygiene`, `circular-dependency`,
`complexity`, `architecture`, `duplication`.

## `ignore`

A list of path substrings. Any finding whose file path contains one is dropped.
(Glob support is planned.)

## `max_cyclomatic` / `max_cognitive`

Thresholds for the complexity engine. Functions strictly above either threshold
are reported as `high-complexity`. Defaults: 10 / 15.

## Inline suppression

Instead of config, suppress a single finding by adding its suppression comment on
the line, e.g.:

```python
def legacy_entrypoint():  # mollify: ignore[unused-export]
    ...
```
