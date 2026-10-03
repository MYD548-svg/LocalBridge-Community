import { useEffect, useState } from "react";
import { bridge, uiErrorMessage } from "../../bridge";
import { connectionApi, type ConnectionMode, type ConnectionState } from "./api";

export function ConnectionPanel({ onState }: { onState?: (state: ConnectionState) => void }) {
  const [state, setState] = useState<ConnectionState | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<{ tasks: string[]; action: () => Promise<void> } | null>(null);
  const refresh = async () => { const next = await connectionApi.read(); setState(next); onState?.(next); };
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try { const next = await connectionApi.read(); if (!disposed) { setState(next); onState?.(next); } }
      catch (value) { if (!disposed) setError(uiErrorMessage(value, "无法读取连接状态")); }
      if (!disposed) timer = setTimeout(() => void tick(), 2000);
    };
    void tick();
    return () => { disposed = true; clearTimeout(timer); };
  }, []);
  const run = async (action: () => Promise<void>) => {
    setBusy(true); setError("");
    try { await action(); await refresh(); } catch (value) { setError(uiErrorMessage(value, "连接操作未完成")); }
    finally { setBusy(false); }
  };
  const change = (action: (confirmed: boolean) => Promise<void>) => void run(async () => {
    const current = await connectionApi.read();
    if (current.affectedTasks.length) setPending({ tasks: current.affectedTasks, action: () => action(true) });
    else await action(false);
  });
  return <section className="settings-section connection-panel"><h3>桌面连接</h3>
    <p>默认使用本地连接，无需 Tunnel、API Key 或 Tunnel ID。</p>
    {state?.mode === "local" && <>
      <p>模型仍由 Codex 联网调用。本地工具在这台电脑执行。</p>
      <ul aria-live="polite"><li>服务就绪：{state.serviceReady ? "是" : "请启动 LocalBridge"}</li><li>配置完成：{state.configurationComplete ? "是" : "未接入"}</li><li>客户端已连接：{state.connectedClients > 0 ? "是" : "等待 Codex 新会话"}</li></ul>
      <div className="inline-actions"><button className="primary" disabled={busy} onClick={() => void run(async () => { if (!state.serviceReady) await bridge.restartServices(); await connectionApi.connect(); })}>重新连接</button><button className="secondary" disabled={busy} onClick={() => change(connectionApi.disconnect)}>断开接入</button></div>
      {!state.codexDetected && <p>未检测到 Codex 桌面程序，请安装并登录后重新打开 LocalBridge。</p>}
      <p>接入后请在 Codex 新会话调用一个 LocalBridge 工具。手动关闭服务后，需要自行启动 LocalBridge。</p>
    </>}
    <details><summary>高级兼容连接</summary><div className="inline-actions">{(["local", "openai_tunnel"] as ConnectionMode[]).map((mode) => <button key={mode} className={state?.mode === mode ? "primary" : "secondary"} disabled={busy || !state} onClick={() => change((confirmed) => connectionApi.setMode(mode, confirmed))}>{mode === "local" ? "本地连接" : "OpenAI Tunnel"}</button>)}</div></details>
    {error && <p role="alert">{error}</p>}
    {pending && <div className="dialog-backdrop"><section className="dialog" role="dialog" aria-modal="true"><h2>取消受影响任务</h2><p>下列任务终止并关闭活动连接后，才能继续：</p><ul>{pending.tasks.map((task) => <li key={task}>{task}</li>)}</ul><div className="dialog-actions"><button className="secondary" onClick={() => setPending(null)}>保留任务</button><button className="primary" disabled={busy} onClick={() => { const action = pending.action; setPending(null); void run(action); }}>取消任务并继续</button></div></section></div>}
  </section>;
}
