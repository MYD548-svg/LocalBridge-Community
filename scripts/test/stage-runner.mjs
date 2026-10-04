import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync, writeSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { executableFor, repositoryRoot, validateStages } from "./process.mjs";

const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + "\n");

// The child owns a process group on Unix; Windows taskkill targets only the
// recorded child PID and its descendants. Never terminate processes by name.
async function stopTree(child) {
  if (!child.pid) return;
  if (process.platform === "win32") {
    await new Promise((done, reject) => {
      const killer = spawn("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true, stdio: "ignore", signal: AbortSignal.timeout(10_000) });
      killer.once("error", reject);
      killer.once("close", (code) => code === 0 ? done() : reject(new Error(`taskkill exited ${code}`)));
    });
    return;
  }
  try { process.kill(-child.pid, "SIGTERM"); } catch (error) { if (error.code !== "ESRCH") throw error; }
  await new Promise((done) => setTimeout(done, 250));
  try { process.kill(-child.pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
}

export async function runLoggedStage(stage, { root = repositoryRoot, logDirectory = "tests/artifacts/ci/logs", timeoutMs = 30 * 60_000, echo = true, executionId = randomUUID() } = {}) {
  validateStages([stage]);
  if (!/^[a-z0-9-]+$/.test(stage.id)) throw new Error("invalid stage log id");
  if (!/^[a-f0-9-]{36}$/.test(executionId)) throw new Error("invalid stage execution id");
  const deadline = stage.timeoutMs ?? timeoutMs;
  if (!Number.isSafeInteger(deadline) || deadline <= 0) throw new Error("stage timeout must be a positive integer");
  const directory = resolve(root, logDirectory);
  mkdirSync(directory, { recursive: true });
  const paths = Object.fromEntries(["stdout", "stderr", "result"].map((name) => [name, join(directory, `${stage.id}.${executionId}.${name === "result" ? "json" : name + ".log"}`)]));
  const persist = (value) => {
    save(paths.result, value);
    save(join(directory, `${stage.id}.json`), value);
  };
  const record = { id: stage.id, executionId, program: stage.program, args: stage.args, cwd: stage.cwd ? resolve(root, stage.cwd) : root, startedAt: new Date().toISOString(), timeoutMs: deadline, status: "RUNNING", exitCode: null, signal: null, logs: Object.fromEntries(Object.entries(paths).map(([key, path]) => [key, relative(root, path).replaceAll("\\", "/")])) };
  const started = Date.now();
  persist(record);
  const stdout = openSync(paths.stdout, "w"), stderr = openSync(paths.stderr, "w");
  const { executable, args } = executableFor(stage.program, stage.args);
  let cleanup, timer;
  if (echo) process.stdout.write(`\n[localbridge-test] START ${stage.id} ${record.startedAt}: ${stage.label}\n`);
  try {
    const child = spawn(executable, args, { cwd: record.cwd, env: { ...process.env, ...stage.env }, windowsHide: true, detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"] });
    let finish;
    const complete = new Promise((done) => {
      finish = done;
      child.once("error", (error) => { record.error = error.message; });
      child.once("close", (code, signal) => { record.exitCode = code; record.signal = signal; done(); });
    });
    child.stdout.on("data", (chunk) => { writeSync(stdout, chunk); if (echo) process.stdout.write(chunk); });
    child.stderr.on("data", (chunk) => { writeSync(stderr, chunk); if (echo) process.stderr.write(chunk); });
    timer = setTimeout(() => {
      record.timedOut = true;
      record.error = `stage exceeded ${deadline}ms`;
      persist(record);
      cleanup = stopTree(child).catch((error) => {
        record.cleanupError = error.message;
        child.kill("SIGKILL");
        child.stdout.destroy(); child.stderr.destroy(); child.unref();
        finish();
      });
    }, deadline);
    await complete;
    clearTimeout(timer);
    await cleanup;
    if (record.exitCode !== 0 || record.error) throw new Error(record.error ?? `exit ${record.exitCode ?? "start failure"}`);
    if (stage.requireTests) {
      const output = readFileSync(paths.stdout, "utf8");
      const counts = [...output.matchAll(/test result: ok\. (\d+) passed;/g)].map((match) => Number(match[1]));
      record.passedTests = counts.reduce((total, count) => total + count, 0);
      if (record.passedTests === 0) throw new Error("required behavioral filter executed zero tests");
    }
    record.status = "PASS";
  } catch (error) {
    record.status = "FAIL";
    record.error = error.message;
  } finally {
    clearTimeout(timer);
    closeSync(stdout); closeSync(stderr);
    record.finishedAt = new Date().toISOString();
    record.durationMs = Date.now() - started;
    persist(record);
  }
  if (echo) process.stdout.write(`[localbridge-test] ${record.status} ${stage.id} (${(record.durationMs / 1000).toFixed(1)}s)\n`);
  if (record.status === "FAIL") {
    const error = new Error(`[localbridge-test] FAIL ${stage.id}: ${record.error}`);
    error.record = record;
    throw error;
  }
  return record;
}

// Preserve the synchronous staging/preflight API. A Node supervisor provides
// live log streaming and bounded descendant cleanup while Cargo is running.
export function runLoggedStageSync(stage, options = {}) {
  validateStages([stage]);
  if (!/^[a-z0-9-]+$/.test(stage.id)) throw new Error("invalid stage log id");
  const executionId = randomUUID();
  const path = resolve(options.root ?? repositoryRoot, options.logDirectory ?? "tests/artifacts/ci/logs", `${stage.id}.json`);
  mkdirSync(resolve(path, ".."), { recursive: true });
  // Invalidate old success even if the supervisor itself cannot start.
  save(path, { id: stage.id, executionId, status: "NOT_RUN", exitCode: null });
  const input = JSON.stringify({ stage, options: { ...options, executionId } });
  const result = spawnSync(process.execPath, [resolve(import.meta.dirname, "stage-runner.mjs"), "--execute"], { input, stdio: ["pipe", "inherit", "inherit"], windowsHide: true });
  const record = JSON.parse(readFileSync(path, "utf8"));
  if (result.status !== 0 || record.status !== "PASS" || record.executionId !== executionId) {
    const error = new Error(`[localbridge-test] FAIL ${stage.id}: ${record.error ?? result.error ?? result.status}`);
    error.record = record;
    throw error;
  }
  return record;
}

export async function runReportedStages(stages, report, { root = repositoryRoot, reportPath, execute = runLoggedStage, ...options } = {}) {
  validateStages(stages);
  const selected = new Set(stages.map(({ id }) => id));
  const persist = () => save(resolve(root, reportPath), report);
  mkdirSync(resolve(root, reportPath, ".."), { recursive: true });
  persist();
  for (const stage of stages) {
    const entry = report.stages.find(({ id }) => id === stage.id);
    const blocked = (stage.needs ?? []).filter((id) => selected.has(id) && report.stages.find((item) => item.id === id)?.status !== "PASS");
    if (blocked.length) { Object.assign(entry, { status: "BLOCKED", blockedBy: blocked }); persist(); continue; }
    Object.assign(entry, { status: "RUNNING", startedAt: new Date().toISOString() });
    persist();
    const started = Date.now();
    try { Object.assign(entry, await execute(stage, { root, ...options }), { status: "PASS" }); }
    catch (error) { Object.assign(entry, error.record, { status: "FAIL", error: error.message }); }
    finally { entry.finishedAt = new Date().toISOString(); entry.durationMs = Date.now() - started; persist(); }
  }
  const failures = report.stages.filter(({ status }) => ["FAIL", "BLOCKED"].includes(status));
  report.status = failures.length ? "FAIL" : stages.length === report.stages.length ? "PASS" : "PARTIAL";
  report.finishedAt = new Date().toISOString();
  persist();
  if (failures.length) throw new Error(`gate failed: ${failures.map(({ id, status }) => `${id} (${status})`).join(", ")}`);
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.length !== 3 || process.argv[2] !== "--execute") throw new Error("unknown stage supervisor arguments");
  const { stage, options } = JSON.parse(readFileSync(0, "utf8"));
  try { await runLoggedStage(stage, options); } catch { process.exitCode = 1; }
}
