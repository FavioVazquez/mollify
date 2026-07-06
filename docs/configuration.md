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

## `mollify init` — starter config

`mollify init` scaffolds a documented starter `.mollifyrc.json` (it leaves an
existing one untouched). The generated file sets the core areas to `warn`,
silences `type-health` by default, and surfaces the complexity thresholds as
obvious knobs. `_comment` keys are ignored by the loader and act as inline docs
(JSON has no comments):

```json
{
  "_comment": "mollify config — see docs/configuration.md. Severities: error | warn | off.",
  "source_roots": [".", "src"],
  "severity": {
    "dead-code": "warn",
    "dependency-hygiene": "warn",
    "type-health": "off"
  },
  "ignore": [],
  "exclude_dirs": [],
  "max_cyclomatic": 10,
  "max_cognitive": 15
}
```

To install agent integrations instead of a config, use `mollify init --agent
<name>` / `--all` (see the [README](../README.md#install)).

## pytest test paths

Dead-code reachability treats `test_*`/`Test*` collection roots in test paths as
reachable. Beyond the `tests/`/`test/` convention, mollify reads
`[tool.pytest.ini_options].testpaths` from `pyproject.toml` to widen what counts
as a test path. Note: only `pyproject.toml` is consulted today — `pytest.ini`,
`setup.cfg`, and `tox.ini` are not yet read, so projects that configure
`testpaths` only in those files should add the dir to the `tests/` convention or
declare it in `pyproject.toml`.

## `severity`

Map of **rule id** or **category** → `error` | `warn` | `off`.

- A rule id wins over its category (e.g. `unused-export` overrides `dead-code`).
- `error` findings make the CLI exit non-zero — this is how you gate CI and make
  agent hooks blocking.
- `off` drops the finding entirely.

Rule ids: `unused-file`, `unused-export`, `unused-import`, `unused-variable`,
`unused-parameter`, `unused-method`, `unused-attribute`, `unused-enum-member`,
`unreachable-code`, `commented-code`,
`unused-dependency`, `missing-dependency`, `transitive-dependency`,
`misplaced-dev-dependency`, `unresolved-import`, `duplicate-export`, `private-import`,
`circular-dependency`, `low-cohesion`,
`layer-violation`, `forbidden-import`, `independence-violation`,
`high-complexity`, `duplication`, `untyped-function`, `private-type-leak`, `cold-code`, `hotspot`,
`vulnerable-dependency`, the security rules (`dangerous-eval`,
`subprocess-shell-true`, `sql-injection`, `unsafe-yaml-load`,
`unsafe-deserialization`, `tls-verify-disabled`, `hardcoded-secret`,
`weak-hash`, `weak-cipher`, `insecure-random`, `request-without-timeout`,
`flask-debug-true`, `jinja2-autoescape-false`, `try-except-pass`), and
`policy-violation` and any custom policy ids. Run `mollify explain` for the full list.

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

`mollify supply-chain` is **live by default**: it queries OSV.dev for each
pinned package, falling back to the local advisory database at
`.mollify/advisories.json` (schema `mollify-advisories/1`) when the network is
unavailable. Pass `--offline` to skip the fetch entirely and use only the local
DB (fully deterministic), and `--refresh` to cache a live fetch into the DB for
later offline runs. `mollify audit` never touches the network — it folds in
supply-chain findings from the local DB only, when the file exists. Seed or
regenerate the DB with `scripts/fetch-advisories.py` (OSV.dev / safety-db), and
override its path with `mollify supply-chain --advisory-db <path>`. See
`cookbook/advisories.sample.json`.

## `ignore`

A list of path substrings. Any finding whose file path contains one is dropped.
(Glob support is planned.) This is a post-analysis filter on findings, not a
discovery-time exclusion — see `exclude_dirs` below for the latter.

## `exclude_dirs`

A list of extra directory **names** pruned from discovery entirely (the
parser never touches files inside them), in addition to a builtin denylist
that's always active — no configuration needed for the common case:

```
.bzr .direnv .eggs .git .hg .svn .ipynb_checkpoints .mypy_cache .nox .pyenv
.pytest_cache .pytype .ruff_cache .tox .venv __pycache__ __pypackages__
_build buck-out build dist env node_modules site-packages venv
```

This mirrors `ruff`'s own default exclude list. In addition, **any** directory
that directly contains a `pyvenv.cfg` file is pruned regardless of its name —
this catches custom-named virtualenvs (e.g. from `mkvirtualenv` or a
non-standard Poetry/conda env name) that the name list can't anticipate.

`exclude_dirs` *adds to* this builtin list; it has no way to un-exclude a
builtin name from `.mollifyrc.json` itself.

```json
"exclude_dirs": ["vendor", "third_party"]
```

To scan a directory despite any of these — the builtin denylist, your own
`exclude_dirs`, or `.gitignore` — pass `--include <DIR>` on the command line
(repeatable). It's a per-invocation override, not a config setting, and works
on every analysis command except `coverage`/`supply-chain` (which aren't
path-scoped). It does not override the `pyvenv.cfg` virtualenv guard — an
included directory that is itself a virtualenv stays excluded.

```bash
mollify audit --include node_modules --include vendor
```

## `duplication`

Tune the clone detector: `min_tokens` (normalized-token window, default 40) and
`min_lines` (minimum clone line span, default 5).

```json
"duplication": { "min_tokens": 50, "min_lines": 6 }
```

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

Existing flake8-style `# noqa` comments are honored for the rules they map to:
a blanket `# noqa` (or `# noqa: F401`) silences `unused-import` on that line,
and `# noqa: F841` silences `unused-variable`. Other flake8 codes belong to
other tools and are not interpreted.
