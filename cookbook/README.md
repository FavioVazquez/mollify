# Mollify Cookbook

Short, copy-pasteable recipes that show what Mollify does and how to use it — in
minutes, not hours. Every recipe runs against the tiny **[sample
project](sample-project/)** bundled here, so you can follow along even before you
point Mollify at your own code. **The output blocks below are real** — captured
from these exact commands against `sample-project/`.

> New to Mollify? It's a single deterministic binary that maps a Python codebase
> for dead code, dependency hygiene, complexity, duplication, architecture,
> type health, and security — emitting **evidence, not decisions**. Same input →
> byte-identical output. See the [README](../README.md) for the full picture.

## Setup (30 seconds)

Pick whichever you have. Every channel ships the *same* binary.

```bash
uvx mollify --version            # Python users — no install (recommended)
pipx run mollify --version       # or pipx
cargo install mollify-cli        # Rust users (binary: mollify)
```

Building this repo from source instead? The binary lands at
`./target/release/mollify` after `cargo build --release`. The recipes call it as
`mollify`; if you're using the source build, substitute that path (or put it on
your `PATH`).

Now grab the sample project and you're ready:

```bash
cd cookbook/sample-project
mollify audit          # see Recipe 01
```

## The recipes

| # | Recipe | You'll learn |
|---|--------|--------------|
| 01 | [Your first audit](recipes/01-first-audit.md) | Run one command, read the quality score and findings |
| 02 | [Anatomy of a finding](recipes/02-anatomy-of-a-finding.md) | Confidence tiers, fingerprints, and `explain` |
| 03 | [Clean up dead code, safely](recipes/03-clean-dead-code.md) | `fix` (dry-run → apply) and suppressions |
| 04 | [Dependency hygiene](recipes/04-dependency-hygiene.md) | Find unused & missing distributions |
| 05 | [Gate a PR on *new* issues only](recipes/05-ci-gate.md) | Baselines, SARIF, exit codes — the CI story |
| 06 | [Map your codebase](recipes/06-map-your-codebase.md) | `metrics`, `complexity`, `graph`, `trace`, `inspect` |
| 07 | [JSON for scripts & AI agents](recipes/07-json-and-agents.md) | The `kind`-discriminated contract, `jq`, MCP |

Prefer to watch it all at once? Run the guided tour:

```bash
./cookbook/scripts/tour.sh        # narrates every command against the sample project
```

## The sample project

`sample-project/` is a deliberately messy little Python package (`billing/`). It
packs one of almost every problem Mollify detects into ~40 lines:

- an unused module, function, import, and local variable (**dead code**)
- two declared-but-never-imported dependencies (**dependency hygiene**)
- an over-nested function past the complexity threshold (**complexity**)
- a `hashlib.md5` password hash (**security**, CWE-327)
- public functions with no type annotations (**type health**)

A clean audit scores it **71/100 with 20 findings** — your starting line for the
recipes. Nothing here is fixed for real; the project stays messy on purpose so the
recipes are reproducible.
