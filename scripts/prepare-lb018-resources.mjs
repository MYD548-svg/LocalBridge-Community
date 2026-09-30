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
export function compilePreflight(repository, execute = (program, args) => spawnSync(program, args, { cwd: repository, stdio: "inherit", windowsHide: true }), checkout = () => {
  const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repository, encoding: "utf8", windowsHide: true });
  if (git.status !== 0) throw new Error("cannot identify compile-preflight checkout");
  return git.stdout.trim();
}) {
  const target = "src-tauri/target";
  const common = ["--manifest-path", "src-tauri/Cargo.toml", "--target-dir", target, "--locked"];
  const checks = [
    ["test-compile", ["test", ...common, "--all-targets", "--no-run"]],
    ["test-clippy", ["clippy", ...common, "--all-targets", "--", "-D", "warnings"]],
    ["broker-compile", ["test", ...common, "--features", "privileged-broker", "--bin", "localbridge-privileged-broker", "--no-run"]],
    ["broker-clippy", ["clippy", ...common, "--features", "privileged-broker", "--bin", "localbridge-privileged-broker", "--", "-D", "warnings"]],
    ["adapter-compile", ["test", ...common, "--features", "mcp-adapter", "--bin", "localbridge-mcp", "--no-run"]],
    ["adapter-clippy", ["clippy", ...common, "--features", "mcp-adapter", "--bin", "localbridge-mcp", "--", "-D", "warnings"]],
  ].map(([id, args]) => ({ id, program: "cargo", args: ["+1.85.0", ...args], status: "NOT_RUN", exitCode: null }));
  const directory = resolve(repository, "tests/artifacts/ci");
  mkdirSync(directory, { recursive: true });
  const report = { checkoutSha: null, status: "RUNNING", checks };
  const save = () => writeFileSync(resolve(directory, "COMPILE-PREFLIGHT.json"), JSON.stringify(report, null, 2) + "\n");
  save();
  try {
    report.checkoutSha = checkout();
    for (const check of checks) {
      check.status = "RUNNING";
      save();
      try {
        const result = execute(check.program, check.args);
        check.exitCode = result.status ?? null;
        if (result.status !== 0) throw new Error(`${check.id} failed (${result.status ?? result.error})`);
        check.status = "PASS";
      } catch (error) {
        check.status = "FAIL";
        check.error = error.message;
        throw error;
      } finally { save(); }
    }
    report.status = "PASS";
  } catch (error) {
    report.status = "FAIL";
    report.error = error.message;
    throw error;
  } finally { save(); }
}
export function stageBroker(repository, build, preflight = () => {}) {
  const stage = resolve(repository, "src-tauri/target/release-stage");
  const staged = resolve(stage, "localbridge-privileged-broker.exe");
  const evidence = resolve(stage, "broker-build.json");
  rejectExtras(stage, ["localbridge-privileged-broker.exe", "broker-build.json"]);
  mkdirSync(stage, { recursive: true });
  // Tauri checks this resource while compiling the broker itself.
  writeFileSync(evidence, JSON.stringify({ status: "BUILDING" }));
  if (!existsSync(staged)) writeFileSync(staged, Buffer.alloc(0));
  preflight();
  build();
  const binary = resolve(repository, "src-tauri/target/release/localbridge-privileged-broker.exe");
  const bytes = requiredFile(binary);
  copyFileSync(binary, staged);
  const hash = sha256(bytes);
  if (sha256(requiredFile(staged)) !== hash) throw new Error("broker staging mismatch");
  writeFileSync(evidence, JSON.stringify({ status: "PASS", sha256: hash, builtAt: new Date().toISOString() }, null, 2) + "\n");
}
export function stageAdapter(repository, build, preflight = () => {}) {
  const stage = resolve(repository, "src-tauri/target/local-mcp-stage");
  rejectExtras(stage, ["localbridge-mcp.exe", "adapter-build.json"]);
  mkdirSync(stage, { recursive: true });
  const staged = resolve(stage, "localbridge-mcp.exe");
  const evidence = resolve(stage, "adapter-build.json");
  writeFileSync(evidence, JSON.stringify({ status: "BUILDING" }));
  if (!existsSync(staged)) writeFileSync(staged, Buffer.alloc(0));
  preflight();
  build();
  const binary = resolve(repository, "src-tauri/target/release/localbridge-mcp.exe");
  const hash = sha256(requiredFile(binary));
  copyFileSync(binary, staged);
  if (sha256(requiredFile(staged)) !== hash) throw new Error("adapter staging mismatch");
  writeFileSync(evidence, JSON.stringify({ status: "PASS", sha256: hash, builtAt: new Date().toISOString() }, null, 2) + "\n");
}
export function prepareResources({ compile = false } = {}) {
  for (const forbidden of ["runtime/tunnel-client/cloudflared.exe", "runtime/tunnel-client/cloudflared-manifest.json"]) {
    if (existsSync(resolve(root, forbidden))) throw new Error(`forbidden payload: ${forbidden}`);
  }
  for (const path of ["runtime/python/python.exe", "runtime/coding-tools-mcp/coding_tools_mcp/__init__.py", "runtime/tunnel-client/tunnel-client.exe", "runtime-manifest.toml", "runtime-policy.toml", "LICENSE", "THIRD_PARTY_NOTICES.md"]) requiredFile(resolve(root, path));
  if (/cloudflared|cloudflare managed/i.test(readFileSync(resolve(root, "runtime-manifest.toml"), "utf8"))) throw new Error("obsolete runtime manifest");
  run(process.execPath, ["scripts/prepare-toolbox.mjs"]);
  const adapterStage = resolve(root, "src-tauri/target/local-mcp-stage");
  mkdirSync(adapterStage, { recursive: true });
  if (!existsSync(resolve(adapterStage, "localbridge-mcp.exe"))) writeFileSync(resolve(adapterStage, "localbridge-mcp.exe"), Buffer.alloc(0));
  stageBroker(root, () => run("cargo", ["+1.85.0", "build", "--manifest-path", "src-tauri/Cargo.toml", "--target-dir", "src-tauri/target", "--locked", "--release", "--features", "privileged-broker", "--bin", "localbridge-privileged-broker"]), () => { if (compile) compilePreflight(root); });
  stageAdapter(root, () => run("cargo", ["+1.85.0", "build", "--manifest-path", "src-tauri/Cargo.toml", "--target-dir", "src-tauri/target", "--locked", "--release", "--features", "mcp-adapter", "--bin", "localbridge-mcp"]));
  console.log("LB018_RELEASE_RESOURCES=PASS broker=release toolbox=pinned");
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  if (args.some((arg) => arg !== "--compile-preflight") || args.length > 1) throw new Error("unknown resource preparation arguments");
  prepareResources({ compile: args.includes("--compile-preflight") });
}
