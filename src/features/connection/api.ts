import { invoke } from "@tauri-apps/api/core";
import { isRecord } from "../../bridge";

export type ConnectionMode = "local" | "openai_tunnel";
export interface ConnectionState {
  mode: ConnectionMode;
  codexDetected: boolean;
  serviceReady: boolean;
  configurationComplete: boolean;
  autoConnectEnabled: boolean;
  connectedClients: number;
  successfulCalls: number;
  affectedTasks: string[];
}
export function parseConnectionState(value: unknown): ConnectionState {
  if (!isRecord(value) || !["local", "openai_tunnel"].includes(String(value.mode))
    || typeof value.codexDetected !== "boolean" || typeof value.serviceReady !== "boolean"
    || typeof value.configurationComplete !== "boolean"
    || typeof value.autoConnectEnabled !== "boolean"
    || !Number.isSafeInteger(value.connectedClients) || Number(value.connectedClients) < 0
    || !Number.isSafeInteger(value.successfulCalls) || Number(value.successfulCalls) < 0
    || !Array.isArray(value.affectedTasks) || !value.affectedTasks.every((task) => typeof task === "string")) {
    throw new Error("后端连接状态合同不兼容");
  }
  return value as unknown as ConnectionState;
}
export const connectionApi = {
  read: async () => parseConnectionState(await invoke<unknown>("get_connection_state")),
  setMode: (mode: ConnectionMode, confirmedCancel = false) => invoke<void>("set_connection_mode", { mode, confirmedCancel }),
  connect: () => invoke<void>("connect_codex"),
  autoConnect: () => invoke<void>("auto_connect_codex"),
  disconnect: (confirmedCancel = false) => invoke<void>("disconnect_codex", { confirmedCancel }),
};
