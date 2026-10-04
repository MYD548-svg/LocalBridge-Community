import { invoke } from "@tauri-apps/api/core";
import { isRecord } from "../../bridge";
export interface BrowserPairing { instance: string; workspace: string; permission: string; approved: boolean; revoked: boolean; context: string }
export interface BrowserState { protocol: number; extensionId: string; directory: string; preparedVersion: string | null;
  applicationVersion: string; repository: string; bundle: { available: boolean; version: string | null; message: string };
  serviceReady: boolean; connectedBrowsers: number; connectedChats: number; successfulCalls: number; pairings: BrowserPairing[] }
export interface PrepareResult { version: string; alreadyPrepared: boolean }
export interface ExtensionDownload { state: "available" | "no_release" | "missing_asset" | "incompatible" | "network_error" | "invalid_release";
  message: string; repository: string; applicationVersion: string; url: string | null; tag: string | null;
  extension: null | { version: string; protocol: number; extensionId: string; application: string; asset: { name: string; sha256: string; size: number } } }
const version = (value: unknown): value is string => typeof value === "string" && /^\d+\.\d+\.\d+$/.test(value);
const repository = (value: unknown): value is string => typeof value === "string" && /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(value);
export function parseBrowserState(value: unknown): BrowserState {
  if (!isRecord(value) || value.protocol !== 1 || typeof value.extensionId !== "string" || !/^[a-p]{32}$/.test(value.extensionId)
    || typeof value.directory !== "string" || !(value.preparedVersion === null || version(value.preparedVersion))
    || !version(value.applicationVersion) || !repository(value.repository) || !isRecord(value.bundle)
    || typeof value.bundle.available !== "boolean" || typeof value.bundle.message !== "string"
    || !(value.bundle.version === null || version(value.bundle.version)) || value.bundle.available !== (value.bundle.version !== null)
    || typeof value.serviceReady !== "boolean" || !Number.isSafeInteger(value.connectedChats) || Number(value.connectedChats) < 0
    || !Number.isSafeInteger(value.connectedBrowsers) || Number(value.connectedBrowsers) < 0
    || !Number.isSafeInteger(value.successfulCalls) || Number(value.successfulCalls) < 0 || !Array.isArray(value.pairings)
    || !value.pairings.every(pair => isRecord(pair) && typeof pair.instance === "string" && typeof pair.workspace === "string"
      && typeof pair.permission === "string" && typeof pair.approved === "boolean" && typeof pair.revoked === "boolean" && typeof pair.context === "string" && /^[a-f0-9]{64}$/.test(pair.context))) {
    throw new Error("浏览器连接状态合同不兼容");
  }
  return value as unknown as BrowserState;
}
export function parsePrepareResult(value: unknown): PrepareResult {
  if (!isRecord(value) || !version(value.version) || typeof value.alreadyPrepared !== "boolean") throw new Error("扩展准备状态不兼容");
  return value as unknown as PrepareResult;
}
export function parseExtensionDownload(value: unknown): ExtensionDownload {
  if (!isRecord(value) || !["available", "no_release", "missing_asset", "incompatible", "network_error", "invalid_release"].includes(String(value.state))
    || typeof value.message !== "string" || !repository(value.repository) || !version(value.applicationVersion)) throw new Error("扩展下载信息不兼容");
  if (value.state === "available") {
    const extension = value.extension;
    if (!isRecord(extension) || !version(extension.version) || extension.protocol !== 1 || typeof extension.extensionId !== "string"
      || !/^[a-p]{32}$/.test(extension.extensionId) || typeof extension.application !== "string" || !isRecord(extension.asset)
      || extension.asset.name !== `LocalBridge-ChatGPT-Web-v${extension.version}.zip` || typeof extension.asset.sha256 !== "string"
      || !/^[a-f0-9]{64}$/.test(extension.asset.sha256) || !Number.isSafeInteger(extension.asset.size) || Number(extension.asset.size) <= 0
      || Number(extension.asset.size) > 32 * 1024 * 1024
      || typeof value.tag !== "string" || !/^[A-Za-z0-9_.-]+$/.test(value.tag)
      || value.url !== `https://github.com/${value.repository}/releases/download/${value.tag}/${extension.asset.name}`) {
      throw new Error("扩展附件的版本、来源或校验信息不兼容");
    }
  } else if (value.extension !== null || value.url !== null || value.tag !== null) throw new Error("不可用的下载不能提供附件");
  return value as unknown as ExtensionDownload;
}
export const browserApi = {
  read: async () => parseBrowserState(await invoke<unknown>("get_browser_connection_state")),
  choose: (folder = false) => invoke<string | null>("choose_browser_extension", { folder }),
  import: async (path: string, confirmedDisabled: boolean) => parsePrepareResult(await invoke<unknown>("import_browser_extension", { path, confirmedDisabled })),
  prepareBundled: async (confirmedDisabled = false) => parsePrepareResult(await invoke<unknown>("prepare_bundled_browser_extension", { confirmedDisabled })),
  downloadInfo: async () => parseExtensionDownload(await invoke<unknown>("get_browser_extension_download")),
  openDownload: async () => parseExtensionDownload(await invoke<unknown>("open_browser_extension_download")),
  approve: (instance: string, context: string) => invoke<void>("approve_browser_pairing", { instance, context }),
  revoke: (instance: string) => invoke<void>("revoke_browser_pairing", { instance }),
  openDirectory: () => invoke<void>("open_browser_extension_directory"),
};
