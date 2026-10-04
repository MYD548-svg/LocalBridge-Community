export interface PopupGuide { message: string; primary: string | null }
export function popupGuide(status: string, error = ""): PopupGuide {
  if (status === "unknown") return { message: "执行结果未知。先核查项目中的实际结果，保留记录；这条请求不会自动重发。", primary: null };
  if (/native messaging host.*not found|not found.*native messaging host|Failed to start native messaging host|指定的本机.*未找到/i.test(error)) {
    return { message: "这台电脑缺少可用的 LocalBridge 浏览器宿主。请安装本项目配套的新版安装包，再在应用准备扩展；仅导入 ZIP 无法补齐宿主。", primary: "connect" };
  }
  if (/forbidden|拒绝.*宿主|版本.*不兼容|protocol.*incompatible/i.test(error)) {
    return { message: "安装包与扩展的身份、版本或权限不匹配。请使用 LocalBridge 内置扩展，或本项目同次发行的安装包与 ZIP。", primary: null };
  }
  if (/草稿/.test(error)) return { message: "请先保存或发送聊天输入框中的草稿，再填入工具说明或真实结果。原草稿会保留。", primary: null };
  switch (status) {
    case "awaiting_pairing": return { message: "回到 LocalBridge → 设置 → ChatGPT 网页，核对项目文件夹和权限，点击‘批准配对’。然后返回这里，点击‘我已批准，启用此聊天’。", primary: "enable" };
    case "paired": return { message: "浏览器已获准。点击‘为此聊天启用工具’，看到‘当前聊天已启用’后再开始调用。", primary: "enable" };
    case "enabled": return { message: "当前聊天已启用。首次使用点击‘准备首次只读调用’，检查填入的内容并手动发送。", primary: "first-call" };
    case "pending": return { message: "先核对下方工具名和参数。只有点击‘确认执行以上请求’才会在当前项目执行。", primary: "execute" };
    case "running": return { message: "工具正在本机执行，请等待实际结果。需要停止时点击‘取消当前执行’。", primary: "cancel" };
    case "succeeded": return { message: "本次工具已实际完成。点击‘填入结果’，检查聊天输入框中的内容后手动发送。", primary: "fill" };
    case "failed": return { message: "本次真实调用失败，请查看下方实际结果并处理原因。需要报告给助手时回填该结果。", primary: "fill" };
    case "connecting": return { message: "正在连接。若应用尚未准备好，请选择项目、启动本地服务，并使用本地连接模式。", primary: null };
    case "paused": return { message: "刷新或切换聊天后需要重新连接/启用；项目变化后还需要在应用重新核对授权。", primary: "connect" };
    default: return { message: "先打开 LocalBridge，选择项目并启动本地服务。保持 ChatGPT 当前聊天页面打开，点击‘连接这台电脑’。", primary: "connect" };
  }
}
