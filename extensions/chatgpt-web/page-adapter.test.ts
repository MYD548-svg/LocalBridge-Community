import { describe, expect, it } from "vitest";
import { ObservationGate, candidateText } from "./page-adapter";
describe("simulated page adapter observations", () => {
  it("ignores old messages and streaming until a new stable complete observation", () => {
    const gate = new ObservationGate();
    gate.enable(["old"]);
    const fresh = { message: "fresh", branch: "user", text: '{"localbridge-tool-v1":{}}' };
    const messages = [{ ...fresh, message: "old" }, fresh];
    expect(gate.observe(messages, true)).toEqual([]);
    expect(gate.observe(messages, false)).toEqual([]);
    expect(gate.observe(messages, false)).toEqual([]);
    expect(gate.observe(messages, false)).toEqual([fresh]);
    expect(gate.observe(messages, false)).toEqual([]);
    gate.enable(["old", "fresh"]);
    expect(gate.observe(messages, false)).toEqual([]);
  });
  it("does not combine an incomplete message or changed parameter snapshot with completion", () => {
    const gate = new ObservationGate(); gate.enable([]);
    const fresh = { message: "a", branch: "b", text: "first" };
    gate.observe([fresh], false); gate.observe([fresh], false);
    gate.observe([fresh], true);
    expect(gate.observe([fresh], false)).toEqual([]);
    expect(gate.observe([{ ...fresh, text: "changed" }], false)).toEqual([]);
    expect(gate.observe([{ ...fresh, branch: "" }], false)).toEqual([]);
    expect(gate.observe([{ ...fresh, message: "" }], false)).toEqual([]);
  });
  it("rejects quoted requests, prose examples, multiple blocks and missing page structure", () => {
    const text = '{"localbridge-tool-v1":{}}';
    function node({ quoted = false, prose = false, blocks = 1, body = true } = {}): HTMLElement {
      const pre = { tagName: "PRE", textContent: text, querySelector: () => null };
      const paragraph = { tagName: "P", textContent: "示例", querySelector: () => null };
      const markdown = { children: prose ? [pre, paragraph] : [pre], querySelectorAll: () => Array(blocks).fill(pre) };
      return { querySelectorAll: () => Array(blocks).fill({ textContent: text }),
        querySelector: (selector: string) => selector === "blockquote" ? quoted : body ? markdown : null } as unknown as HTMLElement;
    }
    expect(candidateText(node())).toBe(text);
    for (const input of [{ quoted: true }, { prose: true }, { blocks: 2 }, { body: false }]) expect(candidateText(node(input))).toBeNull();
  });
});
