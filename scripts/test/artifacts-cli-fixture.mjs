import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { CI_STAGE_IDS } from "./ci-contract.mjs";
import { filesBelow, sha256 } from "./runtime-integrity.mjs";
import { copyReleaseInputs, put, root, stageFixtureExtension } from "./product-release-fixture.mjs";

export function artifactFixture(profile) {
  // Preserve fixtures outside the always-upload diagnostic directory. These
  // binaries are synthetic contract inputs, never Windows build evidence.
  const parent = join(root, "tests/artifacts/artifacts-cli-fixtures");
  mkdirSync(parent, { recursive: true });
  const directory = mkdtempSync(join(parent, `${profile} 中文 cli-`));
  copyReleaseInputs(directory);
  for (const name of [
    ".gitignore", "scripts/release-candidate.mjs", "scripts/test/build-evidence.mjs",
    "scripts/test/ci-contract.mjs", "scripts/test/ci-gate.mjs", "scripts/test/process.mjs",
    "scripts/test/stage-runner.mjs", "runtime-manifest.toml", "runtime-policy.toml",
    "provenance/runtime-lock.json", "provenance/local-mcp-references.json",
    "provenance/tunnel-client.json", "docs/licenses/mcp-proxy-MIT.txt",
  ]) {
    put(directory, name, readFileSync(join(root, name)));
  }
  const inputs = Object.fromEntries(filesBelow(directory).map(name => [name, sha256(readFileSync(join(directory, name)))]));
  const harnessHashes = Object.fromEntries(["scripts/test/artifacts-cli.test.mjs", "scripts/test/artifacts-cli-fixture.mjs", "scripts/test/product-release-fixture.mjs"].map(name => [name, sha256(readFileSync(join(root, name)))]));
  const git = (...args) => execFileSync("git", ["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=.git/hooks", ...args], { cwd: directory, encoding: "utf8", windowsHide: true });
  git("init", "--quiet");
  git("add", "--all");
  git("-c", "user.name=LocalBridge CLI fixture", "-c", "user.email=fixture@example.invalid", "commit", "--quiet", "-m", "Artifact CLI contract inputs");
  const source = git("rev-parse", "HEAD").trim();
  const bundle = stageFixtureExtension(directory, source);
  const installerPath = `src-tauri/target/release/bundle/nsis/LocalBridge_${bundle.applicationVersion}_x64-setup.exe`;
  const installer = put(directory, installerPath, "synthetic NSIS contract bytes; not an executable");
  for (const name of [
    "src-tauri/target/release/localbridge.exe",
    "src-tauri/target/release-stage/localbridge-privileged-broker.exe",
    "src-tauri/target/local-mcp-stage/localbridge-mcp.exe",
    "src-tauri/target/browser-host-stage/localbridge-browser-host.exe",
    "runtime/fixture.exe", "runtime/fixture.txt", "src-tauri/target/toolbox-stage/fixture.dll",
  ]) put(directory, name, "synthetic artifact contract bytes");
  for (const name of ["src-tauri/target/local-mcp-stage/adapter-build.json", "src-tauri/target/release-stage/broker-build.json"]) {
    put(directory, name, JSON.stringify({ status: "PASS", fixture: true, sourceSha: source }));
  }
  put(directory, "src-tauri/target/browser-host-stage/native-host-template.json", JSON.stringify({ fixture: true }));
  const host = { status: "PASS", sourceSha: source, extensionId: bundle.extension.extensionId, protocolVersion: bundle.extension.protocol };
  put(directory, "src-tauri/target/browser-host-stage/browser-host-build.json", JSON.stringify(host));
  const extensionPath = `tests/artifacts/browser-extension/${bundle.extension.asset.name}`;
  put(directory, extensionPath, readFileSync(join(directory, "src-tauri/target/browser-extension-stage/extension.zip")));
  const extension = { status: "PASS", commit: source, version: bundle.extension.version, protocol: bundle.extension.protocol, extensionId: bundle.extension.extensionId, sha256: bundle.extension.asset.sha256 };
  put(directory, "tests/artifacts/browser-extension/extension-build.json", JSON.stringify(extension));
  put(directory, "tests/artifacts/browser-extension/SHA256SUMS.txt", `${extension.sha256}  ${bundle.extension.asset.name}\n`);
  const report = { commit: source, profile, status: "RUNNING", startedAt: new Date(Date.now() - 5_000).toISOString(), stages: CI_STAGE_IDS.map(id => ({ id, status: id === "artifacts" ? "RUNNING" : "PASS" })) };
  put(directory, "tests/artifacts/ci/TEST-REPORT.json", JSON.stringify(report));
  put(directory, "tests/artifacts/ci/toolchains.json", JSON.stringify({ fixture: true, node: process.version }));
  return { directory, profile, source, inputs, harnessHashes, bundle, installer, installerPath, extensionPath, host, report };
}

export function assertFixtureInputs(fixture) {
  for (const [name, hash] of Object.entries(fixture.inputs)) {
    if (sha256(readFileSync(join(fixture.directory, name))) !== hash
      || sha256(readFileSync(join(root, name))) !== hash) throw new Error(`artifact CLI input changed: ${name}`);
  }
  for (const [name, hash] of Object.entries(fixture.harnessHashes)) {
    if (sha256(readFileSync(join(root, name))) !== hash) throw new Error(`artifact CLI harness changed: ${name}`);
  }
}
