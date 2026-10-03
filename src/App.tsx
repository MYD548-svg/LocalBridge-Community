import { useCallback, useEffect, useState } from "react";
import { APP_NAME } from "./appModel";
import { bridge, parseUiError, type AccessCode, type ProjectProjection, type UiError } from "./bridge";
import { initialProjectionTransportState, projectionReadFailed, projectionReadSucceeded } from "./projectionTransport";
import { accessText, currentActivityDetail, currentActivityElapsed, currentActivityText, formatLastToolAge, lastActivityAction, lastActivityOutcome, permissionRestartNotice, projectionStatusText, serviceText, uiText, updateStatusText, workspaceDisplayText } from "./presentation";
import { Onboarding } from "./features/onboarding/Onboarding";
import { onboardingApi, type OnboardingState } from "./features/onboarding/api";
import { Diagnostics } from "./features/diagnostics/Diagnostics";
import { WindowChrome } from "./components/WindowChrome";
import { ServiceStatusDot } from "./components/ServiceStatusDot";
import { AdminModeWarning } from "./components/AdminModeWarning";
import { UiErrorNotice } from "./components/UiErrorNotice";
import "./styles.css";
import { ConnectionPanel } from "./features/connection/ConnectionPanel";
import { connectionApi, type ConnectionMode } from "./features/connection/api";
import { LocalConnectionStatus } from "./features/connection/LocalConnectionStatus";
import { ProjectManager } from "./features/projects/ProjectManager";

type View = "main" | "settings" | "diagnostics";

export function App() {
  const [onboarding, setOnboarding] = useState<OnboardingState | null>(null);
  const [onboardingError, setOnboardingError] = useState(false);
  const [onboardingPreview, setOnboardingPreview] = useState(false);
  return (
    <WindowChrome>
      <Dashboard onOpenWelcome={() => { setOnboardingError(false); void onboardingApi.read().then((current) => { setOnboarding(current); setOnboardingPreview(true); }).catch(() => setOnboardingError(true)); }}/>
      {onboardingError && <p role="alert">无法读取使用帮助，请在诊断中查看原因。</p>}
      {onboardingPreview && onboarding && <div className="sheet-backdrop"><section className="sheet welcome-help"><button className="secondary" onClick={() => setOnboardingPreview(false)}>关闭帮助</button><Onboarding initial={onboarding} previewMode onComplete={() => setOnboardingPreview(false)}/></section></div>}
    </WindowChrome>
  );
}

function Dashboard({ onOpenWelcome }: { onOpenWelcome: () => void }) {
  const [projectionTransport, setProjectionTransport] = useState(initialProjectionTransportState);
  const projection = projectionTransport.projection;
  const [view, setView] = useState<View>("main");
  const [error, setError] = useState<UiError | null>(null);
  const [connectionMode, setConnectionMode] = useState<ConnectionMode | null>(null);
  const [switchConfirmation, setSwitchConfirmation] = useState<{ tasks: string[]; action: () => Promise<void> } | null>(null);
  useEffect(() => { void connectionApi.read().then((state) => setConnectionMode(state.mode)).catch(() => undefined); }, []);
  const [keyValue, setKeyValue] = useState("");
  const [tunnelValue, setTunnelValue] = useState("");
  const [editingTunnel, setEditingTunnel] = useState(false);
  const [editingKey, setEditingKey] = useState(false);
  const [confirmingKeyDelete, setConfirmingKeyDelete] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<ProjectProjection | null>(null);
  const [projectPickerOpen, setProjectPickerOpen] = useState(false);
  const [adminWarningOpen, setAdminWarningOpen] = useState(false);
  const [handledGeneration, setHandledGeneration] = useState<number | null>(null);
  const clearCommandError = useCallback(() => {
    setError(null);
  }, []);
  const showCommandError = useCallback((value: unknown) => {
    setError(parseUiError(value, "操作未完成"));
  }, []);
  useEffect(() => {
    const frame = window.requestAnimationFrame(() => { void bridge.uiReady().catch(showCommandError); });
    return () => window.cancelAnimationFrame(frame);
  }, [showCommandError]);
  const refresh = useCallback(async () => {
    try {
      setProjectionTransport(projectionReadSucceeded(await bridge.read()));
    } catch (value) {
      const error = parseUiError(value, "无法读取后端状态");
      setProjectionTransport(projectionReadFailed(error));
      throw error;
    }
  }, []);
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let revision = 0;
      let retryDelayMs = 250;
      while (!cancelled) {
        try {
          const next = await bridge.read();
          if (cancelled) return;
          setProjectionTransport(projectionReadSucceeded(next));
          revision = next.projectionRevision;
          retryDelayMs = 250;
          await bridge.waitForProjectionChange(revision);
        } catch (value) {
          if (cancelled) return;
          setProjectionTransport(projectionReadFailed(parseUiError(value, "无法读取后端状态")));
          await new Promise((resolve) => window.setTimeout(resolve, retryDelayMs));
          retryDelayMs = Math.min(retryDelayMs * 2, 5000);
        }
      }
    })();
    return () => { cancelled = true; };
  }, []);
  const run = useCallback(async (action: () => Promise<void>) => { clearCommandError(); try { await action(); await refresh(); } catch (value) { showCommandError(value); } }, [clearCommandError, refresh, showCommandError]);
  useEffect(() => {
    if (view !== "settings" || (projection && !projection.runtimeKeySaved)) setConfirmingKeyDelete(false);
  }, [view, projection?.runtimeKeySaved]);
  const currentActivity = projection?.currentActivity ?? null;
  const lastActivity = projection?.lastActivity ?? null;
  const taskState = currentActivity?.state ?? (projection?.activityStatus === "ready" ? "idle" : "unavailable");
  const currentDetail = currentActivityDetail(currentActivity);
  const currentElapsed = currentActivityElapsed(currentActivity);
  const activeProject = projection?.projects?.find((item) => item.active) ?? null;
  const reconnectVisible = Boolean(projection?.reconnect && projection.reconnect.generation !== handledGeneration);
  const adminModeFullAccess = projection?.pathAuthority === "administrator";
  const workspaceDisplay = workspaceDisplayText(projection);
  const permissionNotice = permissionRestartNotice(projection);
  const chooseAccess = (mode: AccessCode) => {
    if (mode === "admin" && projection?.privilege !== "active") {
      setAdminWarningOpen(true);
      return;
    }
    void run(() => bridge.setAccess(mode));
  };
  const openProjectPicker = () => {
    setProjectPickerOpen(true);
  };
  const changeProject = (action: (confirmed: boolean) => Promise<void>) => void run(async () => { const state = await connectionApi.read(); if (state.affectedTasks.length) setSwitchConfirmation({ tasks: state.affectedTasks, action: () => action(true) }); else await action(false); });

  return <main className="shell">
    <header className="topbar"><div className="brand">{APP_NAME}</div><div className="top-actions"><button className="ghost" onClick={() => setView("settings")}>{uiText.settings}</button><button className="ghost" onClick={() => setView("diagnostics")}>{uiText.diagnostics}</button></div></header>
    <section className="card">
      <div className="row"><span className="label">当前项目</span><div className="project-actions"><span className={`value project-active-name ${adminModeFullAccess ? "full-access" : ""}`} title={activeProject?.path}>{workspaceDisplay}</span><button className="secondary" disabled={projection?.settingsStatus !== "ready"} onClick={openProjectPicker}>{activeProject ? "管理项目" : "添加项目"}</button></div></div>
      <div className="row"><span className="label">本地运行环境</span><span className="value service-value"><ServiceStatusDot service={projection?.localEnvironmentService ?? null}/><span>{projection?.localEnvironmentService ? serviceText[projection.localEnvironmentService] : projection ? projectionStatusText(projection.runtimeStatus) : "正在读取"}</span></span></div>
      <div className="row"><span className="label">{connectionMode === "openai_tunnel" ? "OpenAI 安全隧道" : "桌面本地连接服务"}</span><span className="value service-value"><ServiceStatusDot service={projection?.tunnelService ?? null}/><span>{projection?.tunnelService ? serviceText[projection.tunnelService] : projection ? projectionStatusText(projection.runtimeStatus) : "正在读取"}</span></span></div>
      <div className="row"><span className="label">编码服务</span><span className="value service-value"><ServiceStatusDot service={projection?.codingService ?? null}/><span>{projection?.codingService ? serviceText[projection.codingService] : projection ? projectionStatusText(projection.runtimeStatus) : "正在读取"}</span></span></div>
      <div className="row"><span className="label">权限模式</span><span className="value permission-mode-value">{projection?.effectivePermission ? accessText[projection.effectivePermission] : projection ? projectionStatusText(projection.authorityStatus) : "正在读取"}</span></div>
    </section>
    {projection?.settingsStatus === "ready" && !activeProject && <p className="project-empty">添加项目后即可使用。点击“添加项目”选择文件夹并命名。</p>}
    <LocalConnectionStatus onState={(state) => setConnectionMode(state.mode)}/>
    {projection?.activeFaults.length ? <section className="fault-banner" role="alert"><div><strong>LocalBridge 需要处理</strong><p>{projection.activeFaults[0].message}{projection.activeFaults.length > 1 ? `（另有 ${projection.activeFaults.length - 1} 项）` : ""}</p></div><button className="secondary" onClick={() => setView("diagnostics")}>查看诊断</button></section> : null}
    <div className="service-actions" aria-label="服务控制"><button className="secondary service-restart" onClick={() => void run(() => bridge.restartServices())}>重启服务</button><button className="secondary service-stop" onClick={() => void run(() => bridge.stopServices())}>关闭服务</button></div>
    <div className="task-row" aria-live="polite"><span className={`activity-dot task-${taskState}`} aria-hidden="true"/><span className="activity-row-main"><span className="activity-action">{currentActivityText(currentActivity, projection?.activityStatus ?? "unavailable")}</span>{currentDetail && <span className="activity-summary">{currentDetail}</span>}</span>{currentElapsed && <span className="activity-elapsed">{currentElapsed}</span>}</div>
    {lastActivity ? <div className="last-tool-row"><span className="activity-dot-spacer" aria-hidden="true"/><span className={`last-activity-left outcome-${lastActivity.outcome}`}><span className="activity-action">上次执行：{lastActivityAction(lastActivity)}</span>{lastActivity.summary && <span className="activity-summary">{lastActivity.summary}</span>}<span className="activity-outcome">{lastActivityOutcome(lastActivity)}</span></span><span className="last-tool-age">{formatLastToolAge(Math.max(0, Date.now() - lastActivity.completedAtMs))}</span></div> : null}
    {projectionTransport.error && <UiErrorNotice error={projectionTransport.error} />}
    {error && <UiErrorNotice error={error} />}
    {view === "settings" && <div className="sheet-backdrop" onMouseDown={() => setView("main")}><section className="sheet" onMouseDown={(event) => event.stopPropagation()}><h2>{uiText.settings}</h2>
      <section className="settings-section"><h3>常规</h3><div className="row"><span>开机启动</span><input type="checkbox" disabled={projection?.autoStart == null} checked={projection?.autoStart ?? false} onChange={(event) => void run(() => bridge.setAutoStart(event.target.checked))}/></div><div className="row"><span>关闭窗口后继续运行</span><input type="checkbox" disabled={projection?.closeWindowContinueRunning == null} checked={projection?.closeWindowContinueRunning ?? false} onChange={(event) => void run(() => bridge.setCloseWindowContinueRunning(event.target.checked))}/></div></section>
      <ConnectionPanel onState={(state) => setConnectionMode(state.mode)}/>{connectionMode === "openai_tunnel" && <section className="settings-section"><h3>高级兼容连接</h3>
        <div className="field"><label htmlFor="tunnel-id">Tunnel ID</label>{editingTunnel ? <><input id="tunnel-id" autoComplete="off" value={tunnelValue} onChange={(event) => setTunnelValue(event.target.value)} placeholder="输入 Tunnel ID"/><div className="inline-actions"><button className="primary" disabled={!tunnelValue.trim()} onClick={() => void run(async () => { await bridge.saveTunnelId(tunnelValue.trim()); setTunnelValue(""); setEditingTunnel(false); })}>保存</button><button className="secondary" onClick={() => { setTunnelValue(""); setEditingTunnel(false); }}>取消</button></div></> : <div className="settings-summary"><span>{projection?.connectionStatus !== "ready" ? projection ? projectionStatusText(projection.connectionStatus) : "正在读取" : projection.connection?.desiredTunnelId ?? "未保存"}</span><button className="secondary settings-replace" disabled={projection?.connectionStatus !== "ready"} onClick={() => { setTunnelValue(projection?.connection?.desiredTunnelId ?? ""); setEditingTunnel(true); }}>更换</button></div>}</div>
        <div className="field"><label htmlFor="runtime-key">Runtime API Key</label>{editingKey ? <><input id="runtime-key" type="password" autoComplete="off" value={keyValue} onChange={(event) => setKeyValue(event.target.value)} placeholder="输入新的 Runtime API Key"/><div className="inline-actions"><button className="primary" disabled={!keyValue.trim()} onClick={() => void run(async () => { await bridge.saveKey(keyValue); setKeyValue(""); setEditingKey(false); })}>保存</button><button className="secondary" onClick={() => { setKeyValue(""); setEditingKey(false); }}>取消</button></div></> : confirmingKeyDelete && projection?.runtimeKeySaved ? <div className="settings-summary settings-delete-confirm"><span>请确认从windows安全凭据中删除？</span><button className="secondary settings-delete-cancel" onClick={() => setConfirmingKeyDelete(false)}>取消</button><button className="secondary settings-confirm-delete" onClick={() => void run(async () => { await bridge.clearKey(); setConfirmingKeyDelete(false); })}>确认</button></div> : <div className="settings-summary"><span>{projection?.runtimeKeySaved == null ? projection ? projectionStatusText(projection.settingsStatus) : "正在读取" : projection.runtimeKeySaved ? "已保存" : "未保存"}</span>{projection?.runtimeKeySaved && <button className="secondary settings-clear" onClick={() => setConfirmingKeyDelete(true)}>清除</button>}<button className="secondary settings-replace" disabled={projection?.runtimeKeySaved == null} onClick={() => { setConfirmingKeyDelete(false); setKeyValue(""); setEditingKey(true); }}>更换</button></div>}</div>
      </section>
      }<section className="settings-section"><h3>权限</h3><div className="access-grid">{(["edit", "full", "admin"] as AccessCode[]).map((mode) => { const selected = projection?.effectivePermission === mode; const pending = projection?.permission === mode && projection?.permissionReconciliation !== "converged"; return <button key={mode} disabled={projection?.authorityStatus !== "ready"} aria-pressed={selected} className={`choice ${mode === "admin" ? "admin-choice" : ""} ${selected ? "selected" : ""} ${pending ? "pending" : ""}`} onClick={() => chooseAccess(mode)}>{accessText[mode]}</button>; })}</div>{permissionNotice ? <p className="settings-status">{permissionNotice}</p> : null}</section>
      <section className="settings-section"><h3>关于</h3><div className="settings-summary"><span>{updateStatusText(projection?.update ?? null, projection?.updateStatus ?? "unavailable")}</span><div className="inline-actions"><button className="secondary" disabled={!projection?.update?.retryable || projection.update.state === "checking"} onClick={() => void run(async () => { await bridge.retryUpdateCheck(); })}>检查更新</button><button className="secondary" disabled={!projection?.update?.releaseUrl} onClick={() => void run(async () => { await bridge.openGitHubReleases(); })}>GitHub Releases</button></div></div></section>
      <div className="dialog-actions">{connectionMode === "openai_tunnel" && <button className="secondary" onClick={() => { setView("main"); onOpenWelcome(); }}>兼容连接帮助</button>}<button className="primary" onClick={() => setView("main")}>完成</button></div>
    </section></div>}
    {view === "diagnostics" && <Diagnostics commandError={error} onClose={() => setView("main")} />}
    {adminWarningOpen && <AdminModeWarning onCancel={() => setAdminWarningOpen(false)} onConfirm={() => { setAdminWarningOpen(false); void run(() => bridge.setAccess("admin")); }} />}
    {projectPickerOpen && <ProjectManager projects={projection?.projects ?? []} error={error} onRun={run} onSelect={(id) => changeProject(async (confirmed) => { await bridge.selectProject(id, confirmed); setProjectPickerOpen(false); })} onRemove={(item) => { if (item.active) { setProjectPickerOpen(false); setRemoveTarget(item); } else void run(() => bridge.removeProject(item.id)); }} onClose={() => setProjectPickerOpen(false)}/>}
    {switchConfirmation && <div className="dialog-backdrop"><section className="dialog" role="dialog" aria-modal="true"><h2>取消任务并切换目录</h2><ul>{switchConfirmation.tasks.map((task) => <li key={task}>{task}</li>)}</ul><div className="dialog-actions"><button className="secondary" onClick={() => setSwitchConfirmation(null)}>保留任务</button><button className="primary" onClick={() => { const action = switchConfirmation.action; setSwitchConfirmation(null); void run(action); }}>取消任务并继续</button></div></section></div>}
    {removeTarget && <div className="dialog-backdrop"><section className="dialog"><h2>移除当前项目</h2><p>从 LocalBridge 移除此项目？<br/>不会删除项目文件。</p><div className="dialog-actions"><button className="secondary" onClick={() => setRemoveTarget(null)}>取消</button><button className="primary" onClick={() => changeProject(async (confirmed) => { await bridge.removeProject(removeTarget.id, confirmed); setRemoveTarget(null); })}>移除</button></div></section></div>}
    {reconnectVisible && projection?.reconnect && <div className="dialog-backdrop"><section className="dialog" role="dialog" aria-modal="true"><h2>连接失败</h2><p>已自动重试 5 次。</p><div className="dialog-actions"><button className="secondary" onClick={() => void run(async () => { await bridge.retry(); setHandledGeneration(projection.reconnect?.generation ?? null); })}>重试</button><button className="primary" onClick={() => { setHandledGeneration(projection.reconnect?.generation ?? null); setView("diagnostics"); }}>查看诊断</button></div></section></div>}
  </main>;
}
