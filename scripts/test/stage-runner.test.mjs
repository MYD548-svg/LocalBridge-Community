import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runLoggedStage, runLoggedStageSync, runReportedStages } from "./stage-runner.mjs";
import { authStages } from "./auth-repeat.mjs";

const fixture = () => mkdtempSync(join(tmpdir(), "localbridge-stage-runner-"));
const node = (id, source, extra = {}) => ({ id, label: "fixture", program: process.execPath, args: ["-e", source], ...extra });
const options = (root) => ({ root, echo: false });

test("independent failures are collected and a dependent package is blocked", async () => {
  const root = fixture(), marker = join(root, "must-not-package");
  const stages = [node("first", "process.exit(7)"), node("second", "process.exit(9)"), node("healthy", "console.log('ok')"), node("package", `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'bad')`, { needs: ["first", "second"] })];
  const report = { stages: stages.map(({ id }) => ({ id, status: "NOT_RUN" })) };
  await assert.rejects(runReportedStages(stages, report, { ...options(root), reportPath: "report.json" }), /first \(FAIL\).*second \(FAIL\).*package \(BLOCKED\)/);
  const saved = JSON.parse(readFileSync(join(root, "report.json")));
  assert.equal(saved.status, "FAIL");
  assert.deepEqual(saved.stages.map(({ status }) => status), ["FAIL", "FAIL", "PASS", "BLOCKED"]);
  assert.deepEqual(saved.stages.at(-1).blockedBy, ["first", "second"]);
  assert.equal(existsSync(marker), false);
  assert.deepEqual(saved.stages.slice(0, 2).map(({ exitCode }) => exitCode), [7, 9]);
});

test("success captures both streams, arguments and execution timing", async () => {
  const root = fixture();
  const stage = node("streams", "console.log('stdout evidence'); console.error('stderr evidence')");
  const record = await runLoggedStage(stage, options(root));
  assert.equal(record.exitCode, 0);
  assert.deepEqual(record.args, stage.args);
  assert.match(readFileSync(join(root, record.logs.stdout), "utf8"), /stdout evidence/);
  assert.match(readFileSync(join(root, record.logs.stderr), "utf8"), /stderr evidence/);
  assert.ok(Date.parse(record.finishedAt) >= Date.parse(record.startedAt));
  assert.ok(record.durationMs >= 0);
});

test("failed startup and nonzero exit leave FAIL evidence", async () => {
  const root = fixture();
  for (const stage of [{ id: "missing", label: "fixture", program: join(root, "absent-command"), args: [] }, node("nonzero", "console.error('specific failure'); process.exit(17)")]) {
    await assert.rejects(runLoggedStage(stage, options(root)), /FAIL/);
    const result = JSON.parse(readFileSync(join(root, `tests/artifacts/ci/logs/${stage.id}.json`)));
    assert.equal(result.status, "FAIL");
    assert.ok(result.error);
    assert.ok(result.finishedAt);
  }
  assert.equal(JSON.parse(readFileSync(join(root, "tests/artifacts/ci/logs/nonzero.json"))).exitCode, 17);
});

test("timeouts terminate the stage and its running descendant", async () => {
  const root = fixture(), pidFile = join(root, "descendant.pid");
  const source = `const {spawn}=require('node:child_process'); const fs=require('node:fs'); process.on('SIGTERM',()=>{}); const child=spawn(process.execPath,['-e',"process.on('SIGTERM',()=>{}); setInterval(()=>{},1000)"],{stdio:'inherit'}); fs.writeFileSync(${JSON.stringify(pidFile)},String(child.pid)); setInterval(()=>{},1000);`;
  await assert.rejects(runLoggedStage(node("timeout", source, { timeoutMs: 2000 }), options(root)), /exceeded/);
  const result = JSON.parse(readFileSync(join(root, "tests/artifacts/ci/logs/timeout.json")));
  assert.equal(result.timedOut, true);
  assert.equal(result.status, "FAIL");
  const pid = Number(readFileSync(pidFile, "utf8"));
  // A killed Unix orphan can remain a zombie until init reaps it. It is no
  // longer executing; Windows does not retain that process state.
  try {
    process.kill(pid, 0);
    if (process.platform === "linux") assert.equal(readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1][0], "Z");
    else assert.fail("stage descendant survived timeout");
  } catch (error) { if (error.code !== "ESRCH" && error.code !== "ENOENT") throw error; }
});

test("zero behavioral tests cannot pass and a real successful count is recorded", async () => {
  const root = fixture();
  await assert.rejects(runLoggedStage(node("empty", "console.log('test result: ok. 0 passed; 0 failed; 1 ignored;')", { requireTests: true }), options(root)), /zero tests/);
  const record = await runLoggedStage(node("behavior", "console.log('test result: ok. 3 passed; 0 failed; 0 ignored;')", { requireTests: true }), options(root));
  assert.equal(record.passedTests, 3);
});

test("the synchronous preflight supervisor preserves failure diagnostics", () => {
  const root = fixture();
  const previous = runLoggedStageSync(node("sync", "console.log('compiled')"), options(root));
  assert.equal(previous.status, "PASS");
  assert.throws(() => runLoggedStageSync(node("sync", "process.exit(19)"), options(root)), /FAIL/);
  const failed = JSON.parse(readFileSync(join(root, "tests/artifacts/ci/logs/sync.json")));
  assert.equal(failed.exitCode, 19);
  assert.equal(failed.status, "FAIL");
  assert.notEqual(failed.executionId, previous.executionId);
  assert.match(readFileSync(join(root, previous.logs.stdout), "utf8"), /compiled/);
  assert.equal(JSON.parse(readFileSync(join(root, previous.logs.result))).executionId, previous.executionId);
});

test("auth repetition keeps all 25 probes and rejects empty filters", () => {
  const stages = authStages();
  assert.equal(stages.length, 25);
  for (const prefix of ["auth-", "stderr-"]) {
    const repeats = stages.filter(({ id }) => id.startsWith(prefix));
    assert.equal(repeats.length, 10);
    assert.ok(repeats.every(({ args }) => args.includes("--exact")));
  }
  assert.ok(stages.every(({ args, requireTests, timeoutMs }) => args.includes("--locked") && requireTests && timeoutMs > 0));
});
