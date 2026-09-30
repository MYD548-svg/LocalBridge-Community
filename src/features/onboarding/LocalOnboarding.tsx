import { useEffect, useState } from "react";
import { bridge, uiErrorMessage, type AccessCode } from "../../bridge";
import { accessText } from "../../presentation";
import { WizardFrame } from "../../components/WizardFrame";
import { AdminModeWarning } from "../../components/AdminModeWarning";
import { connectionApi, type ConnectionState } from "../connection/api";
import { onboardingApi } from "./api";

export function LocalOnboarding({ onComplete, previewMode = false, onTunnel }: { onComplete: () => void; previewMode?: boolean; onTunnel: () => void }) {
  const [step, setStep] = useState(1);
  const [state, setState] = useState<ConnectionState | null>(null);
  const [path, setPath] = useState("");
  const [permission, setPermission] = useState<AccessCode>("edit");
  const [pendingDirectory, setPendingDirectory] = useState<string[] | null>(null);
  const [admin, setAdmin] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try { const next = await connectionApi.read(); if (!disposed) setState(next); }
      catch (value) { if (!disposed) setError(uiErrorMessage(value, "无法读取本地连接状态")); }
      if (!disposed) timer = setTimeout(() => void tick(), 2000);
    };
    void bridge.read().then((projection) => setPermission(projection.effectivePermission ?? "edit")).catch(() => undefined);
    void tick(); return () => { disposed = true; clearTimeout(timer); };
  }, []);
  const run = async (action: () => Promise<void>) => { setBusy(true); setError(""); try { await action(); setState(await connectionApi.read()); } catch (value) { setError(uiErrorMessage(value, "操作未完成")); } finally { setBusy(false); } };
  const choosePermission = (mode: AccessCode) => void run(async () => { await bridge.setAccess(mode); setPermission(mode); });
  const next = () => void run(async () => {
    if (step === 2) { const current = await connectionApi.read(); if (current.affectedTasks.length) { setPendingDirectory(current.affectedTasks); return; } await onboardingApi.prepareProject(null, path); }
    if (step === 4) await connectionApi.connect();
    if (step === 5) { if (!previewMode) await onboardingApi.complete(); onComplete(); return; }
    setStep(step + 1);
  });
  const titles = ["检测 Codex", "选择本地工作目录", "选择权限", "连接 Codex", "在 Codex 中验证调用"];
  const enabled = step === 1 ? state?.codexDetected : step === 2 ? !!path : step === 4 ? state?.serviceReady : step === 5 ? state?.configurationComplete && state.connectedClients > 0 && state.successfulCalls > 0 : true;
  return <><WizardFrame step={step} title={titles[step - 1]} footer={<><button className="secondary" disabled={busy || step === 1} onClick={() => setStep(step - 1)}>上一步</button><button className="primary" disabled={busy || !enabled} onClick={next}>{busy ? "处理中…" : step === 4 ? "连接 Codex" : step === 5 ? "完成" : "下一步"}</button></>}>
    {step === 1 && <><p>{state?.codexDetected ? "已检测到 Codex 桌面程序。请确认已登录。" : "请先安装并登录 Codex 桌面程序。"}</p><p>模型由 Codex 联网调用；LocalBridge 在本机执行工具。</p><button className="secondary" disabled={busy} onClick={() => void run(async () => { await connectionApi.setMode("openai_tunnel"); onTunnel(); })}>使用 OpenAI Tunnel 高级兼容模式</button></>}
    {step === 2 && <><p>LocalBridge 只在你选择的范围内操作。</p><p className="project-path">{path || "尚未选择目录"}</p><button className="secondary" disabled={busy} onClick={() => void run(async () => { const selected = await onboardingApi.chooseWorkspaceFolder(); if (selected) setPath(selected); })}>选择目录</button></>}
    {step === 3 && <><p>默认编辑权限；沿用 LocalBridge 现有权限保护。</p><div className="access-grid">{(["edit", "full", "admin"] as AccessCode[]).map((mode) => <button key={mode} aria-pressed={permission === mode} disabled={busy} className={permission === mode ? "choice selected" : "choice"} onClick={() => mode === "admin" ? setAdmin(true) : choosePermission(mode)}>{accessText[mode]}</button>)}</div></>}
    {step === 4 && <><p>服务就绪：{state?.serviceReady ? "是" : "启动失败，请检查诊断或返回选择目录"}</p><p>将使用 Codex 自带的官方配置命令接入，先备份配置，再核验。</p></>}
    {step === 5 && <><p>配置完成：{state?.configurationComplete ? "是" : "否"}</p><p>客户端已连接：{state && state.connectedClients > 0 ? "是" : "等待连接"}</p><p>真实工具调用：{state && state.successfulCalls > 0 ? "已完成" : "等待调用"}</p><p>请在 Codex 新会话中，要求 LocalBridge 列出所选目录，确认真实工具调用成功后点击完成。</p></>}
    {error && <p role="alert">{error}</p>}
  </WizardFrame>{pendingDirectory && <div className="dialog-backdrop"><section className="dialog" role="dialog" aria-modal="true"><h2>取消任务并选择目录</h2><ul>{pendingDirectory.map((task) => <li key={task}>{task}</li>)}</ul><div className="dialog-actions"><button className="secondary" disabled={busy} onClick={() => setPendingDirectory(null)}>保留任务</button><button className="primary" disabled={busy} onClick={() => void run(async () => { await onboardingApi.prepareProject(null, path, true); setPendingDirectory(null); setStep(3); })}>取消任务并继续</button></div></section></div>}{admin && <AdminModeWarning onCancel={() => setAdmin(false)} onConfirm={() => { setAdmin(false); choosePermission("admin"); }}/>}</>;
}
