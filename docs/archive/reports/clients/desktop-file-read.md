# Desktop 0.0.8 文件读取回归

范围：`3f2f1ca8` 加本次工作区修改，macOS ARM64。测试源码哈希、安装包 server 哈希及前后结果见
[验证 JSON](desktop-file-read-validation.json)。本次未修改 Rust 或发布安装包。

## 原因与修复

适配器将 SDK 原始 `requestId` 换成 `rust-<sequence>`，JSON 响应会恢复 `sourceId`，
但二进制 FileBegin / FileChunk / FileEnd 原来直接透传。SDK 按原始 ID 等待文件，
忽略带内部 ID 的数据，导致实际文件已发送，文件面板仍处于加载状态。
二进制传输没有附加 JSON 成功响应，原有 pending 记录也没有及时释放。

使用 0.0.8 对应的 `47d4b7ee` 适配器源码和未修改的 0.0.8 安装包 server，
在隔离目录创建真实存在的 `src/control/execution.rs`、空文件及 600 KiB 分块文件。
路径后缀查询与文件错误返回均成功；server 返回了 13 个文件帧，但成功读取和 LiveFileModel 加载失败。
这排除了本次测试中文件不存在或 server 无法读取文件的解释。

修复按原版设计保留 SDK 的 requestId：删除 `rust-` 重编号、sourceId 及响应反向映射，
请求与 JSON / 二进制响应全程使用同一 ID。仅在请求缺少 ID 时生成 UUID；文件帧原样转发，不重新编码。
FileEnd 清理 pending、重试和超时计时器；迟到数据忽略，错误物理连接的响应拒绝。
重复的未完成请求 ID 会被拒绝，避免覆盖原请求；完成后可再次使用该 ID。Terminal 帧保持原有处理。

另修正验证脚本的错误分类：未指定 expectedCode 时，不能因错误也没有 code 就把失败记为 expected_gap。

## 本地原版 Paseo 对照

对照本地 `/Users/necokeine/Documents/paseo`，revision `30178c4f58b67f8472901356e1484022bd835de0`：

- `packages/client/src/daemon-client.ts:6048` 的 `createRequestId` 返回调用方 ID，缺省才生成 UUID。
  `readFile` 用同一 ID 注册等待和发送请求；接收 FileBegin 时按该 ID 查找 pending。
- `packages/app/src/desktop/daemon/desktop-daemon-transport.ts:170` 与
  `packages/desktop/src/daemon/local-transport.ts:496` 只转发文本/二进制，IPC 的 sessionId 用于选择连接。
- `packages/server/src/server/session/files/workspace-files-session.ts:246` 从请求取出 requestId，
  FileBegin、FileChunk、FileEnd 均原样使用该 ID，没有重新编号或返回时反向映射。
- `packages/server/src/server/websocket-server.file-transfer.e2e.test.ts` 已有同一 ID 的大文件分块测试，
  同时验证只发送给来源连接；本次仅阅读该上游测试，没有运行原版完整套件。

Ait 为接入 Rust 新增了协议适配层，其中的 `rust-<sequence>` 重编号并非 Paseo 原版方案或 Rust 协议要求。
本次已删除该重编号机制，保留 SDK ID，同时继续做 Rust 接口需要的协议封装、字段名和通道适配。

## Test execution

新增 5 个适配器回归测试覆盖交错并发读取、连续 260 次读取的清理、失败后迟到数据、错误连接响应、重复 ID 保护。
前两项在最初修复前明确失败，最终方案全部通过。测试还断言请求 ID 未改写、二进制帧未经重建，
并覆盖 JSON 响应关联、无 ID 心跳、服务端回调、订阅与重试行为。

```sh
PASEO_TEST_DEPS=<node_modules> \
AIT_SERVER_BIN=/Applications/Ait.app/Contents/Resources/bin/daemon \
node scripts/validate-paseo-rust-client.cjs --files

PASEO_TEST_DEPS=<node_modules> node scripts/validate-paseo-rust-client.cjs --files
PASEO_TEST_DEPS=<node_modules> node scripts/validate-paseo-rust-client.cjs --terminal
```

- 0.0.8 安装包 server 和当前 debug server：每次 115 个前端测试、9 项隔离集成全部通过，0 schema 错误。
- 文件测试覆盖后缀查找、绝对路径、文本/空文件/多片二进制、缺失文件、大小限制、首次加载和磁盘修改后的自动刷新。
- 共享 transport 的 Terminal 回归：81 个前端测试、7 项隔离集成通过。
- App `tsgo --noEmit -p apps/mobile/tsconfig.json`、3 个修改文件的 oxfmt/oxlint、`git diff --check` 通过。
- 实际链路为生产 SDK → renderer IPC transport → main-process transport → Rust server，并驱动生产 LiveFileModel。
  没有操作 Electron 界面、用户文件或运行中的 daemon；可视界面验收仍未执行。

## Test coverage

**Not measured**：本次为局部 TypeScript 修复，只运行相关回归；未修改 Rust，因此按仓库要求跳过 Rust 测试。
没有本次行覆盖率或可比覆盖率基线，测试通过数量不是覆盖率。[共享格式的验证结果](desktop-file-read-validation.json)
记录的是测试执行证据。后续发布验收需更新 Desktop 适配层并检查实际文件面板；仅替换 server 不会应用此客户端修复。
