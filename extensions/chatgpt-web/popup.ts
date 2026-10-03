import { record, resultText } from "./core";
let tab = -1;
const element = (id: string) => document.getElementById(id)!;
const labels: Record<string, string> = { not_connected: "尚未连接", connecting: "正在连接", paired: "浏览器已配对，当前聊天尚未启用",
  awaiting_pairing: "请在 LocalBridge 确认配对", enabled: "当前聊天已启用", pending: "发现待确认请求", running: "正在执行",
  succeeded: "已完成真实调用", failed: "实际调用失败", unknown: "结果未知，禁止自动重放", paused: "聊天已暂停", disconnected: "连接已关闭", reset: "配对身份已重置" };
function render(value: unknown): void {
  if (!record(value)) return;
  element("status").textContent = labels[String(value.status)] || String(value.status);
  element("error").textContent = typeof value.error === "string" ? value.error : "";
  element("candidate").textContent = record(value.candidate) ? JSON.stringify({ 工具: value.candidate.name, 参数: value.candidate.arguments }, null, 2) : "等待新的助手工具请求";
  element("result").textContent = value.result ? typeof value.result === "string" ? value.result : resultText(value.result) : "尚未执行";
  (element("execute") as HTMLButtonElement).disabled = value.status !== "pending";
  (element("enable") as HTMLButtonElement).disabled = value.enabled === true;
  (element("fill") as HTMLButtonElement).disabled = !value.result;
  (element("cancel") as HTMLButtonElement).disabled = value.status !== "running";
  const tools = element("tools"); tools.replaceChildren();
  if (Array.isArray(value.tools)) for (const item of value.tools) if (record(item) && typeof item.name === "string") {
    const label = document.createElement("label"), checkbox = document.createElement("input");
    checkbox.type = "checkbox"; checkbox.value = item.name; checkbox.checked = Array.isArray(value.selected) && value.selected.includes(item.name);
    checkbox.addEventListener("change", () => void action("select", { names: Array.from(tools.querySelectorAll<HTMLInputElement>("input:checked")).map(input => input.value) }));
    label.append(checkbox, document.createTextNode(" " + item.name)); tools.append(label);
  }
}
async function action(type: string, extra = {}): Promise<void> {
  try {
    const response = await chrome.runtime.sendMessage({ type, tab, ...extra });
    if (!response?.ok) throw new Error(response?.error || "扩展操作失败");
    if (type === "copy") {
      await navigator.clipboard.writeText(response.value.text);
      element("status").textContent = "结果已复制，请在聊天中确认发送";
    } else render(response.value);
  } catch (error) { element("error").textContent = String(error); }
}
for (const name of ["connect","enable","instructions","execute","cancel","fill","copy","disconnect","reset"]) {
  element(name).addEventListener("click", () => {
    if (name === "reset" && !confirm("重置后需要在 LocalBridge 重新配对。继续？")) return;
    void action(name);
  });
}
void (async () => {
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tabs[0]?.url?.startsWith("https://chatgpt.com/")) throw new Error("请先打开 ChatGPT 网页，再点击扩展图标");
  tab = tabs[0].id;
  await action("read");
  setInterval(() => void action("read"), 1500);
})().catch(error => { element("error").textContent = String(error); });
