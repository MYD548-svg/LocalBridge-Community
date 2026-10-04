import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { bridge, uiErrorMessage } from "../../bridge";
import { browserApi, type BrowserState, type ExtensionDownload, type PrepareResult } from "./browserApi";
import { browserSteps, guideStep, mayPrepare } from "./browserGuide";
import "./browser-guide.css";

interface Props { onManageProjects?: () => void; onUseLocal?: () => void; localMode?: boolean | null }
export function BrowserConnectionPanel({ onManageProjects, onUseLocal, localMode = null }: Props) {
  const [state, setState] = useState<BrowserState | null>(null);
  const [error, setError] = useState("");
  const [readError, setReadError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [confirmedDisabled, setConfirmedDisabled] = useState(false);
  const [browser, setBrowser] = useState<"edge" | "chrome">("edge");
  const [requestedStep, setRequestedStep] = useState(0);
  const [expanded, setExpanded] = useState(true);
  const [download, setDownload] = useState<ExtensionDownload | null>(null);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try { const value = await browserApi.read(); if (!disposed) { setState(value); setReadError(""); } }
      catch (value) { if (!disposed) { setState(null); setReadError(uiErrorMessage(value, "无法读取网页接入状态")); } }
      if (!disposed) timer = setTimeout(() => void refresh(), 2000);
    };
    void refresh();
    const unlisten = listen("browser-pairing-requested", () => { if (!disposed) { setNotice("新的浏览器请求配对，请核对下面的项目文件夹与权限。"); setRequestedStep(4); setExpanded(true); } });
    return () => { disposed = true; clearTimeout(timer); void unlisten.then(stop => stop()).catch(() => undefined); };
  }, []);
  async function run(action: () => Promise<void>) {
    setBusy(true); setError(""); setNotice("");
    try { await action(); setState(await browserApi.read()); setReadError(""); }
    catch (value) { setError(uiErrorMessage(value, "网页接入操作未完成")); }
    finally { setBusy(false); }
  }
  async function copy(value: string, message: string) { await navigator.clipboard.writeText(value); setNotice(message); }
  function prepared(result: PrepareResult) {
    setSelected(null); setConfirmedDisabled(false); setRequestedStep(3);
    setNotice(result.alreadyPrepared ? `扩展 ${result.version} 已是这份配套文件。请在浏览器核对加载与连接状态。`
      : `扩展 ${result.version} 的文件已准备好。接下来请到浏览器加载；升级后重新启用/重新加载扩展并刷新 ChatGPT。`);
  }
  const step = guideStep(state, requestedStep);
  const canPrepare = mayPrepare(state, confirmedDisabled);
  const browserName = browser === "edge" ? "Edge" : "Chrome";
  return <section className="settings-section browser-connection-panel">
    <div className="browser-guide-heading"><h3>ChatGPT 网页</h3><button className="secondary" onClick={() => setExpanded(value => !value)}>{expanded ? "收起安装步骤" : "开始设置"}</button></div>
    <p>把当前 ChatGPT 聊天接到这台电脑。安装包携带配套扩展，无需填写 API Key 或 Tunnel ID；网页接入也不要求安装 Codex 桌面程序。</p>
    <p className="browser-source">应用版本：{state?.applicationVersion || "正在读取"} · 产品发行源：{state?.repository || "正在读取"}</p>
    {expanded && <div className="browser-guide">
      <div className="browser-choice" aria-label="选择浏览器">{(["edge", "chrome"] as const).map(value => <button key={value} className={browser === value ? "primary" : "secondary"} aria-pressed={browser === value} onClick={() => setBrowser(value)}>{value === "edge" ? "Microsoft Edge" : "Google Chrome"}</button>)}</div>
      <nav className="browser-steps" aria-label="安装步骤">{browserSteps.map((label, index) => <button key={label} aria-current={step === index + 1 ? "step" : undefined} disabled={busy || !state || (index > 0 && !state.serviceReady) || (index > 1 && !state.preparedVersion)} onClick={() => setRequestedStep(index + 1)}>{index + 1}. {label}</button>)}</nav>
      <h4>第 {step} 步：{browserSteps[step - 1]}</h4>
      {step === 1 && <div>
        <p>先选择允许 ChatGPT 使用的项目文件夹，并启动 LocalBridge 本地服务。</p>
        <div className="inline-actions"><button className="primary" disabled={busy || !onManageProjects} onClick={onManageProjects}>选择或管理项目文件夹</button>
          {localMode === false ? <button disabled={busy || !onUseLocal} onClick={onUseLocal}>切换到本地连接</button>
            : <button disabled={busy || !state || localMode !== true} onClick={() => void run(async () => { await bridge.restartServices(); setRequestedStep(0); })}>启动本地服务</button>}
        </div><p>{state?.serviceReady ? "项目与本地服务已就绪，可以准备扩展。" : "服务就绪且已选择项目后，继续下一步。网页扩展使用本地连接模式。"}</p>
        {state?.serviceReady && <button className="primary" onClick={() => setRequestedStep(2)}>下一步：准备扩展</button>}
      </div>}
      {step === 2 && <div>
        <p>{state?.bundle.message || "正在读取配套扩展…"}</p>
        {state?.preparedVersion ? <div className="browser-update-notice"><strong>当前目录已准备扩展 {state.preparedVersion}，替换前请按顺序操作：</strong><ol><li>在所有扩展窗口点击“停止并断开”。</li><li>到 {browserName} 扩展管理页，关闭 LocalBridge 的开关。</li><li>回来勾选确认，再准备或导入配套文件。手动导入不同版本会替换现有扩展。</li></ol><label><input type="checkbox" checked={confirmedDisabled} onChange={event => setConfirmedDisabled(event.target.checked)} />我已停止并断开工具、关闭浏览器中的 LocalBridge，明确允许替换当前扩展文件</label></div>
          : <p>这是首次准备。点击按钮后，应用会校验扩展并放入固定目录；随后还需要在浏览器加载。</p>}
        {Boolean(state?.connectedBrowsers || state?.connectedChats) && <p role="status">仍有浏览器连接，请先停止并断开。断开连接后，升级还需要关闭浏览器中的扩展开关。</p>}
        <button className="primary" disabled={busy || !state?.bundle.available || !canPrepare} onClick={() => void run(async () => prepared(await browserApi.prepareBundled(confirmedDisabled)))}>准备配套扩展（推荐）</button>
        <details><summary>单独下载或手动导入 ZIP</summary>
          <p>选择 LocalBridge-ChatGPT-Web-v…zip。Source code ZIP 和 Actions 附件的外层 ZIP 不能直接导入。</p>
          <div className="inline-actions"><button disabled={busy} onClick={() => void run(async () => setDownload(await browserApi.downloadInfo()))}>查询本版本配套下载</button>
            <button disabled={busy} onClick={() => void run(async () => { setSelected(await browserApi.choose()); setConfirmedDisabled(false); })}>选择发行扩展 ZIP</button>
            <button disabled={busy} onClick={() => void run(async () => { setSelected(await browserApi.choose(true)); setConfirmedDisabled(false); })}>选择已解压的扩展目录</button></div>
          {download && <div><p role="status">{download.message}</p>{download.extension && <><p>{download.extension.asset.name} · 协议 {download.extension.protocol} · 应用兼容范围 {download.extension.application}</p><button disabled={busy} onClick={() => void run(async () => setDownload(await browserApi.openDownload()))}>下载本版本扩展 ZIP</button></>}</div>}
          {selected && <div><p className="browser-path">已选择：{selected}</p><button disabled={busy || !canPrepare} onClick={() => void run(async () => prepared(await browserApi.import(selected, confirmedDisabled)))}>校验并导入此扩展</button></div>}
        </details>
        {state?.preparedVersion && <button className="secondary" onClick={() => setRequestedStep(3)}>保留现有文件，继续浏览器加载</button>}
      </div>}
      {step === 3 && <div>
        <p><strong>文件已准备，浏览器还需要手动加载。</strong>请按顺序操作：</p>
        <ol><li>复制 <code>{browser}://extensions</code>，粘贴到 {browserName} 地址栏，按回车。</li><li>打开页面上的“开发者模式”。</li><li>点击“{browser === "edge" ? "加载解压缩的扩展" : "加载已解压的扩展程序"}”。</li><li>选择下方固定目录，里面应有 <code>manifest.json</code>。下载的 ZIP 用于导入应用，浏览器加载请选择这个目录。</li><li>升级时重新启用/重新加载扩展，并刷新 ChatGPT。</li></ol>
        <div className="inline-actions"><button disabled={busy} onClick={() => void run(() => copy(`${browser}://extensions`, `已复制。请粘贴到 ${browserName} 地址栏并按回车。`))}>复制 {browserName} 管理页地址</button><button className="primary" disabled={busy || !state?.preparedVersion} onClick={() => void run(() => copy(state!.directory, "已复制固定加载目录。请在浏览器加载扩展时选择这个目录。"))}>复制固定加载目录</button><button disabled={busy || !state?.preparedVersion} onClick={() => void run(browserApi.openDirectory)}>打开加载目录</button></div>
        <p className="browser-path">固定加载目录：{state?.directory || "请先准备扩展"}</p><p>请保留这个目录；下载的 ZIP 导入完成后可以不再保留。</p>
        <button className="primary" onClick={() => setRequestedStep(4)}>我已在浏览器加载，查看连接与配对步骤</button>
      </div>}
      {step === 4 && <div>
        <ol><li>打开或刷新 <code>https://chatgpt.com</code> 并登录。</li><li>点击浏览器工具栏的“扩展”图标，找到 LocalBridge 并固定到工具栏。</li><li>点击 LocalBridge →“连接这台电脑”。</li><li>回到本页面，核对下面请求中的项目文件夹与权限，再批准配对。</li></ol>
        <p>浏览器加载与连接是两个步骤。没有请求时，回到 ChatGPT 点击扩展里的“连接这台电脑”。</p>
        <button className="secondary" onClick={() => setRequestedStep(5)}>查看下一步：启用当前聊天</button>
      </div>}
      {step === 5 && <div><p>批准配对后，返回 ChatGPT 的 LocalBridge 扩展窗口，点击“为此聊天启用工具”。</p><p><strong>看到“当前聊天已启用”才完成这一步。</strong>当前浏览器与聊天的状态以扩展窗口为准；本页面的数量属于本机总计。</p><p>刷新页面、切换聊天后重新启用；切换项目后需要重新核对授权。</p><button className="primary" onClick={() => setRequestedStep(6)}>查看首次只读调用步骤</button></div>}
      {step === 6 && <div><ol><li>在扩展点击“准备首次只读调用”，将工具说明和测试提示填入聊天。</li><li>检查输入框内容后手动发送；已有草稿时先处理草稿。</li><li>助手产生请求后，在扩展核对工具是 <code>workspace_context</code>，检查参数并点击“确认执行以上请求”。</li><li>看到真实结果后，点击“填入结果”，检查内容并手动发送。</li></ol><p>扩展显示“已完成真实调用”才表示该次调用成功。回填内容会进入在线聊天。</p></div>}
    </div>}
    <div className="browser-live-state"><h4>本机实际接入状态</h4><ul aria-live="polite"><li>扩展文件已准备：{state?.preparedVersion || "尚未准备"}</li><li>项目与本地服务：{state?.serviceReady ? "已就绪" : "未就绪"}</li><li>已连接浏览器总数：{state?.connectedBrowsers ?? "正在读取"}</li><li>已启用聊天总数：{state?.connectedChats ?? "正在读取"}</li><li>真实成功调用总数：{state?.successfulCalls ?? "正在读取"}</li></ul><button className="secondary" disabled={busy} onClick={() => void run(async () => {})}>刷新连接状态</button></div>
    {state?.pairings.filter(pair => !pair.revoked).map(pair => <div className="browser-pairing" key={pair.instance}><p>浏览器实例：{pair.instance.slice(0, 8)} · 项目：{pair.workspace} · 权限：{({ edit: "编辑", full: "完整权限", elevated: "管理员" } as Record<string, string>)[pair.permission.replaceAll('"', "").toLowerCase()] || pair.permission.replaceAll('"', "")}</p>
      {pair.approved ? <><p>此实例已获准。请在扩展里启用当前聊天。</p><button disabled={busy} onClick={() => void run(() => browserApi.revoke(pair.instance))}>撤销此浏览器授权并停止其会话</button></>
        : <button className="primary" disabled={busy || !state.serviceReady} onClick={() => void run(async () => { await browserApi.approve(pair.instance, pair.context); setRequestedStep(5); setNotice("配对已批准。请返回扩展，点击‘为此聊天启用工具’。"); })}>确认此项目与权限，批准配对</button>}</div>)}
    {notice && <p role="status">{notice}</p>}
    {readError && <p role="alert">{readError}</p>}
    {error && <div role="alert"><p>{error}</p><button className="secondary" onClick={() => setError("")}>关闭此操作提示</button></div>}
  </section>;
}
