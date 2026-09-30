import { expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Onboarding } from "../features/onboarding/Onboarding";
import type { OnboardingState } from "../features/onboarding/api";
const state: OnboardingState = { connectionMode: "local", complete: false, projectionRevision: 0, connectionConfigured: false, runtimeKeySaved: false, runtimeKeyLength: null, tunnelId: null, readiness: { localEnvironment: false, codingService: false, openaiTunnel: false } };
it("new local onboarding starts with desktop detection and omits tunnel credentials", () => {
  const markup = renderToStaticMarkup(<Onboarding initial={state} onComplete={() => undefined}/>);
  expect(markup).toContain("检测 Codex");
  expect(markup).toContain("联网调用");
  expect(markup).not.toContain("Runtime API Key");
  expect(markup).not.toContain("Tunnel ID");
});
it("existing tunnel onboarding remains available", () => {
  const markup = renderToStaticMarkup(<Onboarding initial={{ ...state, connectionMode: "openai_tunnel" }} onComplete={() => undefined}/>);
  expect(markup).toContain("简单设置 即可开始");
  expect(markup).not.toContain("检测 Codex");
});
