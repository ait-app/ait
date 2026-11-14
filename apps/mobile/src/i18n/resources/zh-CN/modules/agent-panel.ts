export const agentPanel = {
  states: {
    notFound: "未找到 Agent",
    failedToLoad: "加载 Agent 失败",
    reconnecting: "正在重新连接主机",
    updating: "正在更新消息",
    timelineSyncFailed: "无法刷新代理历史记录。",
    timelineSyncRetrying: "正在重试…",
    archivingTitle: "正在归档 Agent...",
    archivingSubtitle: "请稍候，我们正在归档这个 Agent。",
  },
  unavailable: {
    selectedHost: "选中的 Host",
    unknownHost: "无法打开此 Agent，因为此设备上未配置 {{serverLabel}}。",
    addHost: "请在设置中添加 Host，或打开已配置服务器上的 Agent 后继续。",
    preparingSession: "正在准备 {{serverLabel}} 会话...",
    connecting: "正在连接 {{serverLabel}}...",
    showSoon: "稍后将显示此 Agent。",
    showWhenOnline: "Host 在线后将显示此 Agent。",
    reconnectingTo: "正在重新连接 {{serverLabel}}...",
    showAgainWhenReachable: "Host 可访问后将再次显示此 Agent。",
  },
  archived: {
    callout: "此 Agent 已归档",
    unarchive: "取消归档",
  },
};
