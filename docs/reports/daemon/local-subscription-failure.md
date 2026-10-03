# 本地 0.0.7 订阅与活动会话兼容性

- 日期：2026-09-27
- 源码：基于 `b733bc0` 的工作区快照。
- 平台：macOS arm64，本地桌面构建。

本报告记录 0.0.7 本地修复的验证结果，测试数与安装包信息仅对应该次源码快照。

## 已确认原因

1. `terminal.list.subscribe.request` 被转换为 `list_terminals_response`，而 SDK 的
   `observeTerminals` 等待携带所属订阅 ID 的 `terminals_changed`。无法识别的初始响应
   导致等待持续 60 秒，随后以 `Subscription request failed` 关闭共享传输，也中断了
   无关请求。
2. Rust Agent 快照将缺失的 `lastError` 序列化为 `null`，前端却只接受字符串或省略字段，
   因而每五秒拒绝一次包含 Agent 的目录响应。捕获的校验路径为
   `message.payload.entries[n].agent.lastError`。
3. 活动会话的 `persistence.nativeHandle` 和 `persistence.metadata` 也可能为 `null`。
   已安装本地构建的 DevTools 捕获了 `fetch_agents_response` 中这两处校验失败。
   先前仅检查元数据的测试使用非活动快照，漏掉了该差异；Rust 运行时信息也允许
   `extra` 为 null，现在一并按相同方式处理。

最初安装版本在隔离配置下复现了终端错误。首次本地重建后，已安装应用的 bundle 与新构建
一致，但活动会话快照仍校验失败，因此剩余列表错误并非旧应用进程导致。

## 修改

- 终端订阅初始响应映射为 `terminals_changed`。
- 将 null 的 `lastError`、持久化字段和运行时扩展字段归一化为 SDK 已有的可选表示，
  保留实际值并拒绝格式错误值，不迁移或重写会话记录与消息历史。
- 通过 SDK 和 adapter 测试终端初始响应、推送更新、订阅所有权、释放及连接存续。
- 使用 Zod 与生成的校验器测试 null、省略、有效值和非法值，包含有活动会话的目录。
- 将协议测试纳入桌面 CI。
- 打包启动冒烟测试增加隔离的离线 Provider 模拟端：创建工作区和会话、订阅时间线、
  发送流式轮次、读取运行会话列表、追加输入、接收工具与流式事件、完成并释放订阅，
  重启后恢复持久历史。renderer 协议校验警告与未捕获错误都会使测试失败。

## 测试结果

- 协议测试：65 个文件，734 项通过。
- 客户端测试：8 个文件，216 项通过；不包含网络 E2E 套件。
- Rust transport adapter：3 个文件，31 项通过。
- 发布打包：12 项通过。
- SDK 构建、共享界面及客户端类型检查、方法目录检查、格式与 lint 通过。
  目录检查包含 171 项前端映射，对应 168 个规范 Rust 方法。
- 修复后的 SDK/adapter 配合已安装 Rust 二进制和临时元数据副本，完成终端订阅、释放及
  两个 Agent、一个 Workspace 的查询。只读时间线订阅和原生历史分别返回两个既有
  Agent 的 22 与 174 项记录；未向用户会话发送消息，也未修改原应用 registry。
- 独立的隔离离线 Provider 检查通过创建、订阅、发送、追加输入、完成、目录及时间线
  读取，包含推理和工具事件。该检查验证客户端、adapter、daemon 的完整协议路径，
  不发起付费模型请求。
- 最终打包应用使用隔离用户元数据，返回两个 Agent 和一个 Workspace，在 1 ms 内完成
  终端订阅，直到下一次目录刷新均无校验警告。

## Test coverage

Rust 覆盖率不适用：此次未修改 Rust 源码，按仓库规则未重跑 Rust workspace 测试或
llvm-cov。桌面打包过程中构建了 release Rust 二进制。
TypeScript 覆盖率未测量；本次执行定向回归和打包冒烟检查，上述测试数不代表覆盖率。
如需评估未覆盖分支，应对协议校验器和客户端 adapter 单独测量。

## 本地产物

构建命令：

```sh
EXPO_NO_TELEMETRY=1 EXPO_OFFLINE=1 AIT_DESKTOP_SMOKE=1 AIT_DISABLE_SINGLE_INSTANCE_LOCK=1 npm run build:dmg
```

扩展后的打包启动冒烟检查通过全部活动会话和重启用例，renderer 错误数为零。
应用通过 `codesign --verify --deep --strict`，DMG 通过 `hdiutil verify`。
该本地应用使用 ad-hoc 签名，未经公证。

历史产物路径为 `apps/desktop/release/Ait-0.0.7-local-arm64.dmg`，旁有 `.sha256` 文件；
这些是本地构建产物，不是仓库内的下载入口。SHA-256：
`2c89c48edf56f819f3ea89adc8c8691aef78e4bc9c524c29999ba114f10c9a86`。

该安装包包含活动会话 null 字段修复，替代了先前只验证元数据、漏掉这些字段的本地产物。
本次验证不包含发布 Release 资产或替换用户已安装应用。
