import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import test from "node:test";
import { AdapterRpcClient } from "./adapter-client.mjs";

function fixture() {
  const child = new EventEmitter();
  child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.stderr = new PassThrough();
  const writes = [], records = [];
  child.stdin.on("data", (bytes) => writes.push(bytes.toString()));
  const client = new AdapterRpcClient(child, { name: "synthetic", record: (type, data) => records.push({ type, ...data }) });
  return { child, client, writes, records };
}

test("request after process exit rejects without writing or waiting", async () => {
  const { child, client, writes } = fixture();
  child.emit("exit", 1);
  await assert.rejects(client.request("initialize", {}, 1), /adapter_not_writable/);
  assert.deepEqual(writes, []); assert.equal(client.pending.size, 0);
  child.emit("close", 1);
});

test("early close rejects all outstanding requests and clears timers", async () => {
  const { child, client, writes } = fixture();
  const a = assert.rejects(client.request("initialize", {}, 1), /adapter_exited/);
  const b = assert.rejects(client.request("tools\/list", {}, 2), /adapter_exited/);
  child.emit("exit", 1); child.emit("close", 1);
  await Promise.all([a, b]); assert.equal(client.pending.size, 0); assert.equal(writes.length, 2);
});

test("buffered final error is received after exit and before stream close", async () => {
  const { child, client } = fixture();
  const request = client.request("tools/list", null, 0);
  child.emit("exit", 1);
  child.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: 0, error: { code: -32000, message: "initialize is required" } }) + "\n");
  child.emit("close", 1);
  assert.equal((await request).id, 0); assert.equal(client.pending.size, 0);
});

test("exit with inherited open streams cannot retain pending requests", async () => {
  const { child, client } = fixture();
  const pending = assert.rejects(client.request("initialize", {}, 1), /adapter_exited/);
  child.emit("exit", 1);
  await pending;
  assert.equal(client.pending.size, 0);
  child.emit("close", 1);
});

for (const [name, trigger, code] of [
  ["stdin error", (child) => child.stdin.emit("error", new Error("synthetic private detail")), "adapter_stdin_failed"],
  ["invalid JSON", (child) => child.stdout.write("invalid synthetic private detail\n"), "adapter_invalid_json"],
  ["unknown response", (child) => child.stdout.write('{"jsonrpc":"2.0","id":99,"result":{}}\n'), "adapter_unmatched_response"],
  ["spawn error", (child) => child.emit("error", new Error("synthetic private detail")), "adapter_spawn_failed"],
]) test(`${name} fails immediately without retry or raw error logging`, async () => {
  const { child, client, writes, records } = fixture();
  const pending = assert.rejects(client.request("initialize", {}, 1), new RegExp(code));
  trigger(child); await pending;
  await assert.rejects(client.request("initialize", {}, 2), new RegExp(code));
  assert.equal(client.pending.size, 0); assert.equal(writes.length, 1);
  assert.ok(!JSON.stringify(records).includes("private detail"));
  child.emit("close", 1);
});
