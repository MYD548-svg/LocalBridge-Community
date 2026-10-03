import identity from "./identity";
import { Candidate, Observation, Reassembler, Tool, canonical, fragments, instructions, parseCandidate, record, resultText, validPageSender } from "./core";
interface ChatState { chat: string; status: string; enabled: boolean; tools: Tool[]; selected: string[]; candidate?: Candidate; result?: unknown; error?: string; activeId?: string }
class NativeChannel {
  private port: any;
  private assembly = new Reassembler();
  private pending = new Map<string, { resolve: (value: any) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  private closed = false;
  constructor(private onClose: () => void, private onNotification: (payload: Record<string, unknown>) => void) {
    this.port = chrome.runtime.connectNative(identity.host);
    this.port.onMessage.addListener((frame: unknown) => {
      try {
        const message = this.assembly.push(frame);
        if (!record(message)) return;
        const payload = record(message.payload) ? message.payload : null;
        if (message.type === "mcp" && payload && typeof payload.method === "string" && payload.id === undefined) this.onNotification(payload);
        const id = String(message.requestId || (message.type === "mcp" ? payload?.id : message.rpcId) || "");
        const waiter = this.pending.get(id);
        if (waiter) {
          clearTimeout(waiter.timer); this.pending.delete(id);
          if (message.type === "error") waiter.reject(new Error(String(message.error)));
          else waiter.resolve(message.type === "mcp" ? payload : message.payload);
        }
        if (message.type === "disconnected") this.disconnect();
      } catch { this.disconnect(); }
    });
    this.port.onDisconnect.addListener(() => {
      // Read runtime.lastError in its callback, but do not log credentials.
      const error = chrome.runtime.lastError?.message;
      this.fail(error || "本机连接已关闭；请求不会自动重放");
    });
  }
  private fail(message: string): void {
    if (this.closed) return; this.closed = true;
    for (const waiter of this.pending.values()) { clearTimeout(waiter.timer); waiter.reject(new Error(message)); }
    this.pending.clear(); this.onClose();
  }
  send(message: unknown): void {
    if (this.closed) throw new Error("本机连接已关闭");
    for (const frame of fragments(message, crypto.randomUUID())) this.port.postMessage(frame);
  }
  request(message: Record<string, unknown>, id: string, timeout = 10_000): Promise<any> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error("响应超时，执行结果未知，禁止自动重放")); }, timeout);
      this.pending.set(id, { resolve, reject, timer });
      try { this.send(message); } catch (error) { clearTimeout(timer); this.pending.delete(id); reject(error); }
    });
  }
  rpc(method: string, params?: unknown, observation?: Observation, timeout = 10_000, id: string = crypto.randomUUID()): Promise<any> {
    return this.request({ version: 1, type: "mcp", payload: { jsonrpc: "2.0", id, method, ...(params === undefined ? {} : { params }) }, observation }, id, timeout);
  }
  disconnect(): void { try { this.port.disconnect(); } finally { this.fail("本机连接已关闭"); } }
}
const chats = new Map<number, ChatState>();
const history = new Map<string, ChatState>();
const channels = new Map<number, NativeChannel>();
let persistence = Promise.resolve();
function persist(): Promise<void> {
  persistence = persistence.catch(() => undefined).then(() => chrome.storage.local.set({
    chats: Object.fromEntries(Array.from(chats).map(([tab, chat]) => [tab, { ...chat, result: typeof chat.result === "string" ? chat.result : chat.result ? resultText(chat.result) : undefined }])),
    history: Object.fromEntries(Array.from(history).map(([key, chat]) => [key, { chat: chat.chat, enabled: false, tools: [], selected: [], status: chat.status === "running" ? "unknown" : chat.status, result: typeof chat.result === "string" ? chat.result : chat.result ? resultText(chat.result) : undefined, error: chat.error }])),
  }));
  return persistence;
}
const ready = (async () => {
  await chrome.storage.local.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
  const saved = await chrome.storage.local.get(["chats", "identity", "history"]);
  if (record(saved.history)) for (const [key, value] of Object.entries(saved.history)) if (record(value) && typeof value.chat === "string") {
    history.set(key, { chat: value.chat, enabled: false, tools: [], selected: [], status: value.status === "running" || value.status === "unknown" ? "unknown" : "paused", result: value.result, error: "历史结果已保留；请重新连接当前聊天" });
  }
  if (record(saved.chats)) for (const [tab, value] of Object.entries(saved.chats)) if (record(value) && typeof value.chat === "string") {
    chats.set(Number(tab), { chat: value.chat, enabled: false, tools: [], selected: [],
      status: value.status === "running" || value.status === "unknown" ? "unknown" : "paused", result: value.result, error: "后台已恢复，请重新连接并启用；请求不会自动重放" });
  }
  if (!record(saved.identity)) {
    const random = crypto.getRandomValues(new Uint8Array(32));
    await chrome.storage.local.set({ identity: { instance: crypto.randomUUID(), secret: Array.from(random, byte => byte.toString(16).padStart(2, "0")).join("") } });
  }
})();
async function credentials(): Promise<{ instance: string; secret: string }> {
  const value = (await chrome.storage.local.get("identity")).identity;
  if (!record(value) || typeof value.instance !== "string" || typeof value.secret !== "string") throw new Error("扩展身份不可用，请重置配对");
  return { instance: value.instance, secret: value.secret };
}
async function page(tab: number, request: Record<string, unknown>): Promise<any> {
  const response = await chrome.tabs.sendMessage(tab, request, { frameId: 0 });
  if (!response?.ok) throw new Error(response?.error || "请打开或刷新 ChatGPT 网页");
  return response;
}
function channel(tab: number): NativeChannel {
  let transport = channels.get(tab);
  if (!transport) {
    transport = new NativeChannel(() => {
      channels.delete(tab);
      const chat = chats.get(tab);
      if (chat) { chat.status = chat.status === "running" ? "unknown" : "disconnected"; chat.enabled = false; chat.error = "本机连接已关闭，请检查 LocalBridge；未确认结果不会重放"; void persist(); void page(tab, { type: "disable" }).catch(() => undefined); }
    }, payload => {
      const chat = chats.get(tab);
      if (chat && payload.method === "notifications/tools/list_changed") {
        chat.enabled = false; chat.candidate = undefined; chat.error = "工具目录已变化，请断开后重新启用当前聊天";
        void persist(); void page(tab, { type: "disable" }).catch(() => undefined);
      }
    });
    channels.set(tab, transport);
  }
  return transport;
}
function current(tab: number): ChatState {
  const value = chats.get(tab);
  if (!value) throw new Error("请先连接当前聊天");
  return value;
}
async function uiAction(tab: number, request: Record<string, unknown>): Promise<unknown> {
  if (request.type === "reset") {
    for (const transport of channels.values()) transport.disconnect();
    channels.clear(); chats.clear(); history.clear();
    const random = crypto.getRandomValues(new Uint8Array(32));
    await chrome.storage.local.set({ identity: { instance: crypto.randomUUID(), secret: Array.from(random, byte => byte.toString(16).padStart(2, "0")).join("") }, chats: {}, history: {} });
    return { status: "reset" };
  }
  const binding = await page(tab, { type: "binding" });
  let chat = chats.get(tab);
  if (chat && chat.chat !== binding.chat) {
    if (history.size >= 128 && !history.has(tab + ":" + chat.chat)) throw new Error("历史聊天已达保存上限，请复制所需结果后重置配对");
    channels.get(tab)?.disconnect();
    history.set(tab + ":" + chat.chat, { ...chat, enabled: false, candidate: undefined, activeId: undefined });
    chat = history.get(tab + ":" + binding.chat); chats.delete(tab);
    if (chat) { chat = { ...chat, enabled: false, tools: [], selected: [], candidate: undefined }; chats.set(tab, chat); }
    await persist();
  }
  if (!chat) {
    const previous = history.get(tab + ":" + binding.chat);
    if (previous) { chat = { ...previous, enabled: false, tools: [], selected: [], candidate: undefined }; chats.set(tab, chat); }
  }
  if (request.type === "read") return chat || { status: "not_connected", chat: binding.chat, tools: [], selected: [], enabled: false };
  if (request.type === "connect" || request.type === "enable") {
    if (!binding.compatible) throw new Error("网页结构不兼容，请刷新页面");
    if (!chat) { chat = { chat: binding.chat, status: "connecting", enabled: false, tools: [], selected: [] }; chats.set(tab, chat); }
    const native = channel(tab), requestId = crypto.randomUUID();
    const response = await native.request({ version: 1, type: request.type === "connect" ? "pair" : "enable", requestId,
      ...await credentials(), chat: binding.chat, startApp: request.type === "connect" }, requestId);
    chat.status = response?.approved ? "paired" : "awaiting_pairing";
    chat.error = response?.approved ? undefined : "请在 LocalBridge 的 ChatGPT 网页卡片确认工作区与权限，然后点击启用";
    if (request.type === "enable" && response?.approved) {
      const initialized = await native.rpc("initialize", { protocolVersion: "2025-11-25", capabilities: {}, clientInfo: { name: "LocalBridge ChatGPT Web", version: "0.1.5" } });
      if (initialized.error) throw new Error(JSON.stringify(initialized.error));
      native.send({ version: 1, type: "mcp", payload: { jsonrpc: "2.0", method: "notifications/initialized" } });
      const catalog = await native.rpc("tools/list");
      if (!Array.isArray(catalog?.result?.tools)) throw new Error("工具目录不可用");
      chat.tools = catalog.result.tools.filter((tool: unknown): tool is Tool => record(tool) && typeof tool.name === "string" && typeof tool.description === "string" && record(tool.inputSchema));
      chat.selected = chat.tools.filter(tool => tool.name !== "view_image").map(tool => tool.name);
      chat.enabled = true; chat.status = "enabled"; chat.candidate = undefined;
      await page(tab, { type: "enable", chat: binding.chat });
    }
    await persist(); return chat;
  }
  chat = current(tab);
  if (request.type === "select") {
    if (!Array.isArray(request.names) || !request.names.every(name => typeof name === "string" && chat!.tools.some(tool => tool.name === name))) throw new Error("工具选择无效");
    chat.selected = request.names as string[]; chat.candidate = undefined; await persist(); return chat;
  }
  if (request.type === "instructions") {
    if (!chat.enabled || !chat.selected.length) throw new Error("请启用聊天并选择工具");
    await page(tab, { type: "fill", chat: chat.chat, text: instructions(chat.tools.filter(tool => chat!.selected.includes(tool.name))) });
    return chat;
  }
  if (request.type === "disconnect") {
    channels.get(tab)?.disconnect(); await page(tab, { type: "disable" }); await persist(); return current(tab);
  }
  if (request.type === "cancel") {
    const native = channels.get(tab);
    if (native && chat.activeId) native.send({ version: 1, type: "mcp", payload: { jsonrpc: "2.0", method: "notifications/cancelled", params: { requestId: chat.activeId } } });
    chat.error = "取消已请求；等待实际终止结果"; await persist(); return chat;
  }
  if (request.type === "execute") {
    if (!chat.enabled || !chat.candidate || chat.status === "running" || chat.status === "unknown") throw new Error("没有可确认的请求；结果未知时不能重放");
    const live = await page(tab, { type: "candidate" });
    const candidate = parseCandidate(live.text, live.observation, chat.tools.filter(tool => chat!.selected.includes(tool.name)));
    if (live.chat !== chat.chat || canonical(candidate) !== canonical(chat.candidate)) throw new Error("聊天或工具参数已变化，请重新查看");
    if (current(tab) !== chat || !chat.enabled || chat.status === "running" || chat.status === "unknown") throw new Error("聊天请求已变化，请重新查看");
    const native = channels.get(tab);
    if (!native) throw new Error("本机连接未建立");
    chat.status = "running"; chat.activeId = crypto.randomUUID(); chat.error = undefined; chat.result = undefined;
    await persist(); // Never submit without recording the uncertain operation first.
    try {
      const result = await native.rpc("tools/call", { name: candidate.name, arguments: candidate.arguments }, candidate.observation, 610_000, chat.activeId);
      chat.result = result; chat.candidate = undefined;
      chat.status = result.error?.code === -32011 ? "unknown" : result.error || result.result?.isError ? "failed" : "succeeded";
    } catch (error) { chat.status = "unknown"; chat.error = String(error); }
    chat.activeId = undefined; await persist(); return chat;
  }
  if (request.type === "fill") {
    if (!chat.result) throw new Error("没有可回填的真实结果");
    const text = typeof chat.result === "string" ? chat.result : resultText(chat.result);
    await page(tab, { type: "fill", chat: chat.chat, text }); return chat;
  }
  if (request.type === "copy") return { text: typeof chat.result === "string" ? chat.result : chat.result ? resultText(chat.result) : "" };
  throw new Error("扩展操作不支持");
}
chrome.runtime.onMessage.addListener((request: unknown, sender: any, respond: (response: unknown) => void) => {
  void ready.then(async () => {
    if (!record(request) || typeof request.type !== "string") throw new Error("扩展消息格式无效");
    if (sender.url === chrome.runtime.getURL("popup.html") && sender.id === chrome.runtime.id) {
      if (!Number.isSafeInteger(request.tab)) throw new Error("请选择 ChatGPT 标签页");
      return uiAction(Number(request.tab), request);
    }
    if (!validPageSender(sender, chrome.runtime.id) || !Number.isSafeInteger(sender.tab?.id)) throw new Error("网页来源被拒绝");
    const tab = Number(sender.tab.id), chat = chats.get(tab);
    if (request.type === "navigation") {
      channels.get(tab)?.disconnect();
      if (chat) { chat.enabled = false; if (chat.status !== "unknown") chat.status = "paused"; chat.candidate = undefined; }
      await persist(); return null;
    }
    if (request.type !== "observe" || !chat?.enabled || request.chat !== chat.chat || typeof request.text !== "string" || !record(request.observation)) throw new Error("聊天未启用或消息无效");
    if (chat.status === "running" || chat.status === "unknown") return null;
    const candidate = parseCandidate(request.text, request.observation as unknown as Observation, chat.tools.filter(tool => chat.selected.includes(tool.name)));
    chat.candidate = candidate; chat.status = "pending"; chat.error = undefined; await persist(); return null;
  }).then(value => respond({ ok: true, value }), error => respond({ ok: false, error: String(error) }));
  return true;
});
chrome.tabs.onRemoved.addListener((tab: number) => { channels.get(tab)?.disconnect(); chats.delete(tab); void persist(); });
