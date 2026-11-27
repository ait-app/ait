---
id: ADR-284
status: superseded
date: 2026-11-16
paths:
  - apps/desktop
  - apps/mobile/src/desktop/browser
---

# ADR-284：Workspace 浏览器由主进程 WebContentsView 托管

状态说明：Superseded（2026-11-27），由 [ADR-317](adr-317-browser-webview-tag.md) 取代：原生 view 无法被 web overlay 覆盖，浏览器回到 `<webview>`。
以下保留 `WebContentsView` 方案的历史决策。

## 背景

Workspace 浏览器此前通过渲染进程的 `<webview>` 标签创建 guest `WebContents`。这样把浏览器内容的生命周期、几何位置和安全配置分散在 DOM、渲染进程 IPC 以及主进程的 attach 回调中，也让浏览器能力继续依赖 Electron 的嵌入式标签路径。

浏览器页面仍需要复用现有的 profile partition、导航限制、弹窗策略、键盘策略、自动化和远端 Workspace 出站能力。迁移不能改变这些边界，也不能把 Google 登录误认为已经获得系统浏览器的用户代理。

## 决策

Workspace 浏览器的生产路径改为由主进程创建和拥有 `WebContentsView`。每个 view 放入主窗口 `contentView` 下的 `View` 容器，并由主进程负责：

- 使用已授权的 profile partition 创建沙箱化 WebContents；
- 安装导航、弹窗、权限、键盘和上下文菜单策略；
- 在 Workspace 浏览器注册表中登记 WebContents，并在窗口或 view 销毁时回收；
- 通过受限 IPC 接收导航、命令、脚本和 bounds 更新，并向窗口转发页面事件。

渲染进程保留一个轻量 DOM 代理，用来承接现有浏览器 pane 的事件和控制接口。代理只同步 view 的容器矩形、内容矩形与可见性；页面内容不再由渲染进程创建的 `<webview>` 承载。旧 `<webview>` attach 处理保留为兼容回退路径，生产桌面 bridge 默认使用 `WebContentsView`。

Google 账号登录仍按统一浏览器登录边界交给 `shell.openExternal`。`WebContentsView` 仍是 Electron 内嵌内容，不能作为绕过 Google embedded user-agent 限制的方案。

## 后果

- 浏览器 WebContents 的拥有者、profile 和安全策略集中在主进程，view 的窗口生命周期与 Workspace 窗口一致。
- 渲染层不再依赖 guest attach 才能控制浏览器，自动化和驻留 tab 继续通过同一代理接口工作。
- view 以原生窗口层绘制，位于主窗口 renderer 内容之上；浏览器区域内的 renderer overlay 需要后续迁移到主进程 view 或调整为窗口外 overlay。
- 迁移不会扩大允许的 URL、profile partition 或 popup 范围，也不会改变远端 Workspace 的 daemon 出站路径。
