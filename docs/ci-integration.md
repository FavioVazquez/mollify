# CI integration

Mollify is built for CI: deterministic output, exit codes, SARIF, and a
PR-scoped gate.

## Fail a build on regressions only

`--gate new-only` attributes findings to changed files (vs `--base`) and reports
only the introduced ones, so existing debt doesn't block PRs:

```bash
mollify audit --gate new-only --base origin/main
# exit 1 if any *introduced* finding is error-severity
```

Make specific rules blocking via `.mollifyrc.json` (`"severity": {"dead-code": "error"}`).

## GitHub Actions

```yaml
name: mollify
on: [pull_request]
permissions:
  contents: read
  security-events: write          # required to upload SARIF to code scanning
jobs:
  mollify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
        with: { fetch-depth: 0 }          # needed for --gate/--base diffing
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install mollify-cli     # or: --git https://github.com/FavioVazquez/mollify
      - name: Audit changed code
        run: mollify audit --gate new-only --base origin/${{ github.base_ref }}
      - name: SARIF for code scanning
        if: always()
        run: mollify audit --format sarif > mollify.sarif
      - uses: github/codeql-action/upload-sarif@v4
        if: always()
        with: { sarif_file: mollify.sarif }
```

## GitLab CI

```yaml
mollify:
  image: rust:latest
  script:
    - cargo install --git https://github.com/FavioVazquez/mollify mollify-cli
    - mollify audit --format sarif > mollify.sarif
  artifacts:
    reports:
      sast: mollify.sarif
```

## JSON for custom tooling

`mollify audit --format json` emits the kind-discriminated contract
(`schema_version` `0.1`). Switch on `kind`, iterate `findings[]`, and key on
`severity` / `confidence` / `attribution`. See
`.agents/skills/mollify/references/json-contract.md`.
