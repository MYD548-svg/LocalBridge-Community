import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import test from "node:test";

test("bundled Python preserves command output through terminal settlement", { timeout: 900_000 }, () => {
  const root = resolve(import.meta.dirname, "../..");
  const result = spawnSync(resolve(root, "runtime/python/python.exe"), ["-B", "tests/runtime_output_regression.py"], {
    cwd: root, encoding: "utf8", windowsHide: true, timeout: 890_000,
    env: { ...process.env, PYTHONDONTWRITEBYTECODE: "1" },
  });
  assert.equal(result.status, 0, `${result.error ?? ""}\n${result.stdout}\n${result.stderr}`);
  process.stdout.write(result.stdout);
  process.stdout.write(result.stderr);
});
