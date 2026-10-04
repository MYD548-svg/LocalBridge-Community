import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative } from "node:path";
import test from "node:test";
import { repositoryRoot } from "./process.mjs";
import { runLoggedStage } from "./stage-runner.mjs";

const typesStart = '#[derive(Debug, Clone, PartialEq, Eq, Serialize)]\n#[serde(rename_all = "camelCase")]\npub struct MainProjection';
const typesEnd = "const ADMIN_CONSENT_DURATION:";
const testStart = "#[test]\nfn main_projection_json_contract_matches_the_frontend_fixture()";
const testEnd = "#[test]\nfn unavailable_and_stale_sections_never_become_live_ui_business_state()";
const expectedTypes = ["MainProjection", "UiWorkspaceProjection", "UiConnectionProjection", "UpdateProjection", "OpenReleaseProjection", "UiFaultProjection", "ProjectProjection", "TaskProjection", "CurrentActivityProjection", "LastActivityProjection", "ReconnectProjection"];
const inputPaths = ["src-tauri/src/commands/ui.rs", "tests/unit/ui/backend_projection.rs", "tests/fixtures/ui/main_projection.json", "scripts/test/fixtures/ui-projection-probe/Cargo.toml", "scripts/test/fixtures/ui-projection-probe/Cargo.lock", "src-tauri/Cargo.lock", "scripts/test/ui-projection-contract.test.mjs"];
const digest = bytes => createHash("sha256").update(bytes).digest("hex");

function between(source, start, end) {
  const text = source.replaceAll("\r\n", "\n");
  assert.equal(text.split(start).length, 2, `missing or ambiguous extraction start: ${start}`);
  assert.equal(text.split(end).length, 2, `missing or ambiguous extraction end: ${end}`);
  assert.ok(text.indexOf(start) < text.indexOf(end), "extraction boundaries are reversed");
  return text.slice(text.indexOf(start), text.indexOf(end));
}

function packageEntries(lock) {
  return lock.replaceAll("\r\n", "\n").split("[[package]]").slice(1).map(block => ({
    name: block.match(/^name = "([^"]+)"/m)?.[1],
    version: block.match(/^version = "([^"]+)"/m)?.[1],
    checksum: block.match(/^checksum = "([^"]+)"/m)?.[1],
  }));
}

const serializationTests = `
fn update_fixture(failure_reason: Option<String>) -> UpdateProjection {
    UpdateProjection {
        state: "failed", current_version: "0.1.5".into(), latest_version: None,
        release_url: None, operation_id: None, attempt: None, retryable: true,
        failure_reason,
    }
}
#[test]
fn absent_update_failure_keeps_the_existing_json_contract() {
    let value = serde_json::to_value(update_fixture(None)).unwrap();
    assert!(value.get("failureReason").is_none());
    assert!(value.get("failure_reason").is_none());
}
#[test]
fn update_failure_is_exposed_with_the_frontend_field_name() {
    let value = serde_json::to_value(update_fixture(Some("本项目尚未发布配套下载".into()))).unwrap();
    assert_eq!(value["failureReason"], "本项目尚未发布配套下载");
    assert!(value.get("failure_reason").is_none());
}
`;

test("actual UI projection types compile with the original fixture and reject an unrelated failure field", async () => {
  const parent = join(repositoryRoot, "tests/artifacts/ci/ui-projection-probes");
  mkdirSync(parent, { recursive: true });
  const probe = mkdtempSync(join(parent, "probe-"));
  // Keep compiler outputs out of the always-uploaded CI diagnostics tree.
  const target = join(repositoryRoot, "tests/artifacts/ui-projection-target", basename(probe));
  const evidencePath = join(probe, "contract.json");
  const report = { status: "RUNNING", startedAt: new Date().toISOString(), sourceHashes: {}, cases: {} };
  const save = () => writeFileSync(evidencePath, JSON.stringify(report, null, 2) + "\n");
  save();
  try {
    const inputs = Object.fromEntries(inputPaths.map(name => [name, readFileSync(join(repositoryRoot, name))]));
    report.sourceHashes = Object.fromEntries(Object.entries(inputs).map(([name, bytes]) => [name, digest(bytes)]));
    const types = between(inputs[inputPaths[0]].toString(), typesStart, typesEnd);
    const names = [...types.matchAll(/^(?:pub )?struct (\w+) \{/gm)].map(match => match[1]);
    assert.deepEqual(names, expectedTypes, "the projection declaration block changed; update the extraction explicitly");
    const fixtureTest = between(inputs[inputPaths[1]].toString(), testStart, testEnd);
    assert.equal((fixtureTest.match(/^#\[test\]/gm) ?? []).length, 1, "exactly the original fixture test must be extracted");
    const helperPackages = packageEntries(inputs[inputPaths[4]].toString()).filter(pkg => pkg.checksum);
    const applicationPackages = packageEntries(inputs[inputPaths[5]].toString());
    assert.ok(helperPackages.length > 0, "the probe lock must include real dependencies");
    for (const pkg of helperPackages) {
      assert.ok(applicationPackages.some(actual => actual.name === pkg.name && actual.version === pkg.version && actual.checksum === pkg.checksum), `probe dependency must match the application lock: ${pkg.name} ${pkg.version}`);
    }
    const original = "use serde::Serialize;\n" + types + fixtureTest + serializationTests;
    const faults = [...fixtureTest.matchAll(/active_faults: vec!\[UiFaultProjection \{[\s\S]*?\n        \}\],/g)];
    assert.equal(faults.length, 1, "expected exactly one fault literal for the compiler rejection probe");
    assert.ok(!faults[0][0].includes("failure_reason"), "the unrelated fault field is still present in the real fixture");
    const invalidFault = faults[0][0].replace("\n        }],", "\n            failure_reason: None,\n        }],");
    const invalid = original.replace(faults[0][0], invalidFault);
    assert.notEqual(invalid, original, "the compiler rejection input must change");
    const logDirectory = relative(repositoryRoot, join(probe, "logs"));
    async function compile(name, source, requireTests) {
      const base = join(probe, name), crate = join(base, "src-tauri");
      mkdirSync(join(crate, "src"), { recursive: true });
      for (const file of ["Cargo.toml", "Cargo.lock"]) copyFileSync(join(repositoryRoot, "scripts/test/fixtures/ui-projection-probe", file), join(crate, file));
      const fixture = join(base, "tests/fixtures/ui/main_projection.json");
      mkdirSync(dirname(fixture), { recursive: true });
      writeFileSync(fixture, inputs[inputPaths[2]]);
      writeFileSync(join(crate, "src/lib.rs"), source);
      report.cases[name] = { status: "RUNNING", generatedSourceSha256: digest(source) };
      save();
      return runLoggedStage({
        id: `ui-projection-${name}`, label: "actual portable UI projection contract", program: "cargo",
        args: ["+1.85.0", "test", "--manifest-path", join(crate, "Cargo.toml"), "--target-dir", target, "--locked", "--color", "never", "--lib", "--", "--test-threads=1"],
        requireTests, timeoutMs: 10 * 60_000,
      }, { root: repositoryRoot, logDirectory, echo: false });
    }
    const current = await compile("current", original, true);
    assert.equal(current.passedTests, 3, "the original fixture and both serialization checks must execute");
    report.cases.current = { ...report.cases.current, ...current };
    save();
    let rejection;
    try { await compile("invalid-fault", invalid, false); }
    catch (error) { rejection = error.record; }
    assert.ok(rejection, "Rust must reject a failure field on UiFaultProjection");
    assert.equal(rejection.exitCode, 101);
    assert.ok(!rejection.timedOut && !rejection.cleanupError, "a timeout or cleanup failure is not the expected compiler rejection");
    const compiler = readFileSync(join(repositoryRoot, rejection.logs.stderr), "utf8");
    assert.match(compiler, /error\[E0560\]: struct `UiFaultProjection` has no field named `failure_reason`/);
    report.cases["invalid-fault"] = { ...report.cases["invalid-fault"], ...rejection, status: "EXPECTED_REJECTION", compilerStatus: rejection.status };
    for (const name of inputPaths) assert.equal(digest(readFileSync(join(repositoryRoot, name))), report.sourceHashes[name], `input changed during the probe: ${name}`);
    report.status = "PASS";
    console.log(`UI_PROJECTION_CONTRACT=PASS rust_tests=3 expected_rejections=1 evidence=${relative(repositoryRoot, evidencePath).replaceAll("\\", "/")}`);
  } catch (error) {
    report.status = "FAIL";
    report.error = error.message;
    if (error.record) report.failedCompilation = error.record;
    throw error;
  } finally {
    report.finishedAt = new Date().toISOString();
    save();
  }
});
