import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { rejectExtras, requiredFile, sha256 } from "./test/runtime-integrity.mjs";
const root = resolve(import.meta.dirname, "..");
function run(program, args) {
  const result = spawnSync(program, args, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.status !== 0) throw new Error(`${program} failed (${result.status ?? result.error})`);
}
export function stageBroker(repository, build) {
  const stage = resolve(repository, "src-tauri/target/release-stage");
  const staged = resolve(stage, "localbridge-privileged-broker.exe");
  const evidence = resolve(stage, "broker-build.json");
  rejectExtras(stage, ["localbridge-privileged-broker.exe", "broker-build.json"]);
  mkdirSync(stage, { recursive: true });
  // Tauri checks this resource while compiling the broker itself.
  writeFileSync(evidence, JSON.stringify({ status: "BUILDING" }));
  if (!existsSync(staged)) writeFileSync(staged, Buffer.alloc(0));
  build();
  const binary = resolve(repository, "src-tauri/target/release/localbridge-privileged-broker.exe");
  const bytes = requiredFile(binary);
  copyFileSync(binary, staged);
  const hash = sha256(bytes);
  if (sha256(requiredFile(staged)) !== hash) throw new Error("broker staging mismatch");
  writeFileSync(evidence, JSON.stringify({ status: "PASS", sha256: hash, builtAt: new Date().toISOString() }, null, 2) + "\n");
}
export function prepareResources() {
  for (const forbidden of ["runtime/tunnel-client/cloudflared.exe", "runtime/tunnel-client/cloudflared-manifest.json"]) {
    if (existsSync(resolve(root, forbidden))) throw new Error(`forbidden payload: ${forbidden}`);
  }
  for (const path of ["runtime/python/python.exe", "runtime/coding-tools-mcp/coding_tools_mcp/__init__.py", "runtime/tunnel-client/tunnel-client.exe", "runtime-manifest.toml", "runtime-policy.toml", "LICENSE", "THIRD_PARTY_NOTICES.md"]) requiredFile(resolve(root, path));
  if (/cloudflared|cloudflare managed/i.test(readFileSync(resolve(root, "runtime-manifest.toml"), "utf8"))) throw new Error("obsolete runtime manifest");
  run(process.execPath, ["scripts/prepare-toolbox.mjs"]);
  stageBroker(root, () => run("cargo", ["+1.85.0", "build", "--manifest-path", "src-tauri/Cargo.toml", "--target-dir", "src-tauri/target", "--locked", "--release", "--features", "privileged-broker", "--bin", "localbridge-privileged-broker"]));
  console.log("LB018_RELEASE_RESOURCES=PASS broker=release toolbox=pinned");
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) prepareResources();
