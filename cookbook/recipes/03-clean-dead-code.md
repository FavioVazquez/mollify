# Recipe 03 — Clean up dead code, safely

**Goal:** delete the cruft Mollify found — without breaking anything. The trick
is that Mollify only *automatically* touches what it can **prove** is safe.

## Preview first (always a dry-run)

`mollify fix` shows exactly what it *would* remove and changes nothing until you
say so:

```bash
cd cookbook/sample-project
mollify fix
```

```text
5 safe fix(es) (dry-run — pass --apply to write):
  ./billing/app.py:1-1  Remove the unused import `os`
  ./billing/app.py:12-14  Delete unused function `_legacy_helper`
  ./billing/services/invoice.py:1-1  Remove the unused import `requests`
  ./billing/services/invoice.py:2-2  Remove the unused import `Money`
  ./billing/services/invoice.py:3-3  Remove the unused import `ledger`
```

Notice what's **not** in that list. The dead-code engines report 8 findings, but
`fix` offers only **5** — every one `certain`. The `likely` ones (the public
`password_hash`, the unused local) are left for you to judge. **Mollify removes
only what it can prove; it never guesses with your code.**

## Apply when you're ready

```bash
mollify fix --apply
```

This is the candidate/verifier split in action: Mollify *proposes* candidates,
and only `certain` + `auto_fixable` ones are ever written. Re-run `mollify
dead-code` afterward and the 5 fixed findings are gone; the judgement calls
remain.

> Tip: run `fix --apply` on a clean working tree (commit or stash first) so the
> change is a reviewable diff. It's deterministic, so the same input always
> produces the same edit.

## Keeping a finding you disagree with

Sometimes a "dead" symbol is intentional — a public API, a plugin hook, something
reached by reflection. Two ways to silence it:

**1. Inline suppression** — drop the finding's suppression comment on the line:

```python
def password_hash(p):   # mollify: ignore[unused-export]
    ...
```

(Every finding's JSON carries its exact `suppression_comment`, so you never have
to guess the syntax — see Recipe 07.)

**2. Config** — turn a rule or whole category down in `.mollifyrc.json`:

```json
{ "severity": { "unused-export": "off" } }
```

**Next:** [Recipe 04 — Dependency hygiene »](04-dependency-hygiene.md)
