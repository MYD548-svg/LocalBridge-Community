import assert from "node:assert/strict";
import test from "node:test";

import { CI_STAGES, cargoCommand, parseGateArguments } from "./ci-gate.mjs";
import { selectStages, validateStages } from "./process.mjs";

test("the shared local and CI gate has stable unique stages", () => {
  assert.deepEqual(
    CI_STAGES.map((stage) => stage.id),
    [
      "toolchains",
      "dependencies",
      "tunnel-source",
      "bundled-integrity",
      "test-base",
      "format",
      "public-release",
      "licenses",
      "schema44",
      "frontend-test",
      "frontend-build",
      "runtime-resources",
      "staged-integrity",
      "auth-repeat",
      "rust-test",
      "rust-clippy",
      "nsis-package",
      "package-integrity",
      "artifacts",
    ],
  );
  assert.throws(
    () => validateStages([CI_STAGES[0], CI_STAGES[0]]),
    /duplicate test stage id/,
  );
});

test("targeted local diagnosis reuses the declared gate instead of copying commands", () => {
  assert.deepEqual(selectStages(CI_STAGES, { only: "rust-test" }).map(({ id }) => id), [
    "rust-test",
  ]);
  assert.deepEqual(selectStages(CI_STAGES, { from: "rust-clippy" }).map(({ id }) => id), [
    "rust-clippy",
    "nsis-package",
    "package-integrity",
    "artifacts",
  ]);
  assert.deepEqual(
    selectStages(CI_STAGES, { from: "schema44", through: "frontend-build" }).map(({ id }) => id),
    ["schema44", "frontend-test", "frontend-build"],
  );
  assert.deepEqual(
    selectStages(CI_STAGES, { through: "rust-clippy" }).at(-1)?.id,
    "rust-clippy",
  );
  assert.deepEqual(parseGateArguments(["--only", "schema44"]), {
    only: "schema44",
  });
  assert.deepEqual(parseGateArguments(["--from", "schema44", "--through", "rust-test"]), {
    from: "schema44",
    through: "rust-test",
  });
  assert.throws(() => parseGateArguments(["--only", "format", "--from", "rust-test"]));
  assert.throws(() => selectStages(CI_STAGES, { from: "rust-test", through: "format" }));
});

test("the full Rust gate collects independent target failures without weakening its result", () => {
  const rust = CI_STAGES.find(({ id }) => id === "rust-test");
  assert.ok(rust.args.includes("--no-fail-fast"));
  assert.ok(rust.args.includes("--locked"));
  assert.deepEqual(rust.args.slice(-2), ["--", "--test-threads=1"]);
});

test("the base gate compiles the real UI projection fixture before native resource builds", () => {
  const base = CI_STAGES.find(stage => stage.id === "test-base");
  assert.ok(base.args.includes("scripts/test/ui-projection-contract.test.mjs"));
  assert.ok(CI_STAGES.find(stage => stage.id === "runtime-resources").needs.includes(base.id));
});

test("a running desktop binary can use the same Rust gate with an isolated target directory", () => {
  assert.deepEqual(cargoCommand(["test"], {}).args, ["+1.85.0", "test"]);
  assert.deepEqual(
    cargoCommand(["test"], { LOCALBRIDGE_CARGO_TARGET_DIR: "src-tauri/target/local-gate" }).args,
    ["+1.85.0", "test", "--target-dir", "src-tauri/target/local-gate"],
  );
});

test("only the shared resource gate opts into compile preflight", () => {
  const resources = CI_STAGES.find((stage) => stage.id === "runtime-resources");
  assert.deepEqual(resources.args, ["scripts/prepare-lb018-resources.mjs", "--compile-preflight"]);
});

test("packaging requires all earlier gates and dependencies refer to earlier stages", () => {
  const packageIndex = CI_STAGES.findIndex(({ id }) => id === "nsis-package");
  assert.deepEqual(CI_STAGES[packageIndex].needs, CI_STAGES.slice(0, packageIndex).map(({ id }) => id));
  for (const [index, stage] of CI_STAGES.entries()) {
    assert.ok(stage.needs.every((id) => CI_STAGES.slice(0, index).some((item) => item.id === id)));
    assert.ok(stage.timeoutMs > 0);
  }
});
