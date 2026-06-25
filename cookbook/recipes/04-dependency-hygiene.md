# Recipe 04 — Dependency hygiene

**Goal:** find dependencies you declare but never use (bloat, slower installs,
bigger attack surface) — and imports you use but never declared (works on your
machine, breaks in CI).

```bash
cd cookbook/sample-project
mollify deps
```

```text
Mollify deps — .
2 finding(s) across 7 file(s) — 0 error, 2 warn
  ./pyproject.toml:1 [warn/likely] unused-dependency — declared dependency `fastapi` is never imported  (unused-dependency:3f2dc71f)
  ./pyproject.toml:1 [warn/likely] unused-dependency — declared dependency `rich` is never imported  (unused-dependency:9333f874)
```

The sample's `pyproject.toml` declares three dependencies but the code only ever
imports `requests`. Mollify maps **declared distributions ↔ actual imports** and
reports the gap in both directions:

- **`unused-dependency`** — declared in your manifest, never imported. (`fastapi`,
  `rich` above.) Candidates to drop.
- **`missing-dependency`** — imported in code, not declared anywhere. These are
  the ones that bite you in a fresh environment.
- **`transitive-dependency`** — imported but only present because *something else*
  pulled it in. Works today; breaks the day that intermediary drops it.

## It understands the whole Python packaging zoo

You don't configure your toolchain — Mollify detects it. It reads
`pyproject.toml`, `requirements*.txt`, `uv.lock`, and PDM, and it's **venv-aware**
so it maps an import like `import yaml` back to the `PyYAML` distribution (the
import name and the package name differ more often than you'd think).

## Why this beats grep

Naively grepping imports misses three things Mollify gets right: import-name vs
distribution-name mismatches (`cv2` → `opencv-python`), extras, and the
transitive-vs-direct distinction. It's the equivalent of `deptry`, folded into the
same single pass as everything else.

**Next:** [Recipe 05 — Gate a PR on *new* issues only »](05-ci-gate.md)
