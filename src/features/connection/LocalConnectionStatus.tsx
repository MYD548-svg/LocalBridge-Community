import { useEffect, useRef, useState } from "react";
import { bridge, uiErrorMessage } from "../../bridge";
import { connectionApi, type ConnectionState } from "./api";

export function shouldAutoConnect(state: ConnectionState): boolean {
  return state.mode === "local" && state.autoConnectEnabled && state.serviceReady
    && state.codexDetected && !state.configurationComplete;
}

export function LocalConnectionStatus({ onState }: { onState: (state: ConnectionState) => void }) {
  const [state, setState] = useState<ConnectionState | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const attempted = useRef(false);
  const onStateRef = useRef(onState);
  onStateRef.current = onState;
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const current = await connectionApi.read();
        if (disposed) return;
        setState(current); onStateRef.current(current);
        if (!current.serviceReady || !current.autoConnectEnabled || current.mode !== "local") attempted.current = false;
        if (shouldAutoConnect(current) && !attempted.current) {
          attempted.current = true;
          setBusy(true);
          try {
            await connectionApi.autoConnect();
            const connected = await connectionApi.read();
            if (!disposed) { setState(connected); onStateRef.current(connected); setError(""); }
          } catch (value) { if (!disposed) setError(uiErrorMessage(value, "自动接入未完成")); }
          finally { if (!disposed) setBusy(false); }
        }
      } catch (value) { if (!disposed) setError(uiErrorMessage(value, "无法读取连接状态")); }
      if (!disposed) timer = setTimeout(() => void tick(), 2000);
    };
    void tick();
    return () => { disposed = true; clearTimeout(timer); };
  }, []);
  const connect = async () => {
    setBusy(true);
    try {
      if (!state?.serviceReady) await bridge.restartServices();
      await connectionApi.connect();
      const connected = await connectionApi.read();
      setState(connected); onStateRef.current(connected); setError("");
    } catch (value) { setError(uiErrorMessage(value, "连接未完成")); }
    finally { setBusy(false); }
  };
  return <section className="local-connection-status" aria-live="polite">
    {!state ? <p>正在读取桌面连接…</p> : state.mode === "local" ? <>
      <p>桌面接入：{busy ? "正在接入…" : !state.autoConnectEnabled ? "已断开" : state.configurationComplete ? "配置完成" : "等待自动接入"} · 客户端：{state.connectedClients > 0 ? "已连接" : "等待连接"} · 工具调用：{state.successfulCalls > 0 ? "已验证" : "等待调用"}</p>
      {!state.codexDetected && <p>未检测到桌面客户端。请先安装并登录，再点击重试连接。</p>}
      {state.autoConnectEnabled && state.configurationComplete && state.connectedClients === 0 && <p>首次接入后，请在桌面客户端刷新 MCP 连接或重启客户端，再打开新的本地聊天。</p>}
      {(!state.autoConnectEnabled || (!state.configurationComplete && state.serviceReady)) && <button className="secondary" disabled={busy} onClick={() => void connect()}>{state.autoConnectEnabled ? "重试连接" : "重新连接"}</button>}
    </> : <p>正在使用高级 Tunnel 兼容连接。</p>}
    {error && <p className="error" role="alert">{error}</p>}
  </section>;
}
