import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MARKER, Reassembler, fragments } from "./core";
import identity from "./identity.json";
type Listener = (request: unknown, sender: unknown, respond: (value: any) => void) => boolean;
let listener: Listener;
let binding: string;
let candidate: any;
let unknown: boolean;
let stored: Record<string, any>;
let requests: any[];
let pageRequest: ReturnType<typeof vi.fn>;
const popup = { id: identity.id, url: "chrome-extension://" + identity.id + "/popup.html" };
const sender = { id: identity.id, frameId: 0, url: "https://chatgpt.com/c/one", tab: { id: 1 } };
async function action(type: string, fields = {}, source: unknown = popup): Promise<any> {
  return new Promise(resolve => listener({ type, tab: 1, ...fields }, source, resolve));
}
beforeEach(async () => {
  vi.resetModules();
  stored = { identity: { instance: "11111111-1111-1111-1111-111111111111", secret: "a".repeat(64) } };
  binding = "chat:one"; unknown = false; requests = [];
  candidate = { text: JSON.stringify({ [MARKER]: { name: "filesystem", arguments: { path: "中文.txt" } } }), observation: { message: "assistant", branch: "user", index: 0 } };
  pageRequest = vi.fn(async (_tab: number, request: any) => {
    if (request.type === "binding") return { ok: true, chat: binding, compatible: true };
    if (request.type === "candidate") return { ok: true, chat: binding, ...candidate };
    return { ok: true };
  });
  const connectNative = vi.fn(() => {
    const assembly = new Reassembler();
    let receive: (frame: unknown) => void = () => undefined;
    let disconnected: () => void = () => undefined;
    let closed = false;
    return { onMessage: { addListener: (fn: typeof receive) => { receive = fn; } },
      onDisconnect: { addListener: (fn: typeof disconnected) => { disconnected = fn; } },
      disconnect: () => { if (!closed) { closed = true; disconnected(); } },
      postMessage: (frame: unknown) => {
        const message: any = assembly.push(frame);
        if (!message) return;
        requests.push(message);
        let response: unknown;
        if (message.type === "pair" || message.type === "enable") response = { version: 1, type: "state", requestId: message.requestId, payload: { approved: true } };
        else if (message.type === "mcp" && message.payload.id) {
          const method = message.payload.method;
          const result = method === "initialize" ? { protocolVersion: "2025-11-25" } : method === "tools/list"
            ? { tools: [{ name: "filesystem", description: "read", inputSchema: { type: "object", properties: { path: { type: "string" } }, required: ["path"], additionalProperties: false } },
              { name: "workspace_context", description: "read project context", inputSchema: { type: "object", properties: {}, additionalProperties: false } }] }
            : { isError: false, content: [{ type: "text", text: "实际结果" }], structuredContent: { output_refs: { stdout: "retained" } } };
          response = { version: 1, type: "mcp", payload: { jsonrpc: "2.0", id: message.payload.id,
            ...(unknown && method === "tools/call" ? { error: { code: -32011, message: "unknown" } } : { result }) } };
        }
        if (response) for (const reply of fragments(response, crypto.randomUUID())) receive(reply);
      } };
  });
  vi.stubGlobal("chrome", { runtime: { id: identity.id, getURL: (path: string) => "chrome-extension://" + identity.id + "/" + path,
    onMessage: { addListener: (fn: Listener) => { listener = fn; } }, connectNative },
    storage: { local: { setAccessLevel: vi.fn(async () => undefined), get: vi.fn(async () => stored),
      set: vi.fn(async (value: Record<string, unknown>) => { Object.assign(stored, value); }) } },
    tabs: { sendMessage: pageRequest, onRemoved: { addListener: vi.fn() } } });
  await import("./background");
});
afterEach(() => vi.unstubAllGlobals());
describe("mock native host and extension confirmation flow", () => {
  async function enabled(): Promise<void> { expect((await action("enable")).ok).toBe(true); }
  async function observed(): Promise<void> { expect((await action("observe", { chat: binding, ...candidate }, sender)).ok).toBe(true); }
  const calls = () => requests.filter(request => request.payload?.method === "tools/call");
  it("first-use guidance fills only read-only instructions and never executes or sends", async () => {
    expect((await action("first_call")).ok).toBe(false);
    await enabled();
    const response = await action("first_call");
    expect(response.ok).toBe(true);
    expect(response.value.selected).toEqual(["workspace_context"]);
    expect(pageRequest.mock.calls.find(([, request]) => request.type === "fill")?.[1].text).toContain("workspace_context");
    expect(calls()).toHaveLength(0);
  });
  it("first-use guidance cannot discard a request already waiting for confirmation", async () => {
    await enabled(); await observed();
    expect((await action("first_call")).ok).toBe(false);
    expect((await action("read")).value.status).toBe("pending");
    expect(calls()).toHaveLength(0);
  });
  it("does not execute observations or fill results until separate popup confirmations", async () => {
    await enabled(); await observed(); expect(calls()).toHaveLength(0);
    expect((await action("execute")).value.status).toBe("succeeded"); expect(calls()).toHaveLength(1);
    expect(pageRequest.mock.calls.some(([, request]) => request.type === "fill")).toBe(false);
    await action("fill");
    const fill = pageRequest.mock.calls.find(([, request]) => request.type === "fill")?.[1];
    expect(fill?.text).toContain("retained");
    expect((await action("execute", {}, sender)).ok).toBe(false);
    expect(calls()).toHaveLength(1);
  });
  it("rechecks live parameters, rejects duplicate confirmations and pauses after navigation", async () => {
    await enabled(); await observed();
    const original = candidate.text;
    candidate.text = JSON.stringify({ [MARKER]: { name: "filesystem", arguments: { path: "changed.txt" } } });
    expect((await action("execute")).ok).toBe(false); expect(calls()).toHaveLength(0);
    candidate.text = original;
    const results = await Promise.all([action("execute"), action("execute")]);
    expect(results.filter(result => result.ok)).toHaveLength(1); expect(calls()).toHaveLength(1);
    binding = "chat:two";
    expect((await action("read")).value.enabled).toBe(false);
    expect((await action("fill")).ok).toBe(false);
    binding = "chat:one";
    expect((await action("read")).value.result).toBeDefined();
    expect((await action("copy")).value.text).toContain("retained");
  });
  it("unknown results and a restarted background never replay a submitted operation", async () => {
    await enabled(); await observed(); unknown = true;
    expect((await action("execute")).value.status).toBe("unknown");
    expect((await action("execute")).ok).toBe(false); expect(calls()).toHaveLength(1);
    stored.chats = { 1: { chat: binding, status: "running", enabled: true } };
    vi.resetModules(); await import("./background");
    expect((await action("read")).value.status).toBe("unknown");
    expect((await action("execute")).ok).toBe(false); expect(calls()).toHaveLength(1);
  });
});
