import { invoke } from "@tauri-apps/api/core";
import { isRecord } from "../../bridge";
export interface BrowserPairing { instance: string; workspace: string; permission: string; approved: boolean; revoked: boolean; context: string }
export interface BrowserState { protocol: number; extensionId: string; directory: string; preparedVersion: string | null; serviceReady: boolean; connectedBrowsers: number; connectedChats: number; successfulCalls: number; pairings: BrowserPairing[] }
export function parseBrowserState(value: unknown): BrowserState {
  if (!isRecord(value) || value.protocol !== 1 || typeof value.extensionId !== "string" || !/^[a-p]{32}$/.test(value.extensionId)
    || typeof value.directory !== "string" || !(value.preparedVersion === null || typeof value.preparedVersion === "string")
    || typeof value.serviceReady !== "boolean" || !Number.isSafeInteger(value.connectedChats) || Number(value.connectedChats) < 0
    || !Number.isSafeInteger(value.connectedBrowsers) || Number(value.connectedBrowsers) < 0
    || !Number.isSafeInteger(value.successfulCalls) || Number(value.successfulCalls) < 0 || !Array.isArray(value.pairings)
    || !value.pairings.every(pair => isRecord(pair) && typeof pair.instance === "string" && typeof pair.workspace === "string"
      && typeof pair.permission === "string" && typeof pair.approved === "boolean" && typeof pair.revoked === "boolean" && typeof pair.context === "string" && /^[a-f0-9]{64}$/.test(pair.context))) {
    throw new Error("浏览器连接状态合同不兼容");
  }
  return value as unknown as BrowserState;
}
export const browserApi = {
  read: async () => parseBrowserState(await invoke<unknown>("get_browser_connection_state")),
  choose: (folder = false) => invoke<string | null>("choose_browser_extension", { folder }),
  import: (path: string, confirmedDisabled: boolean) => invoke<string>("import_browser_extension", { path, confirmedDisabled }),
  approve: (instance: string, context: string) => invoke<void>("approve_browser_pairing", { instance, context }),
  revoke: (instance: string) => invoke<void>("revoke_browser_pairing", { instance }),
  openDirectory: () => invoke<void>("open_browser_extension_directory"),
};
