import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { rejectExtras, requiredFile, sha256 } from "./test/runtime-integrity.mjs";
const root = resolve(import.meta.dirname, "..");
function run(program, args, repository = root) {
  const result = spawnSync(program, args, { cwd: repository, stdio: "inherit", windowsHide: true });
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
    ["browser-compile", ["test", ...common, "--features", "browser-host", "--all-targets", "--no-run"]],
    ["browser-clippy", ["clippy", ...common, "--features", "browser-host", "--all-targets", "--", "-D", "warnings"]],
    ["browser-dev-compile", ["test", ...common, "--features", "browser-host-dev", "--bin", "localbridge-browser-host", "--no-run"]],
    ["browser-dev-clippy", ["clippy", ...common, "--features", "browser-host-dev", "--bin", "localbridge-browser-host", "--", "-D", "warnings"]],
  ].map(([id, args]) => ({ id, program: "cargo", args: ["+1.85.0", ...args], status: "NOT_RUN", exitCode: null }));
  const directory = resolve(repository, "tests/artifacts/ci");
  mkdirSync(directory, { recursive: true });
  const report = { checkoutSha: null, status: "RUNNING", checks };
  const save = () => writeFileSync(resolve(directory, "COMPILE-PREFLIGHT.json"), JSON.stringify(report, null, 2) + "\n");
  save();
  try {
    report.checkoutSha = checkout();
    if (!/^[a-f0-9]{40}$/.test(report.checkoutSha ?? "")) throw new Error("invalid compile-preflight source SHA");
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
      } finally { save(); }
    }
    const failures = checks.filter((check) => check.status === "FAIL");
    if (failures.length) throw new Error(`compile preflight failed: ${failures.map((check) => check.id).join(", ")}`);
    report.status = "PASS";
  } catch (error) {
    report.status = "FAIL";
    report.error = error.message;
    throw error;
  } finally { save(); }
}
const nativeStages = [
  { directory: "release-stage", binary: "localbridge-privileged-broker.exe", evidence: "broker-build.json" },
  { directory: "local-mcp-stage", binary: "localbridge-mcp.exe", evidence: "adapter-build.json" },
  { directory: "browser-host-stage", binary: "localbridge-browser-host.exe", evidence: "browser-host-build.json" },
];
function saveEvidence(repository, spec, value) {
  const stage = resolve(repository, "src-tauri/target", spec.directory);
  mkdirSync(stage, { recursive: true });
  writeFileSync(resolve(stage, spec.evidence), JSON.stringify(value, null, 2) + "\n");
}
function stageExecutable(repository, spec, action) {
  // Invalidate before checking extras or identity, not only before compilation.
  saveEvidence(repository, spec, { status: "BUILDING" });
  const stage = resolve(repository, "src-tauri/target", spec.directory);
  const staged = resolve(stage, spec.binary);
  try {
    rejectExtras(stage, [spec.binary, spec.evidence, ...(spec.directory === "browser-host-stage" ? ["native-host-template.json"] : [])]);
    // Tauri checks these resources while compiling the same crate.
    if (!existsSync(staged)) writeFileSync(staged, Buffer.alloc(0));
    const details = action(stage);
    const binary = resolve(repository, "src-tauri/target/release", spec.binary);
    const hash = sha256(requiredFile(binary));
    copyFileSync(binary, staged);
    if (sha256(requiredFile(staged)) !== hash) throw new Error(`${spec.binary} staging mismatch`);
    saveEvidence(repository, spec, { status: "PASS", sha256: hash, builtAt: new Date().toISOString(), ...details });
  } catch (error) {
    saveEvidence(repository, spec, { status: "FAIL", error: error.message });
    throw error;
  }
}
export function stageBroker(repository, build, preflight = () => {}) {
  stageExecutable(repository, nativeStages[0], () => { preflight(); build(); });
}
export function stageAdapter(repository, build, preflight = () => {}) {
  stageExecutable(repository, nativeStages[1], () => { preflight(); build(); });
}
export function stageBrowserHost(repository, build, checkout = () => {
  const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repository, encoding: "utf8", windowsHide: true });
  if (git.status !== 0) throw new Error("cannot identify browser host checkout");
  return git.stdout.trim();
}) {
  stageExecutable(repository, nativeStages[2], (stage) => {
    const identity = JSON.parse(requiredFile(resolve(repository, "extensions/chatgpt-web/identity.json")));
    const template = { name: identity.host, description: "LocalBridge ChatGPT web gateway", path: "localbridge-browser-host.exe", type: "stdio", allowed_origins: ["chrome-extension://" + identity.id + "/"] };
    writeFileSync(resolve(stage, "native-host-template.json"), JSON.stringify(template, null, 2) + "\n");
    const sourceSha = checkout();
    if (!/^[a-f0-9]{40}$/.test(sourceSha ?? "")) throw new Error("invalid browser host source SHA");
    build();
    return { sourceSha, templateSha256: sha256(requiredFile(resolve(stage, "native-host-template.json"))), protocolVersion: identity.protocol, extensionId: identity.id };
  });
}
export function prepareResources({ compile = false, repository = root } = {}) {
  // A failed attempt must not leave another component's previous PASS usable.
  for (const spec of nativeStages) saveEvidence(repository, spec, { status: "BUILDING" });
  try {
    for (const forbidden of ["runtime/tunnel-client/cloudflared.exe", "runtime/tunnel-client/cloudflared-manifest.json"]) {
      if (existsSync(resolve(repository, forbidden))) throw new Error(`forbidden payload: ${forbidden}`);
    }
    for (const path of ["runtime/python/python.exe", "runtime/coding-tools-mcp/coding_tools_mcp/__init__.py", "runtime/tunnel-client/tunnel-client.exe", "runtime-manifest.toml", "runtime-policy.toml", "LICENSE", "THIRD_PARTY_NOTICES.md"]) requiredFile(resolve(repository, path));
    if (/cloudflared|cloudflare managed/i.test(readFileSync(resolve(repository, "runtime-manifest.toml"), "utf8"))) throw new Error("obsolete runtime manifest");
    run(process.execPath, ["scripts/prepare-toolbox.mjs"], repository);
    const adapterStage = resolve(repository, "src-tauri/target/local-mcp-stage");
    mkdirSync(adapterStage, { recursive: true });
    if (!existsSync(resolve(adapterStage, "localbridge-mcp.exe"))) writeFileSync(resolve(adapterStage, "localbridge-mcp.exe"), Buffer.alloc(0));
    const hostStage = resolve(repository, "src-tauri/target/browser-host-stage");
    mkdirSync(hostStage, { recursive: true });
    if (!existsSync(resolve(hostStage, "localbridge-browser-host.exe"))) writeFileSync(resolve(hostStage, "localbridge-browser-host.exe"), Buffer.alloc(0));
    // The manifest is a resource of the same crate that is compiling the host.
    const identity = JSON.parse(requiredFile(resolve(repository, "extensions/chatgpt-web/identity.json")));
    writeFileSync(resolve(hostStage, "native-host-template.json"), JSON.stringify({ name: identity.host, description: "LocalBridge ChatGPT web gateway", path: "localbridge-browser-host.exe", type: "stdio", allowed_origins: ["chrome-extension://" + identity.id + "/"] }, null, 2) + "\n");
    writeFileSync(resolve(hostStage, "browser-host-build.json"), JSON.stringify({ status: "BUILDING" }));
    stageBroker(repository, () => run("cargo", ["+1.85.0", "build", "--manifest-path", "src-tauri/Cargo.toml", "--target-dir", "src-tauri/target", "--locked", "--release", "--features", "privileged-broker", "--bin", "localbridge-privileged-broker"], repository), () => { if (compile) compilePreflight(repository); });
    stageAdapter(repository, () => run("cargo", ["+1.85.0", "build", "--manifest-path", "src-tauri/Cargo.toml", "--target-dir", "src-tauri/target", "--locked", "--release", "--features", "mcp-adapter", "--bin", "localbridge-mcp"], repository));
    stageBrowserHost(repository, () => run("cargo", ["+1.85.0", "build", "--manifest-path", "src-tauri/Cargo.toml", "--target-dir", "src-tauri/target", "--locked", "--release", "--features", "browser-host", "--bin", "localbridge-browser-host"], repository));
    console.log("LB018_RELEASE_RESOURCES=PASS broker=release toolbox=pinned");
  } catch (error) {
    for (const spec of nativeStages) saveEvidence(repository, spec, { status: "FAIL", error: error.message });
    throw error;
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  if (args.some((arg) => arg !== "--compile-preflight") || args.length > 1) throw new Error("unknown resource preparation arguments");
  prepareResources({ compile: args.includes("--compile-preflight") });
}
