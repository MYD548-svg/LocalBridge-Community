import { describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { browserApi, parseBrowserState } from "../features/connection/browserApi";
const state = { protocol: 1, extensionId: "a".repeat(32), directory: "C:\\测试", preparedVersion: null,
  serviceReady: true, connectedBrowsers: 0, connectedChats: 0, successfulCalls: 0, pairings: [] };
describe("browser connection state and explicit user actions", () => {
  it("does not confuse prepared files with connected chats or real calls", () => {
    expect(parseBrowserState({ ...state, preparedVersion: "0.1.5" }).successfulCalls).toBe(0);
    for (const change of [{ protocol: 2 }, { extensionId: "*" }, { connectedChats: -1 }, { pairings: [{}] }]) expect(() => parseBrowserState({ ...state, ...change })).toThrow();
  });
  it("passes the explicit disabled-extension acknowledgment and pairing identity", async () => {
    invoke.mockResolvedValue("0.1.5");
    await browserApi.import("C:\\包.zip", true);
    expect(invoke).toHaveBeenLastCalledWith("import_browser_extension", { path: "C:\\包.zip", confirmedDisabled: true });
    await browserApi.revoke("instance");
    expect(invoke).toHaveBeenLastCalledWith("revoke_browser_pairing", { instance: "instance" });
    await browserApi.approve("instance", "b".repeat(64));
    expect(invoke).toHaveBeenLastCalledWith("approve_browser_pairing", { instance: "instance", context: "b".repeat(64) });
  });
});
