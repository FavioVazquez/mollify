// Shared launcher for bin/mollify, bin/mollify-mcp, and bin/mollify-lsp.
//
// 1. Resolves the platform package for this process (platform + arch + libc).
// 2. Locates the prebuilt `mollify` binary inside it.
// 3. Execs the binary, optionally prefixing a subcommand (mcp / lsp) so the
//    single Rust binary backs all three npm bin entries.

"use strict";

const { execFileSync } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");

const { getPlatformPackage } = require("./platform-package");

function resolvePlatformPackageName() {
  if (process.platform !== "linux") {
    return getPlatformPackage(process.platform, process.arch);
  }
  try {
    const { familySync } = require("detect-libc");
    return getPlatformPackage(process.platform, process.arch, familySync());
  } catch {
    // musl binaries are statically linked and work on both glibc and musl.
    return getPlatformPackage(process.platform, process.arch, "musl");
  }
}

function resolveBinaryPath() {
  const pkg = resolvePlatformPackageName();
  if (!pkg) {
    process.stderr.write(
      `mollify: unsupported platform ${process.platform}-${process.arch}\n`,
    );
    process.exit(1);
  }
  let pkgDir;
  try {
    pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
  } catch {
    process.stderr.write(
      `mollify: could not find ${pkg}. Run your package manager's install to fetch the platform binary.\n`,
    );
    process.exit(1);
  }
  const exe = process.platform === "win32" ? "mollify.exe" : "mollify";
  const binaryPath = path.join(pkgDir, exe);
  if (!fs.existsSync(binaryPath)) {
    process.stderr.write(`mollify: binary not found at ${binaryPath}\n`);
    process.exit(1);
  }
  return binaryPath;
}

// `prefixArgs` lets mollify-mcp / mollify-lsp map onto `mollify mcp` / `mollify lsp`.
function runBinary(prefixArgs = []) {
  const binaryPath = resolveBinaryPath();
  const args = [...prefixArgs, ...process.argv.slice(2)];
  try {
    execFileSync(binaryPath, args, { stdio: "inherit" });
  } catch (e) {
    if (e.status === undefined) throw e;
    process.exit(e.status);
  }
}

module.exports = { runBinary, resolvePlatformPackageName };
