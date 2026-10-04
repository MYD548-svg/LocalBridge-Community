import { describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { browserApi, parseBrowserState, parseExtensionDownload } from "../features/connection/browserApi";
import { guideStep, mayPrepare } from "../features/connection/browserGuide";
const state = { protocol: 1, extensionId: "a".repeat(32), directory: "C:\\测试", preparedVersion: null,
  applicationVersion: "0.1.5", repository: "MYD548-svg/LocalBridge-Community", bundle: { available: true, version: "0.1.5", message: "ready" },
  serviceReady: true, connectedBrowsers: 0, connectedChats: 0, successfulCalls: 0, pairings: [] };
describe("browser connection state and explicit user actions", () => {
  it("does not confuse prepared files with connected chats or real calls", () => {
    expect(parseBrowserState({ ...state, preparedVersion: "0.1.5" }).successfulCalls).toBe(0);
    for (const change of [{ protocol: 2 }, { extensionId: "*" }, { connectedChats: -1 }, { pairings: [{}] }]) expect(() => parseBrowserState({ ...state, ...change })).toThrow();
  });
  it("passes the explicit disabled-extension acknowledgment and pairing identity", async () => {
    invoke.mockResolvedValue({ version: "0.1.5", alreadyPrepared: false });
    await browserApi.import("C:\\包.zip", true);
    expect(invoke).toHaveBeenLastCalledWith("import_browser_extension", { path: "C:\\包.zip", confirmedDisabled: true });
    await browserApi.revoke("instance");
    expect(invoke).toHaveBeenLastCalledWith("revoke_browser_pairing", { instance: "instance" });
    await browserApi.approve("instance", "b".repeat(64));
    expect(invoke).toHaveBeenLastCalledWith("approve_browser_pairing", { instance: "instance", context: "b".repeat(64) });
  });
  it("allows first preparation without closing a nonexistent extension and guards replacements", async () => {
    expect(mayPrepare(state, false)).toBe(true);
    expect(mayPrepare({ ...state, preparedVersion: "0.1.5" }, false)).toBe(false);
    expect(mayPrepare({ ...state, preparedVersion: "0.1.5" }, true)).toBe(true);
    expect(mayPrepare({ ...state, connectedBrowsers: 1 }, true)).toBe(false);
    invoke.mockResolvedValue({ version: "0.1.5", alreadyPrepared: true });
    expect((await browserApi.prepareBundled(false)).alreadyPrepared).toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("prepare_bundled_browser_extension", { confirmedDisabled: false });
  });
  it("global browser, chat and call counts cannot complete the current-tab guide", () => {
    const connected = { ...state, preparedVersion: "0.1.5", connectedBrowsers: 3, connectedChats: 8, successfulCalls: 90 };
    expect(guideStep(connected, 0)).toBe(3);
    expect(guideStep({ ...connected, serviceReady: false }, 6)).toBe(1);
    expect(guideStep({ ...state, preparedVersion: null }, 6)).toBe(2);
    expect(guideStep(connected, 2)).toBe(2);
  });
  it("no release offers no URL, and a product ZIP cannot redirect to another project", () => {
    const unavailable = { state: "no_release", message: "尚未发布", repository: state.repository, applicationVersion: "0.1.5", extension: null, url: null, tag: null };
    expect(parseExtensionDownload(unavailable).url).toBeNull();
    expect(() => parseExtensionDownload({ ...unavailable, url: "https://github.com/other/project/releases" })).toThrow();
    const name = "LocalBridge-ChatGPT-Web-v0.1.5.zip";
    const info = { ...unavailable, state: "available", tag: "v0.1.5-community.1", url: `https://github.com/${state.repository}/releases/download/v0.1.5-community.1/${name}`,
      extension: { version: "0.1.5", protocol: 1, extensionId: state.extensionId, application: ">=0.1.5, <0.2.0", asset: { name, sha256: "b".repeat(64), size: 100 } } };
    expect(parseExtensionDownload(info).extension?.asset.name).toBe(name);
    expect(() => parseExtensionDownload({ ...info, url: "https://github.com/zephyr7030/LocalBridge/releases/download/v0.1.5/extension.zip" })).toThrow();
  });
});
