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
  "max_cognitive": 15,
  "architecture": {
    "preset": "layered",
    "layers": ["api", "service", "domain", "infra"]
  }
}
```

## `severity`

Map of **rule id** or **category** → `error` | `warn` | `off`.

- A rule id wins over its category (e.g. `unused-export` overrides `dead-code`).
- `error` findings make the CLI exit non-zero — this is how you gate CI and make
  agent hooks blocking.
- `off` drops the finding entirely.

Rule ids: `unused-file`, `unused-export`, `unused-import`, `commented-code`,
`unused-dependency`, `missing-dependency`, `circular-dependency`,
`layer-violation`, `forbidden-import`, `independence-violation`,
`high-complexity`, `duplication`, `untyped-function`, `cold-code`, `hotspot`,
`vulnerable-dependency`, the security rules (`dangerous-eval`,
`subprocess-shell-true`, `sql-injection`, `unsafe-yaml-load`,
`unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret`,
`weak-hash`, `weak-cipher`, `insecure-random`, `request-without-timeout`), and
any custom policy ids. Run `mollify explain` for the full list.

Categories: `dead-code`, `dependency-hygiene`, `circular-dependency`,
`complexity`, `architecture`, `duplication`, `type-health`, `security`.

## `architecture`

Opt into **layer-boundary** checking. `layers` is an ordered list, top
(most-dependent) → bottom: a module may import its own or lower layers, but
importing a *higher* layer is a `layer-violation`. A layer name matches when it
appears as a path/module segment.

```json
"architecture": { "layers": ["api", "service", "domain", "infra"] }
```

`preset` expands to a conventional ordering when you don't supply `layers`:

| preset | layers (top → bottom) |
| --- | --- |
| `layered` | presentation, application, domain, infrastructure |
| `hexagonal` | adapters, application, domain |
| `feature-sliced` / `bulletproof` | app, features, entities, shared |

## `policies` (rule packs)

Declarative bans, enforced deterministically (a literal match is a `certain`
finding). Each policy forbids an import and/or a call, optionally scoped to path
substrings via `in_paths` (omit for project-wide). The `id` becomes the rule id
and its suppression comment.

```json
"policies": [
  {
    "id": "no-requests-in-domain",
    "forbid_import": "requests",
    "in_paths": ["domain/"],
    "message": "domain must stay I/O-free",
    "severity": "error"
  },
  { "id": "no-print", "forbid_call": "print", "severity": "warn" }
]
```

`forbid_import`/`forbid_call` match by prefix: `requests` matches `requests` and
`requests.get`; `os.system` matches exactly. Policy findings surface under
`mollify arch` and `mollify audit`.

## `contracts` (module boundaries)

Declarative import contracts (import-linter / tach style), checked over the
import graph. `forbidden` bans a module (by dotted prefix) from importing
others; `independent` declares a set of modules that must not import one another.

```json
"contracts": {
  "forbidden": [ { "from": "app.domain", "to": ["app.web", "requests"] } ],
  "independent": [ ["features.billing", "features.users"] ]
}
```

These emit `forbidden-import` / `independence-violation` (Architecture, certain).

## Advisory database (supply-chain)

`mollify supply-chain` (and `mollify audit`, when the file exists) reads a local
advisory database at `.mollify/advisories.json` in the `mollify-advisories/1`
schema. It is an *input*, not a network call — regenerate it out-of-band with
`scripts/fetch-advisories.py` (OSV.dev / safety-db). Override the path with
`mollify supply-chain --advisory-db <path>`. See `examples/advisories.sample.json`.

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
