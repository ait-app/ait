# ADR-056：Ait 与 Paseo 的共享宿主资源隔离

- 状态：Accepted
- 日期：2026-09-27
- 关联：ADR-001 v4、ADR-043、ADR-055
- 修订：ADR-043 的技能目录所有权；补齐 ADR-055 的桌面之外隔离范围

## 背景

隔离 Electron profile 后，技能目标根目录和 Git 仓库仍可由两个应用共同使用。
旧实现把 Paseo 的四种遗留技能列入删除计划，使用相同的文件清单和自动 stash 前缀。
桌面读取 Paseo 环境变量，SSH 默认访问 Paseo 的 6767 `/ws`，移动端沿用其应用 ID。

## 决策

1. 技能逻辑名和 RPC 保持兼容；物理安装目录为三个 agent home 下的
   `skills/ait-<逻辑名>`。安装生成 `.ait-managed-files.json`，必须同时具有
   `version: 1`、`owner: "ait"` 和文件清单才承认所有权。只发现和卸载具备 Ait
   标记的目录，不再把 `paseo-chat` 等名字当成所有权证明。命名冲突时拒绝修改。
   已从 bundle 移除的 Ait 技能仍可被发现，reconcile 保留，确认删除或 uninstall 清理。
2. 事务日志带 `namespace: "ait-v1"`，所有发布和回滚使用上述物理目录。
   不自动恢复旧的无命名空间日志，保留日志和备份并返回错误，避免回滚触及 Paseo。
   不自动搬迁或删除旧目录；若升级遇到未完成的旧事务，应先核对备份和文件所有者，
   完成人工恢复后再处理旧日志。
3. 自动 stash 标识改为 `ait-auto-stash: <branch>`，自动恢复候选只包含该标识。
   `paseoOnly` / `isPaseo` 作为已有 wire 字段保留，语义为当前 Ait 的自动 stash。
   Paseo 和普通 Git stash 可在未筛选列表中查看，不会自动选为 Ait 的分支恢复候选。
4. 桌面配置开关使用 `AIT_*`，包括用户目录、测试名称、单实例锁、窗口控制、调试、
   Chromium flags、CLI、登录 shell 超时和开发根目录。构建端对应使用
   `AIT_WEB_PLATFORM`、`EXPO_PUBLIC_AIT_DEV_BUILD_LABEL`。不回退读取旧 Paseo 变量。
   Linux launcher、开发启动器、打包 smoke 和测试调用方一起更新。内部 IPC 和源码包名
   不承担跨应用身份，不做无关替换。
5. Remote SSH 默认转发远端 loopback 7316，始终请求 Rust `/v1/ws`，并使用同一套
   Rust 多通道协议适配器。用户在独立的掩码字段输入服务 Bearer token；令牌随主机
   连接保存，通过 IPC 独立字段传给主进程的 Authorization header，不放进 URL、
   SSH argv 或连接 ID。探测和长连接共用配置函数。缺少令牌的旧 SSH 连接需重新添加。
   自定义 `daemonPort` 仍可指定，不会降级尝试 Paseo `/ws`。
6. iOS bundle ID 和 Android application ID 分别使用 `dev.ait.mobile`、开发版
   `dev.ait.mobile.debug`，两端均使用 `ait` URL scheme。Fastlane 和设备测试脚本同步。
   原生模块内部 Java 包名不影响应用沙箱，保持原样；显式 iOS 签名 ID 覆盖仍受支持。
   新 ID 需要对应的签名、推送和服务配置，不沿用 Paseo 的应用注册。

## 边界与验证

变更位于本地文件适配器和客户端连接层，不改动 domain、Message、Session、Run 或端口
依赖方向。技能内容仍由宿主部署适配 Ait 的 bundle，不能把含 Paseo CLI 指令的上游技能
直接当作 Ait 技能发布。没有迁移现有用户数据或修改已安装的应用。

验证包含共享根目录中技能的完整生命周期、事务恢复、真实 Git stash、协议协商、
本地 stdio 隧道及 Expo 配置；见[验证报告](../../reports/clients/paseo-coexistence.md)。
