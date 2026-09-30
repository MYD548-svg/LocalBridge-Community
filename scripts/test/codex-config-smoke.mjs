// Actual desktop CLI in an isolated CODEX_HOME. This verifies the upstream
// configuration command contract, not the unbuilt Rust registration wrapper.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
const codex = process.env.LOCALBRIDGE_CODEX_PATH;
if (!codex) throw new Error("LOCALBRIDGE_CODEX_PATH must identify the desktop's bundled official CLI");
const root = resolve(import.meta.dirname, "../..");
const evidence = process.env.LOCALBRIDGE_LOCAL_EVIDENCE_DIR;
if (!evidence) throw new Error("LOCALBRIDGE_LOCAL_EVIDENCE_DIR must be a new retained evidence directory");
mkdirSync(evidence, { recursive: true });
const fixture = mkdtempSync(join(evidence, "Codex 中文 空格-"));
const home = join(fixture, "自定义配置目录");
mkdirSync(home);
const adapter = join(fixture, "安装 目录", "localbridge-mcp.exe");
mkdirSync(resolve(adapter, ".."));
writeFileSync(adapter, "configuration-only fixture; never executed");
const original = "model = 'fixture-model'\nmodel_provider = 'openai'\n[mcp_servers.keep]\ncommand = 'fixture-existing-server'\nargs = ['preserve']\n";
const auth = '{"auth_mode":"apikey","OPENAI_API_KEY":"fixture-not-a-real-key"}\n';
writeFileSync(join(home, "config.toml"), original);
writeFileSync(join(home, "auth.json"), auth);
function cli(args) {
  const result = spawnSync(codex, args, { cwd: fixture, env: { ...process.env, CODEX_HOME: home }, encoding: "utf8", windowsHide: true, timeout: 30000 });
  assert.equal(result.status, 0, `official CLI ${args.slice(0, 2).join(" ")} failed (${result.status ?? result.error?.code})`);
  return result.stdout;
}
function config() {
  const result = spawnSync(join(root, "runtime/python/python.exe"), ["-X", "utf8", "-B", "-c", "import json,sys,tomllib;print(json.dumps(tomllib.loads(sys.stdin.read())))"], { input: readFileSync(join(home, "config.toml"), "utf8"), encoding: "utf8", windowsHide: true });
  assert.equal(result.status, 0, "bundled TOML parser failed");
  return JSON.parse(result.stdout);
}
const before = config();
const id = "a".repeat(64);
let added;
for (let attempt = 0; attempt < 10; attempt++) {
  cli(["mcp", "add", "localbridge", "--", adapter, "--install-id", id]);
  const after = config();
  assert.deepEqual(after.mcp_servers.keep, before.mcp_servers.keep);
  assert.equal(after.model, before.model);
  assert.equal(after.model_provider, before.model_provider);
  assert.equal(after.mcp_servers.localbridge.command, adapter);
  assert.deepEqual(after.mcp_servers.localbridge.args, ["--install-id", id]);
  if (added) assert.deepEqual(after, added);
  added = after;
  assert.equal(readFileSync(join(home, "auth.json"), "utf8"), auth);
}
const readBack = JSON.parse(cli(["mcp", "get", "localbridge", "--json"]));
assert.equal(readBack.transport.command, adapter);
cli(["mcp", "remove", "localbridge"]);
assert.deepEqual(config(), before);
assert.equal(readFileSync(join(home, "auth.json"), "utf8"), auth);
const report = { status: "PASS", coverage: "official desktop CLI only; Rust ownership/backup wrapper NOT_RUN", repetitions: 10, customCodexHome: "PASS", unicodeAndSpacePaths: "PASS", otherMcpAndModelPreserved: "PASS", authFixturePreserved: "PASS", readBackAndRemove: "PASS", fixture };
writeFileSync(join(evidence, "CODEX-CLI-SMOKE.json"), JSON.stringify(report, null, 2) + "\n");
console.log("CODEX_CLI_SMOKE=PASS repetitions=10 coverage=official-command-only fixture-retained=true");
