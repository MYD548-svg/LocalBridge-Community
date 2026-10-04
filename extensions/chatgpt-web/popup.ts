import { record, resultText } from "./core";
import { popupGuide } from "./popupGuide";
declare const __LOCALBRIDGE_RELEASES_URL__: string;
let tab = -1;
let lastValue: Record<string, unknown> | null = null;
let renderedTools = "";
let operationError = "";
let readError = "";
const pending = new Set<string>();
const element = (id: string) => document.getElementById(id)!;
const labels: Record<string, string> = { not_connected: "尚未连接", connecting: "正在连接", paired: "浏览器已配对，当前聊天尚未启用",
  awaiting_pairing: "等待应用批准配对", enabled: "当前聊天已启用", pending: "发现待确认请求", running: "正在执行",
  succeeded: "已完成真实调用", failed: "实际调用失败", unknown: "结果未知，禁止自动重放", paused: "聊天已暂停", disconnected: "连接已关闭", reset: "配对身份已重置" };
function render(value: unknown): void {
  if (!record(value)) return;
  lastValue = value;
  const status = String(value.status);
  const error = operationError || readError || (typeof value.error === "string" ? value.error : "");
  const guide = popupGuide(status, error);
  element("status").textContent = labels[status] || status;
  element("guidance").textContent = guide.message;
  element("error").textContent = error;
  element("candidate").textContent = record(value.candidate) ? JSON.stringify({ 工具: value.candidate.name, 参数: value.candidate.arguments }, null, 2) : "等待新的助手工具请求";
  element("result").textContent = value.result ? typeof value.result === "string" ? value.result : resultText(value.result) : "尚未执行";
  const locked = status === "running" || status === "unknown";
  const disabled: Record<string, boolean> = {
    connect: locked || pending.has("execute"), enable: locked || value.enabled === true || status === "not_connected" || status === "connecting",
    "first-call": !value.enabled || locked || status === "pending" || pending.has("execute"), instructions: !value.enabled || locked,
    execute: status !== "pending", cancel: status !== "running", fill: !value.result || locked, copy: !value.result,
    disconnect: status === "not_connected" || status === "disconnected" || status === "reset",
  };
  for (const [id, unavailable] of Object.entries(disabled)) {
    const button = element(id) as HTMLButtonElement;
    button.disabled = unavailable || pending.has(id === "first-call" ? "first_call" : id);
    button.classList.toggle("primary", guide.primary === id);
  }
  element("enable").textContent = status === "awaiting_pairing" ? "我已批准，启用此聊天" : "为此聊天启用工具";
  const signature = JSON.stringify([value.tools, value.selected, locked]);
  if (renderedTools !== signature) {
    renderedTools = signature;
    const tools = element("tools"); tools.replaceChildren();
    if (Array.isArray(value.tools)) for (const item of value.tools) if (record(item) && typeof item.name === "string") {
      const label = document.createElement("label"), checkbox = document.createElement("input");
      checkbox.type = "checkbox"; checkbox.value = item.name; checkbox.disabled = locked; checkbox.checked = Array.isArray(value.selected) && value.selected.includes(item.name);
      checkbox.addEventListener("change", () => void action("select", { names: Array.from(tools.querySelectorAll<HTMLInputElement>("input:checked")).map(input => input.value) }));
      label.append(checkbox, document.createTextNode(" " + item.name)); tools.append(label);
    }
  }
}
async function action(type: string, extra = {}): Promise<void> {
  if (pending.has(type)) return;
  pending.add(type);
  if (type !== "read") { operationError = ""; element("action-notice").textContent = ""; }
  if (lastValue) render(lastValue);
  try {
    const response = await chrome.runtime.sendMessage({ type, tab, ...extra });
    if (!response?.ok) throw new Error(response?.error || "扩展操作失败");
    if (type === "copy") { await navigator.clipboard.writeText(response.value.text); element("action-notice").textContent = "结果已复制，请在聊天中确认发送"; }
    else { if (type === "read") readError = ""; render(response.value); }
  } catch (error) {
    if (type !== "read") operationError = String(error); else readError = String(error);
    element("error").textContent = operationError || String(error);
    element("guidance").textContent = popupGuide(lastValue ? String(lastValue.status) : "not_connected", String(error)).message;
  } finally { pending.delete(type); if (lastValue) render(lastValue); }
}
for (const name of ["connect","enable","instructions","execute","cancel","fill","copy","disconnect","reset","first-call"]) {
  element(name).addEventListener("click", () => {
    if (name === "reset" && !confirm("重置后需要在 LocalBridge 重新配对。继续？")) return;
    void action(name === "first-call" ? "first_call" : name);
  });
}
(element("release-link") as HTMLAnchorElement).href = __LOCALBRIDGE_RELEASES_URL__;
void (async () => {
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tabs[0]?.url?.startsWith("https://chatgpt.com/")) throw new Error("请先打开并登录 ChatGPT 网页，再点击扩展图标");
  tab = tabs[0].id;
  await action("read");
  setInterval(() => void action("read"), 1500);
})().catch(error => { element("error").textContent = String(error); });
