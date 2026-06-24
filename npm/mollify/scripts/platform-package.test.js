"use strict";

const { test } = require("node:test");
const assert = require("node:assert");

const { getPlatformPackage } = require("./platform-package");

test("maps macOS targets", () => {
  assert.equal(getPlatformPackage("darwin", "arm64"), "@mollify-cli/darwin-arm64");
  assert.equal(getPlatformPackage("darwin", "x64"), "@mollify-cli/darwin-x64");
});

test("maps Linux targets by libc, defaulting to musl", () => {
  assert.equal(getPlatformPackage("linux", "x64", "glibc"), "@mollify-cli/linux-x64-musl");
  assert.equal(getPlatformPackage("linux", "x64", "gnu"), "@mollify-cli/linux-x64-gnu");
  assert.equal(getPlatformPackage("linux", "arm64", "musl"), "@mollify-cli/linux-arm64-musl");
  assert.equal(getPlatformPackage("linux", "x64"), "@mollify-cli/linux-x64-musl");
});

test("maps Windows targets", () => {
  assert.equal(getPlatformPackage("win32", "x64"), "@mollify-cli/win32-x64-msvc");
  assert.equal(getPlatformPackage("win32", "arm64"), "@mollify-cli/win32-arm64-msvc");
});

test("returns null for unsupported platforms", () => {
  assert.equal(getPlatformPackage("sunos", "x64"), null);
  assert.equal(getPlatformPackage("linux", "mips"), null);
});
