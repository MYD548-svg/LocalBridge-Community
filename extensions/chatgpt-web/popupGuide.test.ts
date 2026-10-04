import { describe, expect, it } from "vitest";
import { popupGuide } from "./popupGuide";
describe("next step follows the actual current-chat state", () => {
  it("directs pending pairing back to the exact application card", () => {
    const guide = popupGuide("awaiting_pairing");
    expect(guide.message).toContain("设置 → ChatGPT 网页");
    expect(guide.message).toContain("批准配对");
    expect(guide.primary).toBe("enable");
  });
  it("missing host requires the matching installer rather than another ZIP", () => {
    expect(popupGuide("not_connected", "Specified native messaging host not found.").message).toContain("仅导入 ZIP 无法补齐宿主");
  });
  it("unknown results never suggest automatic replay or execution", () => {
    expect(popupGuide("unknown").primary).toBeNull();
    expect(popupGuide("unknown", "Failed to start native messaging host").primary).toBeNull();
  });
  it("only a real success leads to sending the actual result", () => {
    expect(popupGuide("enabled").primary).toBe("first-call");
    expect(popupGuide("pending").primary).toBe("execute");
    expect(popupGuide("running").primary).toBe("cancel");
    expect(popupGuide("succeeded").primary).toBe("fill");
  });
});
