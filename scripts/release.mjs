// Release chores for Dipstick for Windows. See docs/releasing.md.
//
//   node scripts/release.mjs version 0.2.0   set the version everywhere it is written
//   node scripts/release.mjs check 0.2.0     every file agrees, and CHANGELOG.md has an entry
//   node scripts/release.mjs notes 0.2.0     print that entry
//   node scripts/release.mjs feed --version 0.2.0 --url <installer url> \
//        --signature <installer .sig> --notes <notes file> --out <latest.json>
//
// The version is written in four places — the installer and the updater read
// tauri.conf.json, the binary reads Cargo.toml — and a tag that disagrees with
// any of them would offer every installed copy an update to the version it
// already has, for ever. So `check` refuses to let that be published.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const at = (path) => join(root, path);

const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

function read(path) {
  return readFileSync(at(path), "utf8");
}

function write(path, text) {
  writeFileSync(at(path), text);
}

// Line endings either way: a Windows checkout — the release runner's — has
// CRLF unless told otherwise.
/** The `[package]` version of Cargo.toml, not a dependency's. */
const CARGO_VERSION = /(\[package\][^[]*?\nversion\s*=\s*")([^"]+)(")/;
/** Dipstick's own entry in Cargo.lock. */
const LOCK_VERSION = /(\[\[package\]\]\r?\nname = "dipstick"\r?\nversion = ")([^"]+)(")/;

function versions() {
  return {
    "package.json": JSON.parse(read("package.json")).version,
    "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
    "src-tauri/Cargo.toml": read("src-tauri/Cargo.toml").match(CARGO_VERSION)?.[2],
    "src-tauri/Cargo.lock": read("src-tauri/Cargo.lock").match(LOCK_VERSION)?.[2],
  };
}

function setVersion(version) {
  for (const path of ["package.json", "src-tauri/tauri.conf.json"]) {
    const json = JSON.parse(read(path));
    json.version = version;
    write(path, JSON.stringify(json, null, 2) + "\n");
  }

  const lock = JSON.parse(read("package-lock.json"));
  lock.version = version;
  if (lock.packages?.[""]) lock.packages[""].version = version;
  write("package-lock.json", JSON.stringify(lock, null, 2) + "\n");

  for (const [path, pattern] of [
    ["src-tauri/Cargo.toml", CARGO_VERSION],
    ["src-tauri/Cargo.lock", LOCK_VERSION],
  ]) {
    const text = read(path);
    if (!pattern.test(text)) fail(`no version found in ${path}`);
    write(path, text.replace(pattern, `$1${version}$3`));
  }
}

/** The body of `## <version>` in CHANGELOG.md, up to the next `## `. */
function notes(version) {
  const lines = read("CHANGELOG.md").split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === `## ${version}`);
  if (start < 0) return null;
  const end = lines.findIndex((line, index) => index > start && line.startsWith("## "));
  const body = lines.slice(start + 1, end < 0 ? undefined : end).join("\n").trim();
  return body || null;
}

function check(version) {
  if (!SEMVER.test(version)) fail(`"${version}" is not a version like 0.2.0`);
  let ok = true;
  for (const [path, found] of Object.entries(versions())) {
    if (found !== version) {
      console.error(`error: ${path} says ${found ?? "nothing"}, the tag says ${version}`);
      ok = false;
    }
  }
  if (!notes(version)) {
    console.error(`error: CHANGELOG.md has no "## ${version}" entry. Its words are what installed copies show.`);
    ok = false;
  }
  if (!ok) {
    console.error(`Run: node scripts/release.mjs version ${version}, write the entry, commit, then tag again.`);
    process.exit(1);
  }
  console.log(`Everything says ${version}.`);
}

function option(args, name) {
  const index = args.indexOf(`--${name}`);
  const value = index >= 0 ? args[index + 1] : undefined;
  if (!value) fail(`--${name} is needed`);
  return value;
}

/**
 * The updater's feed. The signature is the file `tauri build` wrote beside
 * the installer: without it, and without it matching, no installed copy will
 * run what the feed points at.
 */
function feed(args) {
  const version = option(args, "version");
  const signature = readFileSync(resolve(option(args, "signature")), "utf8").trim();
  if (!signature) fail("the signature file is empty");
  const document = {
    version,
    notes: readFileSync(resolve(option(args, "notes")), "utf8").trim(),
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    platforms: {
      "windows-x86_64": { signature, url: option(args, "url") },
    },
  };
  writeFileSync(resolve(option(args, "out")), JSON.stringify(document, null, 2) + "\n");
  console.log(`Feed offers ${version}.`);
}

const [command, ...args] = process.argv.slice(2);
switch (command) {
  case "version":
    if (!SEMVER.test(args[0] ?? "")) fail("usage: release.mjs version 0.2.0");
    setVersion(args[0]);
    console.log(`Set ${args[0]} in ${Object.keys(versions()).join(", ")} and package-lock.json.`);
    break;
  case "check":
    check(args[0] ?? "");
    break;
  case "notes": {
    const body = notes(args[0] ?? "");
    if (!body) fail(`CHANGELOG.md has no "## ${args[0]}" entry`);
    console.log(body);
    break;
  }
  case "feed":
    feed(args);
    break;
  default:
    fail("usage: release.mjs version|check|notes|feed …");
}
