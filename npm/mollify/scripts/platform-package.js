// Maps the current platform + arch (+ libc on Linux) to the name of the
// optional dependency that ships the prebuilt `mollify` binary.

"use strict";

// platform -> arch -> (libc ->) package suffix.
const TABLE = {
  darwin: {
    arm64: "darwin-arm64",
    x64: "darwin-x64",
  },
  linux: {
    x64: { gnu: "linux-x64-gnu", musl: "linux-x64-musl" },
    arm64: { gnu: "linux-arm64-gnu", musl: "linux-arm64-musl" },
  },
  win32: {
    x64: "win32-x64-msvc",
    arm64: "win32-arm64-msvc",
  },
};

// Returns the `@mollify-cli/<suffix>` package name, or null if unsupported.
function getPlatformPackage(platform, arch, libc) {
  const byArch = TABLE[platform];
  if (!byArch) return null;
  const entry = byArch[arch];
  if (!entry) return null;
  if (typeof entry === "string") return `@mollify-cli/${entry}`;
  // Linux: resolve by libc family, defaulting to musl (static, runs anywhere).
  const suffix = entry[libc] || entry.musl;
  return suffix ? `@mollify-cli/${suffix}` : null;
}

module.exports = { getPlatformPackage };
