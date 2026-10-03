import { chatIdentity, record } from "./core";
import { ObservationGate, assistants, branchId, candidateText, compatible, fill, messageId, streaming } from "./page-adapter";
const documentId = crypto.randomUUID();
let chat = chatIdentity(location.href, documentId);
let enabled = false;
const gate = new ObservationGate();
let latest: { text: string; observation: { message: string; branch: string; index: number } } | null = null;
const panel = document.createElement("div");
panel.id = "localbridge-web-status";
const shadow = panel.attachShadow({ mode: "closed" });
const style = document.createElement("style");
style.textContent = ":host{position:fixed;right:16px;bottom:16px;z-index:2147483647;font:13px system-ui;max-width:320px}div{background:#eef5ff;color:#17325a;border:1px solid #aac3e8;padding:12px;border-radius:12px;box-shadow:0 4px 20px #0002}pre{white-space:pre-wrap;max-height:160px;overflow:auto;font-size:12px}";
const status = document.createElement("div");
status.textContent = "LocalBridge：点击浏览器工具栏扩展图标连接";
shadow.append(style, status);
document.documentElement.append(panel);
chrome.runtime.onMessage.addListener((request: unknown, sender: { id?: string }, respond: (value: unknown) => void) => {
  if (sender.id !== chrome.runtime.id || !record(request)) return;
  try {
    refreshChat();
    if (request.type === "binding") respond({ ok: true, chat, compatible: compatible() });
    else if (request.type === "enable" && request.chat === chat) {
      enabled = true; gate.enable(assistants().map(messageId)); latest = null;
      status.textContent = "当前聊天已启用；执行确认请在扩展窗口操作";
      respond({ ok: true, chat });
    } else if (request.type === "disable") { enabled = false; latest = null; status.textContent = "当前聊天已暂停"; respond({ ok: true }); }
    else if (request.type === "candidate") {
      const node = assistants().find(node => messageId(node) === latest?.observation.message);
      const text = node ? candidateText(node) : null;
      const isStreaming = streaming();
      if (!enabled || !latest || text !== latest.text || !node || branchId(node) !== latest.observation.branch || isStreaming) throw new Error("助手消息或分支已变化，请重新确认");
      respond({ ok: true, chat, ...latest });
    } else if (request.type === "fill" && request.chat === chat && typeof request.text === "string") {
      fill(request.text); status.textContent = "结果已填入草稿，请确认后在 ChatGPT 点击发送"; respond({ ok: true });
    } else throw new Error("聊天绑定已变化");
  } catch (error) { respond({ ok: false, error: String(error) }); }
});
function refreshChat(): void {
  const next = chatIdentity(location.href, documentId);
  if (next !== chat) {
    chat = next; enabled = false; latest = null; gate.enable([]);
    status.textContent = "聊天已切换，请重新启用 LocalBridge";
    void chrome.runtime.sendMessage({ type: "navigation", chat });
  }
}
setInterval(() => {
  refreshChat();
  if (!enabled) return;
  if (!compatible()) {
    enabled = false; status.textContent = "网页结构不兼容，已暂停；结果仍保留在扩展中";
    void chrome.runtime.sendMessage({ type: "navigation", chat }); return;
  }
  const messages = assistants().map(node => ({ message: messageId(node), branch: branchId(node), text: candidateText(node) }));
  for (const observed of gate.observe(messages, streaming())) {
    latest = { text: observed.text!, observation: { message: observed.message, branch: observed.branch, index: 0 } };
    status.textContent = "发现工具请求；请打开扩展查看参数并确认执行";
    void chrome.runtime.sendMessage({ type: "observe", chat, ...latest });
  }
}, 500);
