import { MARKER, assertEmptyDraft } from "./core";
export interface MessageSnapshot { message: string; branch: string; text: string | null }
// DOM interpretation is isolated here. Missing stable IDs or unfamiliar markup
// cannot create an executable candidate.
export function assistants(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-message-author-role="assistant"]'));
}
export function messageId(node: HTMLElement): string { return node.dataset.messageId || node.closest<HTMLElement>("[data-message-id]")?.dataset.messageId || ""; }
export function branchId(node: HTMLElement): string {
  const all = Array.from(document.querySelectorAll<HTMLElement>("[data-message-author-role]"));
  const index = all.indexOf(node);
  for (let position = index - 1; position >= 0; position--) if (all[position].dataset.messageAuthorRole === "user") return messageId(all[position]);
  return "";
}
export function candidateText(node: HTMLElement): string | null {
  const codes = Array.from(node.querySelectorAll<HTMLElement>("pre code"));
  const candidates = codes.map(code => code.textContent?.trim() || "").filter(text => text.includes(MARKER));
  if (candidates.length !== 1 || node.querySelector("blockquote")) return null;
  const body = node.querySelector<HTMLElement>(".markdown");
  if (!body || body.querySelectorAll("pre").length !== 1) return null;
  const prose = Array.from(body.children).filter(child => !child.querySelector("pre") && child.tagName !== "PRE");
  return prose.some(child => child.textContent?.trim()) ? null : candidates[0];
}
export function streaming(): boolean { return Boolean(document.querySelector('[data-testid="stop-button"]')); }
export function compatible(): boolean { return Boolean(document.querySelector('#prompt-textarea[contenteditable="true"],textarea#prompt-textarea')); }
export function fill(text: string): void {
  if (streaming()) throw new Error("模型仍在输出，请稍后回填");
  const node = document.querySelector<HTMLElement>('#prompt-textarea[contenteditable="true"],textarea#prompt-textarea');
  if (!node) throw new Error("网页输入框不兼容，请复制结果手动发送");
  const old = node instanceof HTMLTextAreaElement ? node.value : node.innerText;
  assertEmptyDraft(old);
  node.focus();
  if (node instanceof HTMLTextAreaElement) {
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set;
    if (!setter) throw new Error("输入框无法写入，请复制结果");
    setter.call(node, text); node.dispatchEvent(new Event("input", { bubbles: true }));
  } else {
    const selection = window.getSelection();
    const range = document.createRange(); range.selectNodeContents(node);
    selection?.removeAllRanges(); selection?.addRange(range);
    if (!document.execCommand("insertText", false, text)) throw new Error("网页拒绝填入，请复制结果手动发送");
    node.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
  }
  const actual = node instanceof HTMLTextAreaElement ? node.value : node.innerText;
  if (actual.trim() !== text.trim()) throw new Error("填入状态无法确认；请检查输入框，结果已保留");
}
export class ObservationGate {
  private baseline = new Set<string>();
  private seen = new Set<string>();
  private stable = new Map<string, { text: string; branch: string; count: number }>();
  enable(existing: string[]): void { this.baseline = new Set(existing); this.seen.clear(); this.stable.clear(); }
  observe(messages: MessageSnapshot[], isStreaming: boolean): MessageSnapshot[] {
    if (isStreaming) { this.stable.clear(); return []; }
    const observed: MessageSnapshot[] = [];
    for (const item of messages) {
      if (!item.message || this.baseline.has(item.message)) continue;
      if (!item.text || !item.branch) { this.stable.delete(item.message); continue; }
      const previous = this.stable.get(item.message);
      const count = previous?.text === item.text && previous.branch === item.branch ? previous.count + 1 : 0;
      this.stable.set(item.message, { text: item.text, branch: item.branch, count });
      const key = item.message + "\n" + item.branch + "\n" + item.text;
      if (count >= 2 && !this.seen.has(key)) { this.seen.add(key); observed.push(item); }
    }
    return observed;
  }
}
