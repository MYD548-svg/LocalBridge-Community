import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { App } from "../App";
import { bridge, parseMainProjection } from "../bridge";
import { ProjectManager, projectNameFromPath } from "../features/projects/ProjectManager";
import { shouldAutoConnect } from "../features/connection/LocalConnectionStatus";
import type { ConnectionState } from "../features/connection/api";
import fixture from "../../tests/fixtures/ui/main_projection.json";
import { workspaceDisplayText } from "../presentation";

describe("dashboard-first startup and named projects", () => {
  it("renders the dashboard before any setup, service or tool call", () => {
    const markup = renderToStaticMarkup(<App/>);
    expect(markup).toContain("添加项目");
    expect(markup).toContain("设置");
    expect(markup).toContain("诊断");
    expect(markup).not.toContain("onboarding-shell");
    expect(markup).not.toContain("Tunnel ID");
  });
  it("shows names first while retaining paths for same-name projects", () => {
    const projects = [{ id: "a", name: "论文项目", path: "D:/论文", active: true }, { id: "b", name: "论文项目", path: "E:/备份/论文", active: false }];
    const markup = renderToStaticMarkup(<ProjectManager projects={projects} error={null} onRun={async (action) => action()} onSelect={() => undefined} onRemove={() => undefined} onClose={() => undefined}/>);
    expect(markup).toContain("重命名");
    expect(markup).toContain("E:/备份/论文");
    expect(markup).toContain("使用");
    expect(workspaceDisplayText(parseMainProjection(fixture))).toBe("LocalBridge");
    expect(projectNameFromPath("D:\\项目\\毕业论文\\")).toBe("毕业论文");
    expect(projectNameFromPath("D:\\")).toBe("本地项目");
    expect(() => parseMainProjection({ ...fixture, projects: [{ id: "a", path: "D:/论文", active: true }] })).toThrow();
  });
  it("saves additional projects without activating and renames by ID", async () => {
    invoke.mockResolvedValue("project-one");
    expect(await bridge.addProjectDeferred("D:/论文", "论文研究")).toBe("project-one");
    expect(invoke).toHaveBeenLastCalledWith("add_project", { path: "D:/论文", name: "论文研究", deferActivation: true });
    await bridge.renameProject("project-one", "研究资料");
    expect(invoke).toHaveBeenLastCalledWith("rename_project", { id: "project-one", name: "研究资料" });
  });
});

describe("automatic connection respects user intent", () => {
  const ready: ConnectionState = { mode: "local", codexDetected: true, serviceReady: true, configurationComplete: false, autoConnectEnabled: true, connectedClients: 0, successfulCalls: 0, affectedTasks: [] };
  it("connects ready local service, preserving manual stop, disconnect and advanced mode", () => {
    expect(shouldAutoConnect(ready)).toBe(true);
    for (const change of [{ serviceReady: false }, { autoConnectEnabled: false }, { configurationComplete: true }, { codexDetected: false }, { mode: "openai_tunnel" as const }]) expect(shouldAutoConnect({ ...ready, ...change })).toBe(false);
  });
});
