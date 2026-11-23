export const timelineExport = {
  title: "导出会话",
  description:
    "仅导出已保存历史。对话包含用户、助手消息及语音转写；完整时间线还包含已保存的卡片。上限 16 MiB。",
  conversation: "仅对话",
  timeline: "完整已保存时间线",
  download: "导出并下载",
  cancel: "取消导出",
  unavailable: "导出不可用。请连接支持时间线导出的主机。",
  tooLarge: "导出超过 16 MiB 上限，未截断任何内容。",
  browserHandoff: "已交给浏览器，请查看浏览器下载列表以完成保存。",
};

export const nativeHtmlExport = {
  title: "原生 HTML 导出（OMP）",
  description:
    "由 Oh My Pi 用其自带格式渲染本会话。文件可能包含活动脚本，仅作为下载保存，不会在 Ait 内打开。上限 16 MiB。",
  download: "导出原生 HTML",
  risk: "原生格式含脚本，打开时可能加载外部查看器（stencil.so），该查看器可读取文件内嵌的会话历史。仅下载，Ait 从不预览或打开；与惰性的通用 HTML 导出不同。",
  unavailable: "原生 HTML 导出不可用。请连接支持该功能的 OMP 主机。",
};
