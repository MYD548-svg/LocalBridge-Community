import assert from "node:assert/strict";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { stageBroker, stageAdapter, stageBrowserHost, compilePreflight, prepareResources } from "../prepare-lb018-resources.mjs";
import { updateManifest } from "./update-tunnel.mjs";
import { verifyRuntime, verifyHash, sha256, rejectExtras, installerEntryPaths, rejectDuplicateInstallerEntries } from "./runtime-integrity.mjs";
import { runStages } from "./process.mjs";
import { validateCheckout } from "./source-check.mjs";

import { copyReleaseInputs, stageFixtureExtension } from "./product-release-fixture.mjs";
const root = resolve(import.meta.dirname, "../..");
// Fixtures are retained: this repository forbids automatic bulk deletion.
// Runners expose %TEMP% in 8.3 short form; canonicalize so fixture paths and
// the validator's canonical output are lexically comparable.
function fixture() { return realpathSync.native(mkdtempSync(join(tmpdir(), "localbridge-build-regression-"))); }
function put(root, name, content) { const path = join(root, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, content); return path; }

test("Tunnel hash update preserves every other manifest section", () => {
  const original = readFileSync(join(root, "runtime-manifest.toml"), "utf8");
  const updated = updateManifest(original, "a".repeat(64));
  const withoutTunnel = (text) => text.replace(/\[tunnel_client\][\s\S]*?(?=\n\[)/, "");
  assert.equal(withoutTunnel(updated), withoutTunnel(original));
  assert.equal((updated.match(new RegExp('"' + "a".repeat(64) + '"', "g")) ?? []).length, 1);
  assert.throws(() => updateManifest(original.replace("[tunnel_client]", "[wrong]"), "a".repeat(64)));
});

test("source verification uses only the exact supplied checkout and rejects drift", () => {
  const directory = fixture(), exact = join(directory, "tunnel-client-pinned"), older = join(directory, "tunnel-client-older");
  mkdirSync(exact); mkdirSync(older);
  const commit = "a".repeat(40);
  function fakeGit(head = commit, dirty = "", reportedRoot = exact) {
    return (args) => {
      assert.equal(args[1], exact);
      if (args.includes("--show-toplevel")) return reportedRoot;
      if (args.includes("HEAD")) return head;
      return dirty;
    };
  }
  assert.equal(validateCheckout(exact, commit, fakeGit()), exact);
  assert.throws(() => validateCheckout(exact, commit, fakeGit("b".repeat(40))), /commit mismatch/);
  assert.throws(() => validateCheckout(exact, commit, fakeGit(commit, " M go.mod")), /clean/);
  assert.throws(() => validateCheckout(exact, commit, fakeGit(commit, "", directory)), /exact source/);
});

test("source verification accepts the 8.3 short form of the same checkout", () => {
  const directory = fixture(), exact = join(directory, "tunnel-client-pinned");
  mkdirSync(exact);
  // GitHub runners expose %TEMP% as C:\Users\RUNNER~1\...; git reports the
  // canonical long path, so the validator must canonicalize both sides.
  const probe = spawnSync("cmd.exe", ["/d", "/s", "/c", `for %I in ("${exact}") do @echo %~sI`], { encoding: "utf8", windowsVerbatimArguments: true });
  const short = (probe.stdout ?? "").trim().split(/\r?\n/).pop();
  if (probe.status !== 0 || !short || !/~\d/.test(short) || short === exact) return; // 8.3 disabled on this volume
  const commit = "a".repeat(40);
  const fakeGit = (args) => {
    if (args.includes("--show-toplevel")) return exact;
    if (args.includes("HEAD")) return commit;
    return "";
  };
  assert.equal(validateCheckout(short, commit, fakeGit), exact);
  assert.throws(() => validateCheckout(short, "b".repeat(40), fakeGit), /commit mismatch/);
});

test("broker bootstrap, repeated build and failure invalidate old success", () => {
  const directory = fixture();
  const binary = "src-tauri/target/release/localbridge-privileged-broker.exe";
  const evidence = "src-tauri/target/release-stage/broker-build.json";
  for (const content of ["first-build", "second-build"]) {
    stageBroker(directory, () => put(directory, binary, content));
    assert.equal(JSON.parse(readFileSync(join(directory, evidence))).sha256, sha256(content));
  }
  assert.throws(() => stageBroker(directory, () => { throw new Error("compiler failed"); }), /compiler failed/);
  assert.equal(JSON.parse(readFileSync(join(directory, evidence))).status, "FAIL");
  const empty = fixture();
  assert.throws(() => stageBroker(empty, () => put(empty, binary, "")), /empty/);
});

test("required hash checks reject missing, empty and corrupt files", () => {
  const directory = fixture(), path = join(directory, "payload.exe"), hash = sha256("expected");
  assert.throws(() => verifyHash(path, hash), /missing/);
  writeFileSync(path, ""); assert.throws(() => verifyHash(path, hash), /empty/);
  writeFileSync(path, "corrupt"); assert.throws(() => verifyHash(path, hash), /mismatch/);
  writeFileSync(path, "expected"); verifyHash(path, hash);
  assert.throws(() => rejectExtras(directory, []), /remove manually/);
});

test("installer payload census rejects duplicate entries that would overwrite attested files", () => {
  const listing = [
    "Path = D:/run/src-tauri/target/release/bundle/nsis/LocalBridge_0.1.5_x64-setup.exe",
    "Path = localbridge.exe",
    "Path = localbridge-privileged-broker.exe",
    "Path = runtime/python/python.exe",
    "Path = localbridge-privileged-broker.exe",
  ].join("\r\n") + "\r\n";
  const paths = installerEntryPaths(listing);
  assert.deepEqual(paths, ["localbridge.exe", "localbridge-privileged-broker.exe", "runtime/python/python.exe", "localbridge-privileged-broker.exe"]);
  assert.throws(() => rejectDuplicateInstallerEntries(paths), /localbridge-privileged-broker\.exe more than once/);
  assert.doesNotThrow(() => rejectDuplicateInstallerEntries(installerEntryPaths("Path = archive\r\nPath = a\r\nPath = B\r\n")));
});

test("complete bundled trees pass; changed runtime source fails", () => {
  assert.equal(verifyRuntime(root, { bundledOnly: true }).status, "PASS");
  const directory = fixture();
  for (const path of ["runtime", "extensions/chatgpt-web/identity.json", "runtime-manifest.toml", "provenance/runtime-lock.json", "src-tauri/src/tunnel/bundle.rs", "src-tauri/src/mcp/bundle.rs", "src-tauri/tauri.conf.json", "docs/licenses/mcp-proxy-MIT.txt"]) {
    mkdirSync(dirname(join(directory, path)), { recursive: true });
    cpSync(join(root, path), join(directory, path), { recursive: true });
  }
  copyReleaseInputs(directory);
  stageFixtureExtension(directory);
  assert.throws(() => verifyRuntime(directory), /missing/); // no staged toolbox
  const lock = JSON.parse(readFileSync(join(directory, "provenance/runtime-lock.json"), "utf8"));
  let manifest = readFileSync(join(directory, "runtime-manifest.toml"), "utf8");
  for (const component of lock.components.filter((item) => item.runtime_destination)) {
    const bytes = `fixture-${component.name}`;
    manifest = manifest.replaceAll(component.executable_sha256, sha256(bytes));
    component.executable_sha256 = sha256(bytes);
    put(directory, component.runtime_destination.replace("runtime/toolbox/", "src-tauri/target/toolbox-stage/"), bytes);
  }
  put(directory, "runtime-manifest.toml", manifest);
  put(directory, "provenance/runtime-lock.json", JSON.stringify(lock));
  put(directory, "src-tauri/target/toolbox-stage/bin/curl.cmd", '@echo off\r\n"%SystemRoot%\\System32\\curl.exe" %*\r\n');
  stageBroker(directory, () => put(directory, "src-tauri/target/release/localbridge-privileged-broker.exe", "fixture-broker"));
  stageAdapter(directory, () => put(directory, "src-tauri/target/release/localbridge-mcp.exe", "fixture-adapter"));
  stageBrowserHost(directory, () => put(directory, "src-tauri/target/release/localbridge-browser-host.exe", "fixture-host"), () => "a".repeat(40));
  assert.equal(verifyRuntime(directory).status, "PASS");
  const configPath = "src-tauri/tauri.conf.json";
  const config = readFileSync(join(directory, configPath));
  put(directory, configPath, JSON.stringify({ ...JSON.parse(config), bundle: { ...JSON.parse(config).bundle, resources: {} } }));
  assert.throws(() => verifyRuntime(directory), /attested staged broker/);
  put(directory, configPath, config);
  const stagedBrokerPath = "src-tauri/target/release-stage/localbridge-privileged-broker.exe";
  const evidencePath = "src-tauri/target/release-stage/broker-build.json";
  const stagedBroker = readFileSync(join(directory, stagedBrokerPath));
  const evidenceJson = readFileSync(join(directory, evidencePath));
  put(directory, stagedBrokerPath, "tampered-staged-broker");
  assert.throws(() => verifyRuntime(directory), /SHA256 mismatch/);
  put(directory, stagedBrokerPath, stagedBroker);
  put(directory, evidencePath, JSON.stringify({ ...JSON.parse(evidenceJson), status: "BUILDING" }));
  assert.throws(() => verifyRuntime(directory), /broker build incomplete/);
  put(directory, evidencePath, "");
  assert.throws(() => verifyRuntime(directory), /missing/);
  put(directory, evidencePath, evidenceJson);
  assert.equal(verifyRuntime(directory).status, "PASS");
  for (const path of ["src-tauri/target/browser-host-stage/localbridge-browser-host.exe", "src-tauri/target/browser-host-stage/native-host-template.json", "src-tauri/target/browser-extension-stage/extension.zip"]) {
    const original = readFileSync(join(directory, path));
    put(directory, path, "tampered-browser-payload");
    assert.throws(() => verifyRuntime(directory), /SHA256 mismatch|bundled extension/);
    put(directory, path, original);
  }
  const aria = "src-tauri/target/toolbox-stage/bin/aria2c.exe";
  put(directory, aria, "corrupt");
  assert.throws(() => verifyRuntime(directory), /mismatch/);
  put(directory, aria, "fixture-toolbox-aria2c");
  const python = lock.components.find((item) => item.name === "python-embedded");
  python.artifacts["missing-required.dll"] = "a".repeat(64);
  put(directory, "provenance/runtime-lock.json", JSON.stringify(lock));
  assert.throws(() => verifyRuntime(directory), /missing-required.dll/);
  delete python.artifacts["missing-required.dll"];
  put(directory, "provenance/runtime-lock.json", JSON.stringify(lock));
  put(directory, "src-tauri/target/toolbox-stage/bin/unregistered.exe", "extra");
  assert.throws(() => verifyRuntime(directory), /remove manually/);
  const metadataPath = "runtime/python/runtime-metadata.json";
  const metadata = readFileSync(join(directory, metadataPath));
  put(directory, metadataPath, JSON.stringify({ ...JSON.parse(metadata), isolated: false }));
  assert.throws(() => verifyRuntime(directory, { bundledOnly: true }), /metadata mismatch/);
  put(directory, metadataPath, metadata);
  put(directory, "runtime/coding-tools-mcp/coding_tools_mcp/server.py", "tampered");
  assert.throws(() => verifyRuntime(directory, { bundledOnly: true }), /tree mismatch/);
});

test("failed stage prevents later stages from executing", () => {
  const marker = join(fixture(), "must-not-exist");
  assert.throws(() => runStages([
    { id: "failure", label: "synthetic failure", program: process.execPath, args: ["-e", "process.exit(7)"] },
    { id: "later", label: "must not run", program: process.execPath, args: ["-e", `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'unexpected')`] },
  ]), /exit 7/);
  assert.throws(() => readFileSync(marker), /ENOENT/);
});

// No real compiler is spawned: failures must invalidate existing broker evidence
// and prevent release builds, including when a previous attempt passed.
for (const failure of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, "multiple", "spawn", "release", null]) {
  test(`compile preflight and broker staging order: ${failure ?? "success"}`, () => {
    const directory = fixture();
    const evidence = "src-tauri/target/release-stage/broker-build.json";
    const binary = "src-tauri/target/release/localbridge-privileged-broker.exe";
    const reportPath = "tests/artifacts/ci/COMPILE-PREFLIGHT.json";
    stageBroker(directory, () => put(directory, binary, "old-broker"));
    put(directory, reportPath, JSON.stringify({ status: "PASS", checkoutSha: "old" }));
    const calls = [];
    const compile = () => compilePreflight(directory, (program, args) => {
      assert.equal(JSON.parse(readFileSync(join(directory, evidence))).status, "BUILDING");
      assert.equal(program, "cargo");
      assert.equal(args[0], "+1.85.0");
      assert.equal(args[args.indexOf("--manifest-path") + 1], "src-tauri/Cargo.toml");
      assert.equal(args[args.indexOf("--target-dir") + 1], "src-tauri/target");
      assert.ok(args.includes("--locked"));
      if (args[1] === "clippy") assert.deepEqual(args.slice(-3), ["--", "-D", "warnings"]);
      const index = calls.length;
      if (index < 2) assert.ok(!args.includes("--features"));
      else assert.equal(args[args.indexOf("--features") + 1], index < 4 ? "privileged-broker" : index < 6 ? "mcp-adapter" : index < 8 ? "browser-host" : "browser-host-dev");
      if (index >= 8) assert.equal(args[args.indexOf("--bin") + 1], "localbridge-browser-host");
      calls.push(args[1]);
      if (failure === "spawn" && index === 2) throw new Error("fixture process launch failed");
      if (failure === "spawn" && index === 7) return { status: null, error: new Error("fixture executable missing") };
      return { status: failure === index || (failure === "multiple" && [1, 5, 9].includes(index)) ? 17 : 0 };
    }, () => "a".repeat(40));
    const build = () => {
      calls.push("release");
      if (failure === "release") throw new Error("release build failed");
      put(directory, binary, "new-broker");
    };
    const action = () => stageBroker(directory, build, compile);
    if (failure === null) action();
    else assert.throws(action, /failed/);
    const report = JSON.parse(readFileSync(join(directory, reportPath)));
    assert.equal(report.checkoutSha, "a".repeat(40));
    const expectedCalls = ["test", "clippy", "test", "clippy", "test", "clippy", "test", "clippy", "test", "clippy"];
    if (Number.isInteger(failure) || failure === "multiple" || failure === "spawn") {
      assert.equal(report.status, "FAIL");
      assert.deepEqual(calls, expectedCalls);
      const failedIndices = Number.isInteger(failure) ? [failure] : failure === "multiple" ? [1, 5, 9] : [2, 7];
      for (const index of failedIndices) {
        assert.equal(report.checks[index].exitCode, failure === "spawn" ? null : 17);
        assert.ok(report.error.includes(report.checks[index].id));
      }
      assert.deepEqual(report.checks.map((check) => check.status),
        report.checks.map((_, index) => failedIndices.includes(index) ? "FAIL" : "PASS"));
    } else {
      assert.equal(report.status, "PASS");
      assert.deepEqual(calls, [...expectedCalls, "release"]);
    }
    assert.equal(JSON.parse(readFileSync(join(directory, evidence))).status, failure === null ? "PASS" : "FAIL");
  });
}

test("adapter staging invalidates stale evidence and rejects installer hash drift", () => {
  const directory = fixture();
  const binary = "src-tauri/target/release/localbridge-mcp.exe";
  const staged = "src-tauri/target/local-mcp-stage/localbridge-mcp.exe";
  const evidence = "src-tauri/target/local-mcp-stage/adapter-build.json";
  stageAdapter(directory, () => put(directory, binary, "first-adapter"));
  const initial = JSON.parse(readFileSync(join(directory, evidence)));
  verifyHash(join(directory, staged), initial.sha256);
  put(directory, staged, "tampered-adapter");
  assert.throws(() => verifyHash(join(directory, staged), initial.sha256), /mismatch/);
  assert.throws(() => stageAdapter(directory, () => { throw new Error("adapter compiler failed"); }), /compiler failed/);
  assert.equal(JSON.parse(readFileSync(join(directory, evidence))).status, "FAIL");
  stageAdapter(directory, () => put(directory, binary, "second-adapter"));
  assert.equal(JSON.parse(readFileSync(join(directory, evidence))).sha256, sha256("second-adapter"));
  put(directory, "src-tauri/target/local-mcp-stage/unregistered.exe", "extra");
  assert.throws(() => stageAdapter(directory, () => {}), /remove manually/);
  assert.equal(JSON.parse(readFileSync(join(directory, evidence))).status, "FAIL");
  assert.equal(readFileSync(join(directory, "src-tauri/target/local-mcp-stage/unregistered.exe"), "utf8"), "extra");
});

test("browser host failure invalidates success and preserves foreign files", () => {
  const directory = fixture();
  put(directory, "extensions/chatgpt-web/identity.json", readFileSync(join(root, "extensions/chatgpt-web/identity.json")));
  const binary = "src-tauri/target/release/localbridge-browser-host.exe";
  stageBrowserHost(directory, () => put(directory, binary, "host"), () => "a".repeat(40));
  const evidence = join(directory, "src-tauri/target/browser-host-stage/browser-host-build.json");
  assert.equal(JSON.parse(readFileSync(evidence)).sourceSha, "a".repeat(40));
  assert.throws(() => stageBrowserHost(directory, () => { throw new Error("host compiler failed"); }, () => "b".repeat(40)), /failed/);
  assert.equal(JSON.parse(readFileSync(evidence)).status, "FAIL");
  stageBrowserHost(directory, () => put(directory, binary, "new-host"), () => "c".repeat(40));
  put(directory, "src-tauri/target/browser-host-stage/foreign.exe", "preserved");
  assert.throws(() => stageBrowserHost(directory, () => {}), /remove manually/);
  assert.equal(JSON.parse(readFileSync(evidence)).status, "FAIL");
  assert.equal(readFileSync(join(directory, "src-tauri/target/browser-host-stage/foreign.exe"), "utf8"), "preserved");
});

for (const checkout of [() => "old", () => { throw new Error("checkout unavailable"); }]) {
  test(`preflight rejects unknown source before launching checks: ${checkout}`, () => {
    const directory = fixture();
    let launches = 0;
    assert.throws(() => compilePreflight(directory, () => { launches++; return { status: 0 }; }, checkout));
    const report = JSON.parse(readFileSync(join(directory, "tests/artifacts/ci/COMPILE-PREFLIGHT.json")));
    assert.equal(launches, 0);
    assert.equal(report.status, "FAIL");
    assert.ok(report.checks.every((check) => check.status === "NOT_RUN"));
  });
}

test("resource preparation invalidates all old native evidence before prerequisite failure", () => {
  const directory = fixture();
  const records = ["release-stage/broker-build.json", "local-mcp-stage/adapter-build.json", "browser-host-stage/browser-host-build.json"];
  for (const record of records) put(directory, "src-tauri/target/" + record, JSON.stringify({ status: "PASS", sha256: "a".repeat(64) }));
  put(directory, "src-tauri/target/local-mcp-stage/user.txt", "preserved");
  assert.throws(() => prepareResources({ repository: directory }), /missing/);
  for (const record of records) assert.equal(JSON.parse(readFileSync(join(directory, "src-tauri/target/" + record))).status, "FAIL");
  assert.equal(readFileSync(join(directory, "src-tauri/target/local-mcp-stage/user.txt"), "utf8"), "preserved");
});

for (const [name, stage, binary, evidence] of [
  ["broker", stageBroker, "localbridge-privileged-broker.exe", "release-stage/broker-build.json"],
  ["adapter", stageAdapter, "localbridge-mcp.exe", "local-mcp-stage/adapter-build.json"],
  ["browser", (directory, build) => stageBrowserHost(directory, build, () => "a".repeat(40)), "localbridge-browser-host.exe", "browser-host-stage/browser-host-build.json"],
]) {
  test(`${name} rejects empty replacement after a successful stage`, () => {
    const directory = fixture();
    put(directory, "extensions/chatgpt-web/identity.json", readFileSync(join(root, "extensions/chatgpt-web/identity.json")));
    stage(directory, () => put(directory, "src-tauri/target/release/" + binary, "good"));
    assert.throws(() => stage(directory, () => put(directory, "src-tauri/target/release/" + binary, "")), /empty/);
    assert.equal(JSON.parse(readFileSync(join(directory, "src-tauri/target/" + evidence))).status, "FAIL");
    stage(directory, () => put(directory, "src-tauri/target/release/" + binary, "new-good"));
    const foreign = "src-tauri/target/" + dirname(evidence) + "/user.txt";
    put(directory, foreign, "preserved");
    assert.throws(() => stage(directory, () => {}), /remove manually/);
    assert.equal(JSON.parse(readFileSync(join(directory, "src-tauri/target/" + evidence))).status, "FAIL");
    assert.equal(readFileSync(join(directory, foreign), "utf8"), "preserved");
  });
}

test("host identity and checkout failures invalidate an earlier successful stage", () => {
  for (const failure of ["identity", "checkout"]) {
    const directory = fixture();
    put(directory, "extensions/chatgpt-web/identity.json", readFileSync(join(root, "extensions/chatgpt-web/identity.json")));
    stageBrowserHost(directory, () => put(directory, "src-tauri/target/release/localbridge-browser-host.exe", "good"), () => "a".repeat(40));
    if (failure === "identity") put(directory, "extensions/chatgpt-web/identity.json", "invalid JSON");
    let builds = 0;
    assert.throws(() => stageBrowserHost(directory, () => builds++, () => "invalid"));
    assert.equal(builds, 0);
    assert.equal(JSON.parse(readFileSync(join(directory, "src-tauri/target/browser-host-stage/browser-host-build.json"))).status, "FAIL");
  }
});

test("NSIS explicitly covers both browsers and views with ownership checks", () => {
  const hooks = readFileSync(join(root, "scripts/public-release/nsis-hooks.nsh"), "utf8");
  for (const browser of ["Microsoft\\Edge", "Google\\Chrome"]) for (const view of [32, 64]) {
    for (const macro of ["CheckRegistration", "Register", "Unregister"]) {
      assert.ok(hooks.includes("!insertmacro LocalBridge" + macro + " " + view + ' "' + browser + '"'));
    }
  }
  assert.ok(hooks.includes("--write-manifest"));
  assert.ok(hooks.includes("--prepare-update"));
  assert.ok(!/Delete\s+["'].*[*?]/.test(hooks));
});
