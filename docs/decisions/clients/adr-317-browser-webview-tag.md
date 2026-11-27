---
id: ADR-317
status: accepted
date: 2026-11-27
supersedes:
  - clients/adr-284-browser-webcontents-view.md
paths:
  - apps/desktop
  - apps/mobile/src/desktop/browser
---

# ADR-317：Workspace 浏览器回到渲染进程 `<webview>`

- 状态说明：Accepted，取代 [ADR-284](adr-284-browser-webcontents-view.md)

## 背景

[ADR-284](adr-284-browser-webcontents-view.md) 把 Workspace 浏览器改为主进程 `WebContentsView`，并接受了一个后果：
原生 view 绘制在主窗口 renderer 内容之上。`BrowserWindow` 的 renderer 就是窗口内容 view 本身，浏览器 view 只能作为它的子 view，
因此任何 web overlay 都无法盖住浏览器。标签页右键菜单、Workspace 菜单、Workspace 信息卡和其他浮层只要伸进浏览器区域，
就被页面遮住，而这类浮层在桌面端很常见。

能让 overlay 盖住原生 view 的方案代价都过高：截图冻结页面会让浮层期间的页面失去交互；把浮层放进另一个 view 需要第二个 renderer
和跨进程状态；改用 `BaseWindow` 并在浮层打开时调整 view 层级，需要迁移所有依赖 `BrowserWindow.webContents` 的窗口代码，
还要让应用外壳在浏览器区域透明。

## 决策

Workspace 浏览器的生产路径回到渲染进程 `<webview>` 标签。guest 页面由 renderer 合成，与其他 DOM 元素共用层叠顺序，
web overlay 按 z-index 自然绘制在浏览器之上。

主进程继续在 `will-attach-webview` 与 `did-attach-webview` 中负责 profile partition 校验、沙箱配置、导航限制、弹窗、权限、
键盘和上下文菜单策略，并在 Workspace 浏览器注册表中登记 guest。删除 `WebContentsView` 管理器、对应的 IPC 与渲染进程 DOM 代理。

Google 账号登录仍按统一浏览器登录边界交给 `shell.openExternal`；`<webview>` 同样是内嵌用户代理，不能绕过 Google 的限制。

## 后果

- 浏览器区域内的菜单、卡片、对话框和其他浮层都能正常显示与交互，不需要按浮层逐一处理。
- 浏览器的生命周期重新由 DOM 元素与 guest attach 共同决定；重挂载 `<webview>` 可能替换 guest WebContents，
  每次 attach 都需要重新登记，驻留标签继续依赖 1x1 停放来保持可绘制。
- 继续使用 Electron 不推荐但仍受支持的 `<webview>` 标签，主窗口保留 `webviewTag: true`。
