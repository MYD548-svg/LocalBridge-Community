import { describe, expect, it } from "vitest";
import { FRAME_LIMIT, MARKER, Reassembler, assertEmptyDraft, canonical, chatIdentity, fragments, parseCandidate, resultText, validPageSender } from "./core";
const tools = [{ name: "filesystem", description: "read", inputSchema: { type: "object", properties: { path: { type: "string", minLength: 1 } }, required: ["path"], additionalProperties: false } }];
const observation = { message: "assistant-1", branch: "user-1", index: 0 };
describe("browser protocol and untrusted assistant candidates", () => {
  it("roundtrips Chinese paths and fragmented structured results", () => {
    const value = { path: "C:\\测试 空格", text: "中文".repeat(220_000), error: { code: "test" }, output_refs: { stdout: "retained" } };
    const frames = fragments(value, "transfer");
    expect(frames.length).toBeGreaterThan(1);
    const assembler = new Reassembler();
    let result: unknown;
    for (const frame of frames) {
      expect(new TextEncoder().encode(JSON.stringify(frame)).length).toBeLessThanOrEqual(FRAME_LIMIT);
      result = assembler.push(frame);
    }
    expect(result).toEqual(value);
  });
  it("rejects malformed, duplicate, out of order, oversized and expired fragments", () => {
    const frames = fragments({ text: "x".repeat(400_000) }, "transfer");
    expect(() => new Reassembler().push(frames[1])).toThrow();
    expect(() => new Reassembler().push(frames[0], 12)).toThrow();
    const duplicate = new Reassembler(); duplicate.push(frames[0]);
    expect(() => duplicate.push(frames[0])).toThrow();
    const expired = new Reassembler(); expired.push(frames[0], undefined, 0);
    expect(() => expired.push(frames[1], undefined, 16_000)).toThrow();
  });
  it("accepts only marked discovered tools with valid arguments", () => {
    const text = JSON.stringify({ [MARKER]: { name: "filesystem", arguments: { path: "中文" } } });
    expect(parseCandidate(text, observation, tools).arguments.path).toBe("中文");
    for (const value of [{ name: "filesystem", arguments: {} }, { [MARKER]: { name: "exec", arguments: {} } },
      { [MARKER]: { name: "filesystem", arguments: { path: "" } } }, { [MARKER]: { name: "filesystem", arguments: { path: "x", extra: 1 } } }]) {
      expect(() => parseCandidate(JSON.stringify(value), observation, tools)).toThrow();
    }
    expect(() => parseCandidate(text, { ...observation, message: "" }, tools)).toThrow();
  });
  it("binds only top-level ChatGPT senders and isolates temporary pages", () => {
    expect(validPageSender({ id: "extension", frameId: 0, url: "https://chatgpt.com/c/a" }, "extension")).toBe(true);
    expect(validPageSender({ id: "extension", frameId: 1, url: "https://chatgpt.com/" }, "extension")).toBe(false);
    expect(validPageSender({ id: "extension", frameId: 0, url: "https://chatgpt.com.evil.invalid/" }, "extension")).toBe(false);
    expect(chatIdentity("https://chatgpt.com/c/abc", "page")).toBe("chat:abc");
    expect(chatIdentity("https://chatgpt.com/", "first")).not.toBe(chatIdentity("https://chatgpt.com/", "second"));
  });
  it("protects drafts and preserves explicit large-result limits", () => {
    expect(() => assertEmptyDraft("尚未发送")).toThrow();
    expect(() => assertEmptyDraft("")).not.toThrow();
    expect(resultText({ result: { content: [{ text: "x".repeat(50_000) }], structuredContent: { output_refs: { stdout: "ref" } } } })).toContain("ref");
    expect(canonical({ b: 1, a: 2 })).toBe(canonical({ a: 2, b: 1 }));
  });
});
