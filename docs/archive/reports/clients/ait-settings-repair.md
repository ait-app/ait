# Ait 设置页修复

- 日期：2026-09-27；基线：`80e97c0` 加本次未提交改动。
- 范围：`apps/mobile` 设置界面、Rust transport 适配及 `apps/desktop` 桌面回归。
- 延续 [ADR-047](../../../decisions/daemon/adr-047-daemon-schedule-browser.md) 的无 Plugin 产品边界；没有修改 Rust 源码或领域依赖。

## 问题与修复

1. 删除插件设置侧栏入口、安装管理页和对应专用测试；移除 `SettingsView` 的插件分支。旧插件配置深链接回到通用设置。
2. 修正 Rust 能力到客户端功能标志的映射：项目列表与稳定 ID、目录创建、GitHub 搜索/克隆、项目图标、Agent profiles 等不再误判为旧主机。
3. Rust 的目录接口不支持上游的目录订阅参数。客户端使用完整分页快照，在页面需要数据时每 5 秒刷新；离线、无页面需求或释放运行实例时停止。修复项目设置页一直加载，并在加载失败时展示错误。
4. 按 Rust 实际支持的事件类型订阅通知，避免将不支持的事件混入一个订阅后导致整个订阅失败。
5. 对服务器明确标为可重试的 `resource_exhausted` 回复，适配器最多退避重试 5 次。保留请求关联、总超时与关闭清理；其他错误、不允许重试的错误不会重试。
6. Rust Provider 快照包含展开后的 `entries` 和 `snapshotHash`，前端原先将其视为没有正文的缓存通知。适配器补齐 compact 正文与每个 Provider 的读取时间；`notModified` 回复仍保持条件缓存语义。修复 Provider 列表及模型配置弹窗无法加载。
7. 设置加载失败时显示错误与重试操作；断线保存配置明确失败，避免将未保存状态当作成功。
8. Rust 当前未接入 relay 配对或 ACP Provider 安装/daemon Provider 覆盖配置。配对页展示直接连接说明；Provider 页保留原生服务的模型与诊断详情，隐藏不能生效的安装、启停与添加自定义模型控件。这些未实现能力没有被包装为成功。

## 验证范围

桌面回归脚本：[settings-rust.e2e.mjs](../../../../apps/desktop/e2e/settings-rust.e2e.mjs)。
使用独立 Electron profile、临时 Rust 数据目录和已构建的 `target/debug/daemon`；不修改用户的现有设置。
覆盖 20 个设置导航项，以及项目列表轮询/项目详情、直接连接说明、Skills 选择、Agent profile 编辑器、
系统提示词保存、工作区选项保存、终端 profile 保存与 Provider 模型详情。配置保存后通过真实 RPC 读回验证。

运行方式：

```sh
EXPO_DEV_URL=http://localhost:8082 npm run test:e2e:settings-rust --workspace=@getpaseo/desktop
```

本机验证 macOS / Electron。未验证 iOS、Android、Linux、Windows 或远程网络配对；Provider 原生能力取决于本机安装与登录状态。

## Test coverage

Rust 覆盖率：不适用，本次未修改 `bins/`、`crates/` 中的 Rust 代码，按 AGENTS.md 跳过 Rust 测试与覆盖率重测。
前端行覆盖率未测量；以下测试数量不是覆盖率百分比。

- 前端 13 个相关单元测试文件：148 项通过。
- 协议 schema 回归 2 个文件：45 项通过。
- App 与 Electron TypeScript 检查通过。
- 真实 Rust 服务桌面回归：20 个设置页、9 组关键操作通过，renderer 错误和 RPC 订阅错误均为 0。
- 格式、lint、`git diff --check` 与 `verify:release` 通过。
- 桌面资源构建 `EXPO_NO_TELEMETRY=1 CI=1 npm run build:desktop-assets` 通过，当前开发应用已重启应用修复。
- [可评审验证结果](ait-settings-validation.json)。

## 后续修复：关于页的“新功能”

实机复现时，弹窗已打开，但远端 `CHANGELOG.md` 返回 HTTP 404，原实现没有本地内容可显示。
现在从根目录 `CHANGELOG.md` 将发布说明写入 Expo 公共配置，首次打开立即显示随应用发布的内容，
后台仍检查在线更新；远端 404、断网或无效内容不会清空已有说明。
Metro 缓存随应用配置、版本及更新日志内容变化，避免 `expo-constants` 继续使用旧的内联应用清单。

- 新增 7 项加载行为测试，加上原有解析测试共 41 项通过。原有日期断言固定为英文，使用 `LC_ALL=en_US.UTF-8` 运行；实际中文界面也已验证。
- 真实 Electron 回归重新通过 20 个设置页、10 组操作，包括“关于 → 新功能”在 HTTP 404 后显示 0.0.6 内容。
- 当前开发窗口另行验证了断网、关闭及再次打开；已重启并停留在正常显示的“新功能”弹窗。
- App 类型检查、格式、lint、差异检查通过；桌面资源重新导出成功，并检查了实际 bundle 中的内置发布说明。
- 本次仍未改动 Rust；未测量 TypeScript 行覆盖率。
