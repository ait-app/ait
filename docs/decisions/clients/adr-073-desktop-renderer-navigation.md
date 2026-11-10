---
id: ADR-073
status: accepted
date: 2026-10-03
amends:
  - clients/adr-055-desktop-profile-isolation.md
paths:
  - apps/desktop
---

# ADR-073：桌面主窗口的导航信任边界

- 关系：补充 [ADR-055](adr-055-desktop-profile-isolation.md) 的桌面隔离约束。

## 背景

Electron 主窗口加载应用 preload，提供桌面 IPC 能力。原导航拦截只拒绝拖入的
`file://` URL，外部网页链接和 HTTP 重定向仍能替换主窗口内容。应用主窗口和
Workspace 浏览器的权限不同，不能让任意网站进入带有应用 preload 的窗口。

## 决策

主窗口在首次加载前安装统一导航策略：

1. 打包版仅允许 `ait://app`；开发版仅允许配置的开发服务器协议、主机及端口。
2. `will-navigate` 和 `will-redirect` 使用相同校验，拒绝用户信息、无效 URL 和其他来源。
   自定义协议的标准 URL origin 可能是 `null`，因此显式比较协议和完整 host。
3. 主窗口拒绝创建任意 popup。应用的外部链接继续通过现有 opener 或 Workspace
   浏览器处理；Workspace webview 保留自己的导航、权限与 popup 策略。
4. 应用自己的路由、查询参数、hash 导航和开发服务器刷新可继续工作。

## 后果与验证

桌面 preload 的信任对象固定为应用 renderer。新增主窗口入口或更换应用来源时，
必须同步安装或更新该策略。程序主动调用的 `loadURL` 仍只接受内部构造的应用地址，
不得用这组 renderer 导航事件替代对该调用的输入校验。

`window-manager.test.ts` 覆盖打包版/开发版、跨来源跳转、重定向、特殊协议、携带
用户信息的地址、同来源路由和 popup；同时运行 Workspace webview 的相关回归测试。
