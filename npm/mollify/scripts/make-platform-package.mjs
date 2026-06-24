#!/usr/bin/env node
// Generates an `@mollify-cli/<suffix>` platform package that ships a single
// prebuilt `mollify` binary, with the right `os`/`cpu`/`libc` constraints so
// npm only installs it on a matching machine.
//
// Usage:
//   node make-platform-package.mjs <suffix> <version> <binaryPath> <outDir>
// e.g.
//   node make-platform-package.mjs linux-x64-gnu 0.1.0 target/release/mollify npm

import fs from "node:fs";
import path from "node:path";

const [, , suffix, version, binaryPath, outDir] = process.argv;
if (!suffix || !version || !binaryPath || !outDir) {
  console.error(
    "usage: make-platform-package.mjs <suffix> <version> <binaryPath> <outDir>",
  );
  process.exit(1);
}

// suffix -> { os, cpu, libc? }
const META = {
  "darwin-arm64": { os: "darwin", cpu: "arm64" },
  "darwin-x64": { os: "darwin", cpu: "x64" },
  "linux-x64-gnu": { os: "linux", cpu: "x64", libc: "glibc" },
  "linux-arm64-gnu": { os: "linux", cpu: "arm64", libc: "glibc" },
  "linux-x64-musl": { os: "linux", cpu: "x64", libc: "musl" },
  "linux-arm64-musl": { os: "linux", cpu: "arm64", libc: "musl" },
  "win32-x64-msvc": { os: "win32", cpu: "x64" },
  "win32-arm64-msvc": { os: "win32", cpu: "arm64" },
};

const meta = META[suffix];
if (!meta) {
  console.error(`unknown platform suffix: ${suffix}`);
  process.exit(1);
}

const pkgDir = path.join(outDir, "@mollify-cli", suffix);
fs.mkdirSync(pkgDir, { recursive: true });

const isWin = meta.os === "win32";
const binName = isWin ? "mollify.exe" : "mollify";
fs.copyFileSync(binaryPath, path.join(pkgDir, binName));
if (!isWin) fs.chmodSync(path.join(pkgDir, binName), 0o755);

const pkg = {
  name: `@mollify-cli/${suffix}`,
  version,
  description: `Prebuilt mollify binary for ${suffix}.`,
  license: "MIT",
  repository: { type: "git", url: "git+https://github.com/FavioVazquez/mollify.git" },
  os: [meta.os],
  cpu: [meta.cpu],
  ...(meta.libc ? { libc: [meta.libc] } : {}),
  files: [binName],
};

fs.writeFileSync(
  path.join(pkgDir, "package.json"),
  JSON.stringify(pkg, null, 2) + "\n",
);
fs.writeFileSync(
  path.join(pkgDir, "README.md"),
  `# @mollify-cli/${suffix}\n\nPrebuilt \`mollify\` binary for \`${suffix}\`. Installed automatically as an optional dependency of [\`mollify\`](https://www.npmjs.com/package/mollify).\n`,
);

console.log(`wrote ${pkgDir} (${binName}, ${version})`);
