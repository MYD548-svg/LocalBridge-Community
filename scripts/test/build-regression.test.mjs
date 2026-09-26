import assert from "node:assert/strict";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { stageBroker } from "../prepare-lb018-resources.mjs";
import { updateManifest } from "./update-tunnel.mjs";
import { verifyRuntime, verifyHash, sha256, rejectExtras } from "./runtime-integrity.mjs";
import { runStages } from "./process.mjs";
import { validateCheckout } from "./source-check.mjs";

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
  assert.equal(JSON.parse(readFileSync(join(directory, evidence))).status, "BUILDING");
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

test("complete bundled trees pass; changed runtime source fails", () => {
  assert.equal(verifyRuntime(root, { bundledOnly: true }).status, "PASS");
  const directory = fixture();
  for (const path of ["runtime", "runtime-manifest.toml", "provenance/runtime-lock.json", "src-tauri/src/tunnel/bundle.rs", "src-tauri/src/mcp/bundle.rs", "src-tauri/tauri.conf.json"]) {
    mkdirSync(dirname(join(directory, path)), { recursive: true });
    cpSync(join(root, path), join(directory, path), { recursive: true });
  }
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
  assert.equal(verifyRuntime(directory).status, "PASS");
  const configPath = "src-tauri/tauri.conf.json";
  const config = readFileSync(join(directory, configPath));
  put(directory, configPath, JSON.stringify({ ...JSON.parse(config), bundle: { ...JSON.parse(config).bundle, resources: {} } }));
  assert.throws(() => verifyRuntime(directory), /attested staged broker/);
  put(directory, configPath, config);
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
