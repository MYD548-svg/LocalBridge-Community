import type { BrowserState } from "./browserApi";
export const browserSteps = ["准备本机", "准备扩展", "浏览器加载", "批准配对", "启用聊天", "首次调用"];
export function guideStep(state: BrowserState | null, requested: number): number {
  if (!state?.serviceReady) return 1;
  if (!state.preparedVersion) return requested === 1 ? 1 : 2;
  // Global counters cannot establish which browser/tab the user is setting up.
  return requested >= 1 && requested <= 6 ? requested : 3;
}
export function mayPrepare(state: BrowserState | null, confirmedDisabled: boolean): boolean {
  return Boolean(state && state.connectedBrowsers === 0 && state.connectedChats === 0 && (!state.preparedVersion || confirmedDisabled));
}
