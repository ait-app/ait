# ADR-169：统一 Markdown 渲染与移动面板编辑生命周期

- 状态：已接受
- 日期：2026-10-20
- 来源：Paseo `91154ba4e8ace3484595a05a4366279fb4818745` 的选择性移植。

## 背景

三端 Markdown 使用不同渲染器，选择复制、图片、文件链接和聊天查找各自依赖旧 DOM 或文本片段。手机编辑器全屏、临时文件面板与持久标签页切换，还需要明确编辑状态和焦点归属。

## 决策

1. `components/markdown` 统一使用固定版本的 `react-native-enriched-markdown`。源文本适配、图片获取、链接解释和 Mermaid 渲染留在 Ait 适配层；代码块复用 Ait 高亮与复制，查找和跨段复制读取共享标记。保留 Ait 的远程用户图片、附件生命周期与主题。
2. 原生流式正文按已完成的结构边界拆为稳定 head 和可变 tail。原生单条显示预算独立于 Web；不改动持久 Timeline 的原始消息或协议字节预算。
3. Markdown 图片通过已有 Host 文件读取端口获取；预览中的相对图片以 Markdown 文件所在目录解析。实测消息高度按 Host、Workspace、文本和宽度隔离，未测量时保留图片高度估算。
4. 全屏状态与保留的选区归 `composer/editing-session`。会话以 Workspace、tab 和 draft 身份持有，位于会因布局切换而重挂载的界面之外；退出全屏继续使用同一份草稿、附件和输入状态。
5. 手机文件、子 Agent 等辅助面板由 panel manifest 的 `compactSheet` 规则选择临时 Sheet。Sheet 使用 Workspace 的上下文与尺寸，关闭时恢复原面板焦点；支持显式转为持久标签页。Ait 的 Bot、Schedule 与浏览器生命周期继续使用各自现有规则。
6. 手机 Scripts 菜单复用 Rust 脚本 RPC 与 Ait 服务 URL 解析。没有多路由选择，也不恢复 ADR-164 删除的代理 URL 字段。

## 后果与验证

原生客户端必须重新构建，旧二进制不能仅靠 JS 更新取得新渲染器。包版本、原生配置和上游补丁一起固定；升级时应复核补丁、原生跨段选择、键盘与全屏转场。验证范围见[实施报告](../../reports/clients/paseo-0.12-port-2026-10-20.md)。
