import { readFileSync, writeFileSync, copyFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { sha256, requiredFile, section, value } from "./runtime-integrity.mjs";

export function replaceOnce(source, pattern, replacement) {
  if ([...source.matchAll(new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : pattern.flags + "g"))].length !== 1) throw new Error(`expected one match: ${pattern}`);
  return source.replace(pattern, replacement);
}
export function updateManifest(source, hash) {
  const old = section(source, "tunnel_client");
  let block = replaceOnce(old, /^executable_sha256 = "[a-f0-9]+"/gm, `executable_sha256 = "${hash}"`);
  block = replaceOnce(block, /^vendoring = "[^"]+"/gm, 'vendoring = "source-built"');
  return source.replace(old, block);
}
export function registerTunnel(root, binary, commit, compiler) {
  const manifestPath = join(root, "runtime-manifest.toml");
  const manifest = readFileSync(manifestPath, "utf8");
  const block = section(manifest, "tunnel_client");
  if (commit !== value(block, "git_commit")) throw new Error("source commit differs from pinned manifest");
  const bytes = requiredFile(binary), hash = sha256(bytes);
  const rustPath = join(root, "src-tauri/src/tunnel/bundle.rs");
  const rust = replaceOnce(readFileSync(rustPath, "utf8"), /pub\(crate\) const TUNNEL_CLIENT_SHA256: &str =\s*"[a-f0-9]+";/g, `pub(crate) const TUNNEL_CLIENT_SHA256: &str =\n    "${hash}";`);
  const lockPath = join(root, "provenance/runtime-lock.json");
  const lock = JSON.parse(readFileSync(lockPath, "utf8").replace(/^\uFEFF/, ""));
  const components = lock.components.filter((c) => c.name === "openai-tunnel-client");
  if (components.length !== 1 || components[0].commit !== commit) throw new Error("ambiguous or mismatched Tunnel lock entry");
  components[0].sha256 = hash;
  components[0].build_type = "source-built";
  const updated = updateManifest(manifest, hash);
  // Validate every replacement before publishing any metadata.
  copyFileSync(binary, join(root, "runtime/tunnel-client/tunnel-client.exe"));
  writeFileSync(rustPath, rust);
  writeFileSync(manifestPath, updated);
  writeFileSync(lockPath, JSON.stringify(lock, null, 2) + "\n");
  writeFileSync(join(root, "provenance/tunnel-client.json"), JSON.stringify({ component: "openai-tunnel-client", source: value(block, "source"), version: value(block, "version"), commit, compiler, target: "windows-amd64", cgo: false, sha256: hash, built_at: new Date().toISOString(), build_origin: "source", status: "built-and-registered" }, null, 2) + "\n");
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) registerTunnel(resolve(import.meta.dirname, "../.."), ...process.argv.slice(2));
