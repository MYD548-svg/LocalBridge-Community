import { createInterface } from "node:readline";

// Test-only stdio transport. A failed child is never restarted and requests
// are never resubmitted. Injecting the child also permits lifecycle regressions.
export class AdapterRpcClient {
  constructor(child, { name, record = () => {} }) {
    this.child = child;
    this.name = name;
    this.record = record;
    this.pending = new Map();
    this.responses = [];
    this.stderr = "";
    this.exited = false;
    this.failure = null;
    this.exit = new Promise((resolve) => {
      child.once("exit", () => {
        this.exited = true;
        // Give already buffered stdout one event-loop turn to deliver its last
        // response; inherited/open streams cannot keep requests pending.
        this.exitDrain = setImmediate(() => {
          if (this.pending.size) this.fail("adapter_exited");
        });
      });
      child.once("close", (code, signal) => {
        this.exited = true;
        this.record("adapter-exit", { name, code, signal, stderr: this.stderr });
        // Drain stdout before rejecting: an exiting Adapter may still deliver
        // the final error response buffered by the operating system.
        clearImmediate(this.exitDrain);
        if (this.pending.size) this.fail("adapter_exited");
        resolve(code);
      });
    });
    child.on("error", () => { this.exited = true; this.fail("adapter_spawn_failed"); });
    child.stdin.on("error", () => this.fail("adapter_stdin_failed"));
    child.stdout.on("error", () => this.fail("adapter_stdout_failed"));
    child.stderr.on("data", (bytes) => { this.stderr += bytes.toString("utf8"); });
    createInterface({ input: child.stdout }).on("line", (line) => {
      let response;
      try { response = JSON.parse(line); } catch { this.fail("adapter_invalid_json"); return; }
      if (!response || response.jsonrpc !== "2.0" || typeof response !== "object") {
        this.fail("adapter_invalid_envelope"); return;
      }
      if (!Object.hasOwn(response, "id")) {
        if (typeof response.method !== "string") { this.fail("adapter_invalid_notification"); return; }
        this.record("response", { name, response });
        this.responses.push(response);
        return;
      }
      const pending = this.pending.get(response.id);
      if (!pending) { this.fail("adapter_unmatched_response"); return; }
      this.record("response", { name, response });
      this.responses.push(response);
      clearTimeout(pending.timer);
      this.pending.delete(response.id);
      pending.resolve(response);
    });
  }

  fail(code) {
    if (!this.failure) {
      this.failure = new Error(`${this.name}: ${code}`);
      this.record("client-failure", { name: this.name, code });
    }
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(this.failure);
    }
    this.pending.clear();
    if (!this.child.stdin.destroyed && !this.child.stdin.writableEnded) this.child.stdin.end();
  }

  assertWritable() {
    if (this.failure) throw this.failure;
    if (this.exited || this.child.stdin.destroyed || this.child.stdin.writableEnded) {
      throw new Error(`${this.name}: adapter_not_writable`);
    }
  }

  request(method, params, id, timeout = 60000) {
    try { this.assertWritable(); } catch (error) { return Promise.reject(error); }
    if (this.pending.has(id)) return Promise.reject(new Error("duplicate pending request ID"));
    const request = { jsonrpc: "2.0", id, method, ...(params ? { params } : {}) };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.fail("adapter_response_deadline"), timeout);
      this.pending.set(id, { resolve, reject, timer });
      this.record("request", { name: this.name, request });
      try { this.child.stdin.write(JSON.stringify(request) + "\n"); }
      catch { this.fail("adapter_stdin_failed"); }
    });
  }

  notify(method, params) {
    this.assertWritable();
    const request = { jsonrpc: "2.0", method, ...(params ? { params } : {}) };
    this.record("notification", { name: this.name, request });
    try { this.child.stdin.write(JSON.stringify(request) + "\n"); }
    catch { this.fail("adapter_stdin_failed"); throw this.failure; }
  }
}
