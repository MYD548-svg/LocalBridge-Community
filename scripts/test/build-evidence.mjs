import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { filesBelow, requiredFile, sha256 } from "./runtime-integrity.mjs";
export const root = resolve(import.meta.dirname, "../..");
export const evidenceRoot = join(root, "tests/artifacts/ci");
export function command(program, args) {
  const result = spawnSync(program, args, { cwd: root, encoding: "utf8", windowsHide: true });
  if (result.status !== 0) throw new Error(`${program} ${args.join(" ")}: ${result.error ?? result.stderr}`);
  return result.stdout.trim();
}
export function toolchains() {
  const versions = { node: process.version, git: command("git", ["--version"]), rust: command("rustc", ["+1.85.0", "--version"]), cargo: command("cargo", ["+1.85.0", "--version"]) };
  if (Number(process.versions.node.split(".")[0]) < 22) throw new Error("Node 22+ required");
  if (process.env.LOCALBRIDGE_BUILD_PROFILE === "community") versions.go = command("go", ["version"]);
  mkdirSync(evidenceRoot, { recursive: true });
  writeFileSync(join(evidenceRoot, "toolchains.json"), JSON.stringify(versions, null, 2));
  console.log(versions);
}
export function tunnelSource() {
  if (process.env.LOCALBRIDGE_BUILD_PROFILE !== "community") {
    console.log("TUNNEL_SOURCE=bundled; source build not requested");
    return;
  }
  const result = spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/build-tunnel-client.ps1", "-UpdateBundleRs", "-SkipTests"], { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.status !== 0) throw new Error(`Tunnel source build failed: ${result.status ?? result.error}`);
}
export function artifacts() {
  const bundleRoot = join(root, "src-tauri/target/release/bundle/nsis");
  const installers = filesBelow(bundleRoot).filter((name) => name.endsWith("-setup.exe"));
  if (installers.length !== 1) throw new Error("Expected exactly one NSIS installer; remove stale artifacts manually");
  const report = JSON.parse(requiredFile(join(evidenceRoot, "TEST-REPORT.json")));
  if (report.stages.find((stage) => stage.id === "nsis-package")?.status !== "PASS") throw new Error("No successful package stage in this run");
  if (statSync(join(bundleRoot, installers[0])).mtimeMs < Date.parse(report.startedAt)) throw new Error("Installer predates this gate run");
  const sourceSha = command("git", ["rev-parse", "HEAD"]);
  const hostEvidence = JSON.parse(requiredFile(join(root, "src-tauri/target/browser-host-stage/browser-host-build.json")));
  const extensionEvidence = JSON.parse(requiredFile(join(root, "tests/artifacts/browser-extension/extension-build.json")));
  if (hostEvidence.status !== "PASS" || hostEvidence.sourceSha !== sourceSha || extensionEvidence.status !== "PASS" || extensionEvidence.commit !== sourceSha) throw new Error("browser artifacts must bind to this checkout");
  if (hostEvidence.extensionId !== extensionEvidence.extensionId || hostEvidence.protocolVersion !== extensionEvidence.protocol) throw new Error("browser artifact protocol or identity mismatch");
  const extension = "tests/artifacts/browser-extension/LocalBridge-ChatGPT-Web-v" + extensionEvidence.version + ".zip";
  if (sha256(requiredFile(join(root, extension))) !== extensionEvidence.sha256) throw new Error("extension ZIP hash mismatch");
  const paths = [
    `src-tauri/target/release/bundle/nsis/${installers[0]}`,
    "src-tauri/target/release/localbridge.exe",
    "src-tauri/target/release-stage/localbridge-privileged-broker.exe",
    "src-tauri/target/local-mcp-stage/localbridge-mcp.exe", "src-tauri/target/local-mcp-stage/adapter-build.json", "provenance/local-mcp-references.json", "docs/licenses/mcp-proxy-MIT.txt",
    "runtime-manifest.toml", "runtime-policy.toml", "provenance/runtime-lock.json",
    "src-tauri/target/browser-host-stage/localbridge-browser-host.exe",
    "src-tauri/target/browser-host-stage/browser-host-build.json",
    "src-tauri/target/browser-host-stage/native-host-template.json",
    extension, "tests/artifacts/browser-extension/extension-build.json", "tests/artifacts/browser-extension/SHA256SUMS.txt",
    "extensions/chatgpt-web/INSTALL.html", "extensions/chatgpt-web/INSTALL.svg",
    "provenance/tunnel-client.json", "src-tauri/target/release-stage/broker-build.json",
    ...filesBelow(join(root, "runtime")).map((name) => `runtime/${name}`),
    ...filesBelow(join(root, "src-tauri/target/toolbox-stage")).map((name) => `src-tauri/target/toolbox-stage/${name}`),
  ];
  for (const path of paths.filter((name) => !name.startsWith("runtime/") || /\.(exe|dll)$/i.test(name))) requiredFile(join(root, path));
  const hashes = Object.fromEntries(paths.map((name) => [name, sha256(readFileSync(join(root, name)))]));
  mkdirSync(evidenceRoot, { recursive: true });
  writeFileSync(join(evidenceRoot, "SHA256SUMS.txt"), Object.entries(hashes).map(([name, hash]) => `${hash}  ${name}`).join("\n") + "\n");
  writeFileSync(join(evidenceRoot, "BUILD-PROVENANCE.json"), JSON.stringify({ commit: command("git", ["rev-parse", "HEAD"]), dirty: Boolean(command("git", ["status", "--porcelain"])), profile: process.env.LOCALBRIDGE_BUILD_PROFILE ?? "bundled", generatedAt: new Date().toISOString(), toolchains: JSON.parse(readFileSync(join(evidenceRoot, "toolchains.json"), "utf8")), hashes }, null, 2));
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const actions = { toolchains, "tunnel-source": tunnelSource, artifacts };
  const action = actions[process.argv[2]];
  if (!action) throw new Error("Unknown evidence operation");
  action();
}
