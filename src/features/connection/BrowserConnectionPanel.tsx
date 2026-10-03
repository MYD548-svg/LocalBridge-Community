import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { bridge, uiErrorMessage } from "../../bridge";
import { browserApi, type BrowserState } from "./browserApi";

export function BrowserConnectionPanel() {
  const [state, setState] = useState<BrowserState | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [disabled, setDisabled] = useState(false);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try { const value = await browserApi.read(); if (!disposed) { setState(value); setError(""); } }
      catch (value) { if (!disposed) setError(uiErrorMessage(value, "无法读取网页接入状态")); }
      if (!disposed) timer = setTimeout(() => void refresh(), 2000);
    };
    void refresh();
    const unlisten = listen("browser-pairing-requested", () => { if (!disposed) setNotice("新的浏览器请求配对，请核对下面的工作区和权限。"); });
    return () => { disposed = true; clearTimeout(timer); void unlisten.then(stop => stop()).catch(() => undefined); };
  }, []);
  async function run(action: () => Promise<void>) {
    setBusy(true); setError(""); setNotice("");
    try { await action(); setState(await browserApi.read()); }
    catch (value) { setError(uiErrorMessage(value, "网页接入操作未完成")); }
    finally { setBusy(false); }
  }
  async function copy(value: string) { await navigator.clipboard.writeText(value); setNotice("已复制"); }
  return <section className="settings-section browser-connection-panel">
    <h3>ChatGPT 网页</h3>
    <p>网关随应用安装。首发从 GitHub 下载扩展 ZIP，开启浏览器开发者模式后手动加载。</p>
    <ol>
      <li>下载扩展 ZIP，选择压缩包或已解压目录，由应用校验并准备。</li>
      <li>进入扩展管理页，开启“开发者模式”，点击“加载解压缩的扩展”，选择下方固定目录。</li>
      <li>打开或刷新 ChatGPT，点击扩展图标连接，在这里确认工作区与权限，再为当前聊天启用工具。</li>
    </ol>
    <div className="inline-actions">
      <button disabled={busy} onClick={() => void run(async () => { await bridge.openGitHubReleases(); })}>下载扩展 ZIP</button>
      <button disabled={busy} onClick={() => void run(async () => { setSelected(await browserApi.choose()); setDisabled(false); })}>选择扩展 ZIP</button>
      <button disabled={busy} onClick={() => void run(async () => { setSelected(await browserApi.choose(true)); setDisabled(false); })}>选择已解压目录</button>
    </div>
    {selected && <div>
      <p>已选择：{selected}</p>
      <label><input type="checkbox" checked={disabled} onChange={event => setDisabled(event.target.checked)} />我已停止工具并关闭浏览器扩展，允许准备或更新受管理的扩展文件</label>
      <button disabled={busy || !disabled} onClick={() => void run(async () => {
        const version = await browserApi.import(selected, disabled); setSelected(null);
        setNotice("扩展 " + version + " 已准备。请加载下方目录；更新时重新启用、点击重新加载并刷新 ChatGPT。");
      })}>导入并准备扩展</button>
    </div>}
    <p>固定加载目录：{state?.directory || "等待读取"}</p>
    <div className="inline-actions">
      <button disabled={busy || !state} onClick={() => void run(() => copy(state!.directory))}>复制加载目录</button>
      <button disabled={busy || !state?.preparedVersion} onClick={() => void run(browserApi.openDirectory)}>打开加载目录</button>
      <button disabled={busy} onClick={() => void run(() => copy("edge://extensions"))}>复制 Edge 管理页地址</button>
      <button disabled={busy} onClick={() => void run(() => copy("chrome://extensions"))}>复制 Chrome 管理页地址</button>
      <button disabled={busy} onClick={() => void run(async () => {})}>检查连接</button>
    </div>
    <ul aria-live="polite">
      <li>扩展文件已准备：{state?.preparedVersion || "请导入 ZIP"}</li>
      <li>应用就绪：{state?.serviceReady ? "是" : "请启动本地服务并选择工作区"}</li>
      <li>浏览器已连接：{state?.connectedBrowsers || 0}</li>
      <li>聊天已启用：{state?.connectedChats || 0}</li>
      <li>已完成真实调用：{state?.successfulCalls || 0}</li>
    </ul>
    <p>当前聊天是否启用，请查看扩展窗口。工具结果回填到在线聊天；输入框有草稿时暂停回填。</p>
    {state?.pairings.filter(pair => !pair.revoked).map(pair => <div key={pair.instance}>
      <p>浏览器实例：{pair.instance.slice(0, 8)} · 工作区：{pair.workspace} · 权限：{({ edit: "编辑", full: "完整权限", elevated: "管理员" } as Record<string, string>)[pair.permission.replaceAll('"', "").toLowerCase()] || pair.permission.replaceAll('"', "")}</p>
      {pair.approved ? <button disabled={busy} onClick={() => void run(() => browserApi.revoke(pair.instance))}>撤销此浏览器授权并停止其会话</button>
        : <button disabled={busy || !state.serviceReady} onClick={() => void run(() => browserApi.approve(pair.instance, pair.context))}>确认此工作区与权限，批准配对</button>}
    </div>)}
    <p>固定目录请保留。升级扩展需要手动重新加载；备份保留。无法连接时先检查本地服务、配对及扩展版本。</p>
    {notice && <p role="status">{notice}</p>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
