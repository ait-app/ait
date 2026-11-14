export const importSession = {
  chooseHostTitle: "从 Host 导入",
  title: "导入会话",
  searchPlaceholder: "搜索会话...",
  scope: {
    host: "{{host}} 上的会话",
    workspace: "此工作区",
  },
  filters: {
    all: "全部",
  },
  status: {
    connectHost: "连接到 Host 以导入会话",
    updateHost: "更新 Host 以导入会话。",
    noProviders: "没有已启用且支持会话导入的提供方。",
    loading: "正在加载最近会话...",
    failedProvider: "无法加载 {{provider}} 的会话",
    failedImport: "无法导入所选会话。",
    workingDirectoryUnavailable:
      "此会话在主机上的原工作目录不可用：{{path}}。请恢复该目录后重试导入。",
  },
  actions: {
    refresh: "刷新会话",
    showAll: "显示全部",
    loadMore: "加载更多",
  },
  preview: {
    untitledSession: "未命名会话",
    noPrompt: "没有提示词预览",
  },
  empty: {
    noRecent: "没有可导入的最近会话。",
    noMatches: "没有与搜索匹配的会话。",
    alreadyImported: "所有最近会话都已导入。",
    noProviderSessions: "没有找到 {{provider}} 会话。",
  },
  row: {
    importing: "正在导入...",
  },
};
