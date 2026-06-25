# Contributing to Mollify

Thanks for your interest! Mollify is a Rust workspace; the bar for every change
is simple: **it compiles, it's tested, and it's documented.**

## Setup

```bash
cargo build
cargo test --all
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

## Ground rules (the invariants)

Read [docs/architecture.md](docs/architecture.md) first. Do not break these:

1. **Determinism** — sort before you emit; no unordered iteration reaches output.
2. **Evidence, not decisions** — new rules produce findings with a `fingerprint`,
   a `confidence` tier, and a human `reason`. Only `certain` findings may be
   `auto_fixable`.
3. **The JSON contract is public** — additive changes bump `SCHEMA_VERSION`'s
   minor; breaking changes bump major.

## Adding a rule / engine

1. Implement it in a `mollify-core` module that takes `&ModuleGraph` and returns
   `Vec<Finding>`.
2. Give it a stable `rule` id and a `Category`; tier the `confidence` honestly.
3. Wire it into `audit_report` and (optionally) a CLI subcommand + `Report` variant.
4. Add unit tests (use a temp project; see existing engines for the pattern).
5. Update the docs (`docs/`) and the CLI reference under `.agents/skills/mollify/references/`.

## Documenting decisions

For any significant design decision or deviation, write an ADR in `docs/adr/`
(see ADR-0001 for the format). Never silently diverge.

## Commits

Keep commits focused and the tree green. Conventional, descriptive messages.
Keep the docs and CLI reference in sync with behavior changes.
