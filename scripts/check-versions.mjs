// Fails when package.json, Cargo.toml and tauri.conf.json disagree on the version (run in CI and before tagging).
import { readFileSync } from "node:fs";
const pkg = JSON.parse(readFileSync("package.json", "utf8")).version;
const tauri = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const cargo = readFileSync("src-tauri/Cargo.toml", "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const tag = process.argv[2]?.replace(/^v/, ""); // optional: release tag, e.g. v0.1.0
const all = { "package.json": pkg, "Cargo.toml": cargo, "tauri.conf.json": tauri, ...(tag ? { tag } : {}) };
if (new Set(Object.values(all)).size !== 1) {
  console.error("Version mismatch:", all);
  process.exit(1);
}
console.log("Versions agree:", pkg);
