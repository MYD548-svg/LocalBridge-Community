import assert from "node:assert/strict";
import test from "node:test";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applicationVersion, releaseConfiguration, verifyBundledExtension } from "../release-contract.mjs";
import { stageReleaseCandidate, verifyReleaseCandidate } from "../release-candidate.mjs";
import { CI_STAGE_IDS } from "./ci-contract.mjs";
import { copyReleaseInputs, put, stageFixtureExtension } from "./product-release-fixture.mjs";

function fixture() { const directory = mkdtempSync(join(tmpdir(), "localbridge-product-release-")); copyReleaseInputs(directory); return directory; }
test("product versions cannot diverge or use an illegal browser version", () => {
  const directory = fixture();
  assert.equal(releaseConfiguration(directory).repository, "MYD548-svg/LocalBridge-Community");
  assert.equal(applicationVersion(directory), "0.1.5");
  const config = JSON.parse(readFileSync(join(directory, "src-tauri/tauri.conf.json")));
  put(directory, "src-tauri/tauri.conf.json", JSON.stringify({ ...config, version: "0.1.6" }));
  assert.throws(() => applicationVersion(directory), /version/);
  put(directory, "package.json", JSON.stringify({ version: "0.1.5-community.1" }));
  assert.throws(() => applicationVersion(directory), /version/);
});
test("a missing, failed, tampered or development bundle cannot pass", () => {
  const directory = fixture();
  assert.throws(() => verifyBundledExtension(directory));
  const bundle = stageFixtureExtension(directory);
  assert.equal(verifyBundledExtension(directory).sourceCommit, bundle.sourceCommit);
  put(directory, "src-tauri/target/browser-extension-stage/extension.zip", "tampered");
  assert.throws(() => verifyBundledExtension(directory), /corrupt/);
  stageFixtureExtension(directory);
  for (const change of [{ status: "BUILDING" }, { status: "FAIL" }, { repository: "zephyr7030/LocalBridge" }, { extension: { ...bundle.extension, extensionId: "a".repeat(32) } }]) {
    put(directory, "src-tauri/target/browser-extension-stage/bundle.json", JSON.stringify({ ...bundle, ...change }));
    assert.throws(() => verifyBundledExtension(directory), /incompatible/);
  }
});
test("publication reuses a complete matching pair and rejects partial evidence and substitutions", () => {
  const directory = fixture(), source = "a".repeat(40);
  const bundle = stageFixtureExtension(directory, source);
  const host = { status: "PASS", sourceSha: source, extensionId: bundle.extension.extensionId, protocolVersion: bundle.extension.protocol };
  put(directory, "src-tauri/target/browser-host-stage/browser-host-build.json", JSON.stringify(host));
  const installer = put(directory, "src-tauri/target/release/bundle/nsis/LocalBridge_0.1.5_x64-setup.exe", "fixture NSIS");
  assert.throws(() => stageReleaseCandidate(directory, "b".repeat(40), installer), /share/);
  const manifest = stageReleaseCandidate(directory, source, installer);
  const ci = join(directory, "tests/artifacts/ci"); mkdirSync(ci, { recursive: true });
  const report = { status: "PASS", profile: "community", commit: source, stages: CI_STAGE_IDS.map(id => ({ id, status: "PASS" })) };
  const provenance = { commit: source, profile: "community", hashes: {
    [`src-tauri/target/release/bundle/nsis/${manifest.installer.name}`]: manifest.installer.sha256,
    [`tests/artifacts/browser-extension/${manifest.extension.asset.name}`]: manifest.extension.asset.sha256,
  } };
  put(ci, "TEST-REPORT.json", JSON.stringify(report)); put(ci, "BUILD-PROVENANCE.json", JSON.stringify(provenance));
  const artifacts = join(directory, "tests/artifacts");
  assert.equal(verifyReleaseCandidate(artifacts, directory, source).tag, "v0.1.5-community.1");
  put(ci, "TEST-REPORT.json", JSON.stringify({ ...report, stages: report.stages.slice(1) }));
  assert.throws(() => verifyReleaseCandidate(artifacts, directory, source), /gate/);
  put(ci, "TEST-REPORT.json", JSON.stringify(report));
  put(artifacts, `release/${manifest.extension.asset.name}`, "wrong ZIP");
  assert.throws(() => verifyReleaseCandidate(artifacts, directory, source), /hash/);
  copyFileSync(join(directory, "src-tauri/target/browser-extension-stage/extension.zip"), join(artifacts, "release", manifest.extension.asset.name));
  assert.throws(() => verifyReleaseCandidate(artifacts, directory, "b".repeat(40)), /source/);
});
