import { describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { connectionApi, parseConnectionState } from "../features/connection/api";

const state = { mode: "local", codexDetected: true, serviceReady: true, configurationComplete: false, autoConnectEnabled: true, connectedClients: 0, successfulCalls: 0, affectedTasks: [] };
describe("independent local connection state", () => {
  it("keeps service readiness, configuration and a real client distinct", async () => {
    invoke.mockResolvedValue(state);
    expect(await connectionApi.read()).toEqual(state);
    expect(parseConnectionState({ ...state, configurationComplete: true }).connectedClients).toBe(0);
  });
  it("rejects invalid mode, counts and affected task lists", () => {
    for (const change of [{ mode: "remote" }, { connectedClients: -1 }, { connectedClients: 0.5 }, { affectedTasks: [42] }, { serviceReady: null }]) {
      expect(() => parseConnectionState({ ...state, ...change })).toThrow();
    }
  });
  it("only submits explicit cancellation for a confirmed operation", async () => {
    invoke.mockResolvedValue(undefined);
    await connectionApi.setMode("openai_tunnel");
    expect(invoke).toHaveBeenLastCalledWith("set_connection_mode", { mode: "openai_tunnel", confirmedCancel: false });
    await connectionApi.disconnect(true);
    expect(invoke).toHaveBeenLastCalledWith("disconnect_codex", { confirmedCancel: true });
    await connectionApi.connect();
    expect(invoke).toHaveBeenLastCalledWith("connect_codex");
  });
});
