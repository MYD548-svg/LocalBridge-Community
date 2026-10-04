import assert from "node:assert/strict";
import test from "node:test";
import { existsSync, readFileSync, readdirSync, renameSync, utimesSync } from "node:fs";
import { join, relative } from "node:path";
import { artifactFixture, assertFixtureInputs } from "./artifacts-cli-fixture.mjs";
import { put, root } from "./product-release-fixture.mjs";
import { sha256 } from "./runtime-integrity.mjs";
import { runLoggedStage } from "./stage-runner.mjs";
import { releaseConfiguration } from "../release-contract.mjs";

const json = (directory, name) => JSON.parse(readFileSync(join(directory, name), "utf8"));
async function cli(fixture, id, entry = "scripts/test/build-evidence.mjs", args = ["artifacts"]) {
  const stage = { id, label: "fresh artifact CLI contract process", program: process.execPath, args: [join(fixture.directory, entry), ...args], cwd: fixture.directory,
    env: { LOCALBRIDGE_BUILD_PROFILE: fixture.profile, VERIFIED_SOURCE_COMMIT: fixture.source }, timeoutMs: 30_000 };
  const options = { root, logDirectory: "tests/artifacts/ci/logs/artifacts-cli", echo: false };
  // No module import in the child before its real CLI entry. Parent imports
  // cannot warm the child's ESM graph or bypass entry-point argument handling.
  let record;
  try { record = await runLoggedStage(stage, options); }
  catch (error) { if (!error.record) throw error; record = error.record; }
  assert.equal(record.timedOut, undefined, "artifact CLI must finish within its deadline");
  assert.equal(record.signal, null);
  assertFixtureInputs(fixture);
  put(root, `${options.logDirectory}/${id}.${record.executionId}.inputs.json`, JSON.stringify({ fixture: relative(root, fixture.directory), source: fixture.source, profile: fixture.profile, hashes: fixture.inputs, harnessHashes: fixture.harnessHashes }, null, 2));
  return { record, stdout: readFileSync(join(root, record.logs.stdout), "utf8"), stderr: readFileSync(join(root, record.logs.stderr), "utf8") };
}
function passed(result) {
  assert.equal(result.record.exitCode, 0, result.stderr);
  assert.equal(result.record.status, "PASS");
  assert.doesNotMatch(result.stderr, /unsettled top-level await/i);
  put(root, result.record.logs.result, JSON.stringify({ ...result.record, contractStatus: "PASS", expectedOutcome: "SUCCESS" }, null, 2));
}
function rejected(result, reason) {
  assert.notEqual(result.record.exitCode, 0);
  assert.equal(result.record.status, "FAIL");
  assert.match(result.stderr, reason);
  assert.doesNotMatch(result.stderr, /unsettled top-level await/i);
  put(root, result.record.logs.result, JSON.stringify({ ...result.record, contractStatus: "PASS", expectedOutcome: "REJECTION" }, null, 2));
}
function checksums(directory, name, expectedNames) {
  const lines = readFileSync(join(directory, name), "utf8").trim().split(/\r?\n/);
  const sums = new Map(lines.map(line => {
    assert.match(line, /^[a-f0-9]{64}  .+$/);
    const hash = line.slice(0, 64), path = line.slice(66);
    assert.equal(sha256(readFileSync(join(directory, path))), hash, path);
    return [path, hash];
  }));
  assert.equal(sums.size, lines.length, "checksum paths must be unique");
  assert.deepEqual([...sums.keys()].sort(), [...expectedNames].sort());
}

for (const profile of ["bundled", "community"]) test(`fresh ${profile} artifacts CLI writes the complete paired candidate`, async () => {
  const fixture = artifactFixture(profile), { directory, source, bundle } = fixture;
  passed(await cli(fixture, `${profile}-artifacts`));
  const provenance = json(directory, "tests/artifacts/ci/BUILD-PROVENANCE.json");
  assert.equal(provenance.commit, source);
  assert.equal(provenance.profile, profile);
  assert.equal(provenance.toolchains.fixture, true);
  assert.ok(Number.isFinite(Date.parse(provenance.generatedAt)));
  const required = [
    fixture.installerPath, "src-tauri/target/release/localbridge.exe", "src-tauri/target/release-stage/localbridge-privileged-broker.exe",
    "src-tauri/target/local-mcp-stage/localbridge-mcp.exe", "src-tauri/target/local-mcp-stage/adapter-build.json",
    "provenance/local-mcp-references.json", "docs/licenses/mcp-proxy-MIT.txt", "runtime-manifest.toml", "runtime-policy.toml", "provenance/runtime-lock.json",
    "src-tauri/target/browser-host-stage/localbridge-browser-host.exe", "src-tauri/target/browser-host-stage/browser-host-build.json",
    "src-tauri/target/browser-host-stage/native-host-template.json", "product-release.json", "src-tauri/target/browser-extension-stage/extension.zip",
    "src-tauri/target/browser-extension-stage/bundle.json", fixture.extensionPath, "tests/artifacts/browser-extension/extension-build.json",
    "tests/artifacts/browser-extension/SHA256SUMS.txt", "extensions/chatgpt-web/INSTALL.html", "extensions/chatgpt-web/INSTALL.svg",
    "provenance/tunnel-client.json", "src-tauri/target/release-stage/broker-build.json", "runtime/fixture.exe", "runtime/fixture.txt", "src-tauri/target/toolbox-stage/fixture.dll",
  ];
  assert.deepEqual(Object.keys(provenance.hashes).sort(), [...required].sort());
  for (const name of required) assert.equal(provenance.hashes[name], sha256(readFileSync(join(directory, name))), name);
  checksums(directory, "tests/artifacts/ci/SHA256SUMS.txt", Object.keys(provenance.hashes));
  const candidate = join(directory, "tests/artifacts/release");
  const manifestAsset = releaseConfiguration(directory).manifestAsset;
  const names = [fixture.installerPath.split("/").at(-1), bundle.extension.asset.name, manifestAsset, "INSTALL.html", "INSTALL.svg"];
  assert.deepEqual(readdirSync(candidate).sort(), [...names, "SHA256SUMS.txt"].sort());
  checksums(candidate, "SHA256SUMS.txt", names);
  const manifest = json(candidate, manifestAsset);
  assert.equal(manifest.sourceCommit, source);
  assert.equal(manifest.repository, "MYD548-svg/LocalBridge-Community");
  assert.equal(manifest.tag, `v${bundle.applicationVersion}-community.1`);
  assert.deepEqual(manifest.extension, bundle.extension);
  assert.equal(manifest.installer.sha256, provenance.hashes[fixture.installerPath]);
  assert.equal(manifest.installer.size, readFileSync(fixture.installer).length);
  for (const [original, copied] of [[fixture.installerPath, manifest.installer.name], [fixture.extensionPath, bundle.extension.asset.name], ["extensions/chatgpt-web/INSTALL.html", "INSTALL.html"], ["extensions/chatgpt-web/INSTALL.svg", "INSTALL.svg"]]) {
    assert.deepEqual(readFileSync(join(directory, original)), readFileSync(join(candidate, copied)));
  }
  // The outer gate writes its final PASS only after this child exits.
  assert.equal(json(directory, "tests/artifacts/ci/TEST-REPORT.json").status, "RUNNING");
  const verify = () => cli(fixture, `${profile}-verify`, "scripts/release-candidate.mjs", ["verify", join(directory, "tests/artifacts")]);
  rejected(await verify(), /complete community gate evidence mismatch/);
  const complete = { ...fixture.report, status: "PASS", finishedAt: new Date().toISOString(), stages: fixture.report.stages.map(stage => ({ ...stage, status: "PASS" })) };
  put(directory, "tests/artifacts/ci/TEST-REPORT.json", JSON.stringify(complete));
  if (profile === "community") {
    const result = await verify(); passed(result);
    assert.match(result.stdout, new RegExp(`RELEASE_CANDIDATE=PASS ${manifest.tag.replaceAll(".", "\\.")} ${source}`));
    for (const stages of [complete.stages.slice(1), [complete.stages[0], ...complete.stages.slice(0, -1)]]) {
      put(directory, "tests/artifacts/ci/TEST-REPORT.json", JSON.stringify({ ...complete, stages }));
      rejected(await verify(), /complete community gate evidence mismatch/);
    }
    put(directory, "tests/artifacts/ci/TEST-REPORT.json", JSON.stringify(complete));
    put(candidate, manifest.extension.asset.name, "substituted ZIP");
    rejected(await verify(), /artifact hash or original provenance mismatch/);
  } else rejected(await verify(), /complete community gate evidence mismatch/);
});

const failures = [
  ["missing-installer", fixture => renameSync(fixture.installer, `${fixture.installer}.preserved`), /Expected exactly one NSIS installer/],
  ["expired-installer", fixture => { const expired = new Date(Date.parse(fixture.report.startedAt) - 10_000); utimesSync(fixture.installer, expired, expired); }, /Installer predates this gate run/],
  ["wrong-source", fixture => put(fixture.directory, "src-tauri/target/browser-host-stage/browser-host-build.json", JSON.stringify({ ...fixture.host, sourceSha: "b".repeat(40) })), /must bind to this checkout/],
  ["wrong-protocol", fixture => put(fixture.directory, "src-tauri/target/browser-host-stage/browser-host-build.json", JSON.stringify({ ...fixture.host, protocolVersion: fixture.host.protocolVersion + 1 })), /protocol or identity mismatch/],
  ["wrong-identity", fixture => put(fixture.directory, "src-tauri/target/browser-host-stage/browser-host-build.json", JSON.stringify({ ...fixture.host, extensionId: "a".repeat(32) })), /protocol or identity mismatch/],
  ["wrong-zip", fixture => put(fixture.directory, fixture.extensionPath, "substituted ZIP"), /extension ZIP hash mismatch/],
];
for (const [id, change, reason] of failures) test(`fresh artifacts CLI rejects ${id}`, async () => {
  const fixture = artifactFixture("community"); change(fixture);
  rejected(await cli(fixture, id), reason);
  assert.equal(existsSync(join(fixture.directory, "tests/artifacts/release")), false);
});

test("fresh evidence CLI rejects unknown operations and extra arguments", async () => {
  const fixture = artifactFixture("bundled");
  for (const args of [[], ["unknown"], ["toString"], ["artifacts", "extra"]]) {
    rejected(await cli(fixture, "invalid-arguments", undefined, args), /Unknown evidence operation or arguments/);
  }
  assert.equal(existsSync(join(fixture.directory, "tests/artifacts/release")), false);
});
