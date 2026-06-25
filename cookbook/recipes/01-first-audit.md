# Recipe 01 — Your first audit

**Goal:** point Mollify at a project and understand what comes back. One command,
no config.

```bash
cd cookbook/sample-project
mollify audit
```

```text
Mollify audit — .
Quality score: 71/100
20 finding(s) across 7 file(s) — 0 error, 20 warn
  ./billing/app.py:1 [warn/likely] unused-file — module `billing.app` is never imported and is not an entry point  (unused-file:acccc57d)
  ./billing/app.py:1 [warn/certain] unused-import — import `os` is never used in this module  (unused-import:dda43b1c)
  ./billing/app.py:7 [warn/likely] unused-export — function `main` has no reachable references in the project  (unused-export:25516e8e)
  ./billing/app.py:12 [warn/certain] unused-export — function `_legacy_helper` has no reachable references in the project  (unused-export:93948eee)
  ./billing/app.py:13 [warn/likely] unused-variable — local variable `tmp` is assigned but never used  (unused-variable:20c24491)
  ./billing/app.py:17 [warn/likely] untyped-function — public function `password_hash` has no type annotations (0/1 params typed, no return type)  (untyped-function:dd33214d)
  ./billing/app.py:18 [warn/likely] weak-hash — `hashlib.md5` is a weak hash; use sha256+ (or pass usedforsecurity=False) [CWE-327]  (weak-hash:5468b0a6)
  ./billing/services/invoice.py:6 [warn/certain] high-complexity — function `create_invoice` is complex (cyclomatic 7, cognitive 21); thresholds 10/15  (high-complexity:d62ca38c)
  ./pyproject.toml:1 [warn/likely] unused-dependency — declared dependency `rich` is never imported  (unused-dependency:9333f874)
  … 11 more
```

That's the whole product in one screen. `audit` runs **every** engine — dead
code, deps, complexity, duplication, architecture, types, security — in a single
deterministic pass and folds the result into one report.

## How to read it

```
./billing/app.py:12   [warn/certain]   unused-export   — function `_legacy_helper` …   (unused-export:93948eee)
└── where ─────────┘   └─ sev/conf ─┘   └── rule ────┘   └────────── reason ────────┘   └── fingerprint ──┘
```

- **Quality score (0–100)** — one number for "how healthy is this code?" Great for
  trend lines and badges. The sample sits at **71**.
- **severity** (`error` / `warn`) decides the **exit code**. Warnings exit `0`;
  errors exit `1`. Everything here is `warn`, so `audit` exits `0` — it's
  advisory until *you* decide a rule should fail the build (Recipe 05).
- **confidence** (`certain` / `likely` / `uncertain`) is Mollify's honesty knob.
  Python dead-code detection is undecidable in general, so a `certain` finding is
  provable and a `likely` one has a small residual dynamic risk. Only `certain`
  findings are ever auto-fixed (Recipe 03).
- **fingerprint** (`unused-export:93948eee`) is a stable, content-derived id. It
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

**Next:** [Recipe 02 — Anatomy of a finding »](02-anatomy-of-a-finding.md)
