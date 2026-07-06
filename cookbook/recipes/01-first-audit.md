# Recipe 01 — Your first audit

**Goal:** point Mollify at a project and understand what comes back. One command,
no config.

```bash
cd cookbook/sample-project
mollify audit
```

```text
Mollify audit — .
Quality score: 84/100
16 finding(s) across 7 file(s) — 0 error, 16 warn
  billing/app.py:1 [warn/certain] unused-import — import `os` is never used in this module  (unused-import:9a5e10483cc040fd)
  billing/app.py:12 [warn/certain] unused-export — function `_legacy_helper` has no reachable references in the project  (unused-export:b3454f8f0092acf5)
  billing/app.py:13 [warn/likely] unused-variable — local variable `tmp` is assigned but never used  (unused-variable:bced62ab9da79356)
  billing/app.py:17 [warn/likely] untyped-function — public function `password_hash` has no type annotations (0/1 params typed, no return type)  (untyped-function:a3c79d14005834f2)
  billing/app.py:17 [warn/likely] unused-export — function `password_hash` has no reachable references in the project  (unused-export:5024ab2f29d5cdeb)
  billing/app.py:18 [warn/likely] weak-hash — `hashlib.md5` is a weak hash; use sha256+ (or pass usedforsecurity=False) [CWE-327]  (weak-hash:2fda8ae0463ffdee)
  billing/services/invoice.py:1 [warn/certain] circular-dependency — import cycle: billing.services.invoice → billing.services.ledger → billing.services.invoice  (circular-dependency:d297a057a059c6fb)
  billing/services/invoice.py:1 [warn/certain] unused-import — import `requests` is never used in this module  (unused-import:5c1d3a12f0f0f698)
  billing/services/invoice.py:6 [warn/certain] high-complexity — function `create_invoice` is complex (cyclomatic 7, cognitive 21); thresholds 10/15  (high-complexity:1cbe4ffa6ee8ed1d)
  … 7 more
```

That's the whole product in one screen. `audit` runs **every** engine — dead
code, deps, complexity, duplication, architecture, types, security — in a single
deterministic pass and folds the result into one report.

## How to read it

```
billing/app.py:12   [warn/certain]   unused-export   — function `_legacy_helper` …   (unused-export:b3454f8f0092acf5)
└── where ───────┘   └─ sev/conf ─┘   └── rule ────┘   └────────── reason ────────┘   └───── fingerprint ─────┘
```

- **Quality score (0–100)** — one number for "how healthy is this code?" Great for
  trend lines and badges. The sample sits at **84** (penalties are weighted by
  confidence, so low-confidence review items count less than proven defects).
- **severity** (`error` / `warn`) decides the **exit code**. Warnings exit `0`;
  errors exit `1`. Everything here is `warn`, so `audit` exits `0` — it's
  advisory until *you* decide a rule should fail the build (Recipe 05).
- **confidence** (`certain` / `likely` / `uncertain`) is Mollify's honesty knob.
  Python dead-code detection is undecidable in general, so a `certain` finding is
  provable and a `likely` one has a small residual dynamic risk. Only `certain`
  findings are ever auto-fixed (Recipe 03).
- **fingerprint** (`unused-export:b3454f8f0092acf5`) is a stable, content-derived id. It
  survives line moves and reformatting — which is what makes baselines and
  PR-scoped gates possible (Recipe 05).

## Focus on one area

`audit` is the firehose. Each engine is also its own subcommand when you want to
zoom in:

```bash
mollify dead-code     # just unused files / exports / imports / variables
mollify deps          # just dependency hygiene
mollify complexity    # just complexity hotspots
mollify security      # just security candidates
mollify arch          # just architecture / import cycles
```

## Point it anywhere

```bash
mollify audit --path /path/to/your/project
```

No config file required. When you *want* to tune thresholds, severities, or
ignore paths, add a `.mollifyrc.json` (`mollify init` scaffolds one) — see
[configuration.md](../../docs/configuration.md).

## Control what gets scanned

Discovery always prunes a builtin denylist (`.venv`, `.git`, `__pycache__`,
`node_modules`, `build`, `dist`, …) plus anything your `.mollifyrc.json` adds via
`exclude_dirs` — so vendored or virtualenv code never shows up as a false
positive. The sample project actually has a `node_modules/` directory checked in
(an accidentally-vendored helper), and it's invisible by default:

```bash
mollify audit
```

```text
Mollify audit — .
Quality score: 84/100
16 finding(s) across 7 file(s) — 0 error, 16 warn
```

Need to scan one anyway — auditing a vendored fork before deleting it, or a
`node_modules` package you suspect is stale? `--include <DIR>` (repeatable)
overrides the builtin denylist, `exclude_dirs`, *and* `.gitignore` for that
directory name (this sample project's own `.gitignore` lists `node_modules/`),
one invocation at a time:

```bash
mollify audit --include node_modules
```

```text
Mollify audit — .
Quality score: 84/100
18 finding(s) across 8 file(s) — 0 error, 18 warn
  …
  node_modules/leftpad/__init__.py:4 [warn/likely] untyped-function — public function `pad` has no type annotations (0/2 params typed, no return type)  (untyped-function:7732bf595fb1c176)
  node_modules/leftpad/__init__.py:4 [warn/likely] unused-export — function `pad` has no reachable references in the project  (unused-export:54aef9d4df4d4f22)
  …
```

`--include` is a per-invocation override, not a config setting — it doesn't
touch `.mollifyrc.json`, so your team's defaults stay intact for everyone else.
It does not override the `pyvenv.cfg` virtualenv guard, so an `--include`'d
directory that's itself a virtualenv stays excluded.

**Next:** [Recipe 02 — Anatomy of a finding »](02-anatomy-of-a-finding.md)
