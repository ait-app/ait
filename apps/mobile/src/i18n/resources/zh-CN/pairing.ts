export const pairing = {
  connectionMethods: {
    title: "添加连接",
    direct: {
      title: "直接连接",
      description: "本地网络或 VPN。",
    },
    remoteSsh: {
      title: "远程 SSH",
      description: "通过桌面 SSH 客户端连接。",
    },
  },
  direct: {
    title: "直接连接",
    helper: "输入服务器地址和访问令牌。",
    fields: {
      host: "Host",
      port: "端口",
      password: "访问令牌",
      optional: "必填",
      useSsl: "使用 SSL",
      connectionUri: "连接 URI",
    },
    advanced: {
      label: "高级",
      show: "显示高级选项",
      hide: "隐藏高级选项",
    },
    passwordVisibility: {
      show: "显示密码",
      hide: "隐藏密码",
    },
    actions: {
      cancel: "取消",
      connect: "连接",
      connecting: "正在连接...",
    },
    errors: {
      hostRequired: "Host 必填",
      invalidPort: "端口必须在 1 到 65535 之间",
      invalidConnection: "无效连接",
      failedTitle: "连接失败",
      failedToConnect: "无法连接到 {{endpoint}}。",
      noAdditionalDetails: "{{detail}}（未提供更多详情）",
      timedOut: "连接超时。请检查主机地址、端口和网络。",
      refused: "连接被拒绝。服务器是否正在此地址运行？",
      hostNotFound: "未找到 host。请检查主机名后重试。",
      hostUnreachable: "Host 不可达。请检查网络和防火墙。",
      tlsError: "TLS 错误。只有守护进程前方配置了 TLS 终止代理时，直接连接才使用 SSL。",
      unableToConnect: "无法连接。请检查主机地址、端口，并确认守护进程可达。",
      details: "详情：{{detail}}",
    },
  },
  remoteSsh: {
    title: "远程 SSH",
    helper: "通过 SSH 连接到远程 Ait 服务器（默认服务器端口：7316）。",
    fields: {
      target: "SSH 主机",
    },
    actions: {
      cancel: "取消",
      connect: "连接",
      connecting: "正在连接...",
    },
    errors: {
      tokenRequired: "请输入远程 Ait 服务令牌（32–256 个字符）。",
      targetRequired: "SSH 主机为必填项",
      invalidTarget: "请输入有效的 ssh:// 主机",
      failedToConnect: "无法通过 SSH 连接。{{detail}}",
    },
  },
};
