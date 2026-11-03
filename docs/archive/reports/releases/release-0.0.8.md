# Ait 0.0.8 发布准备

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-09-28（Asia/Shanghai）。准备基线为最新 main
[`1151611d`](https://github.com/necokeine/ait/commit/1151611de5c5c31fedbc2889a5dbb4142da25bcf)。
本文记录发布准备；正式安装包、签名、公证和上传状态以之后的 Release 工作流为准。

## 版本与来源

- Cargo workspace 的 12 个包、28 处本地 path 依赖版本约束和 `Cargo.lock` 同步到 `0.0.8`。
- 根 npm 包及 6 个活跃 `@ait/*` workspace、根 `package-lock.json` 同步到 `0.0.8`。
- 根 `CHANGELOG.md` 增加 0.0.8 条目，随应用打包供“新功能”页面读取。
- 本次只调整版本元数据、锁文件和文档，不升级第三方依赖或修改运行代码。

当前公开的 0.0.7 安装包曾以
[`c03c42d`](https://github.com/necokeine/ait/tree/c03c42d4a05b54910463826465ed6efe3027e1ea)
重建，`v0.0.7` 标签仍指向最初发布源码。下列新增内容以该重建提交为比较基线，
不能仅用 `v0.0.7` 标签判断已安装版本的内容。
参见 [0.0.7 重建记录](release-0.0.7-rebuild-2026-09-28.md)。

## 主要更新

- **整段聊天搜索。** 显示整段对话的匹配总数与当前位置，支持跨消息前后跳转和首尾循环；
  助手消息按 Markdown 文本匹配，旧服务端缺少计数时保留兼容回退。
  [PR #125](https://github.com/necokeine/ait/pull/125)
- **Codex 大图片输出修复。** 移除原生接收行和图片输出的 2 MiB 截断，避免较大的生成图片
  导致会话失败；支持从原生历史恢复图片并继续对话。
  [PR #123](https://github.com/necokeine/ait/pull/123)
- **Ait 项目配置与运行路径。** 工作区归属读取 server 元数据，项目配置默认写入 `ait.json`，
  兼容读取旧项目文件；附件缓存、搜索目录和克隆临时目录使用 Ait 名称。
  [PR #122](https://github.com/necokeine/ait/pull/122)
- **清理退役功能并统一客户端依赖。** 移除 relay 配对、插件界面与运行时、旧 Node CLI 安装入口；
  桌面、移动端及共享库统一为私有 `@ait/*` 本地 workspace，并修复原生测试的认证连接准备。
  [PR #122](https://github.com/necokeine/ait/pull/122)、
  [PR #126](https://github.com/necokeine/ait/pull/126)

  0.0.7 重建包已有的 worktree 创建、标题与 Git 文案生成、会话导入能力修复继续包含在本版中。

## 安装与兼容

计划发布平台延续现有工作流：macOS Apple Silicon（macOS 13+）的 DMG/ZIP，
Linux x86_64 的 AppImage/tar.gz；不新增 Windows、Intel Mac 或 Linux arm64 安装包。
安装包只内置独立 Rust `server`。正式发布完成后，0.0.7 用户可通过应用更新检查升级，
也可从 [GitHub Releases](https://github.com/necokeine/ait/releases) 下载对应安装包。

旧 relay 连接和插件面板在状态恢复时丢弃；连接 Ait 服务请使用直接连接或已支持的 SSH 入口。
项目旧配置仍可读取，保存使用 `ait.json`，不覆盖旧文件。Ait 的用户数据隔离策略保持不变，
既有 Paseo 数据不自动迁移或删除。Worktree 的 PR/Forge checkout 仍不支持。

## 准备验证

以下检查在 macOS arm64 上执行，验证对象为 `1151611d` 加本 PR 的 0.0.8 版本准备改动。

| 检查                                                                                                  | 结果                                                          |
| ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `npm ci --offline --no-audit --no-fund`                                                               | 通过，安装脚本执行成功                                        |
| `npm run verify:release -- v0.0.8`                                                                    | 通过，7 个 npm 包与 Rust workspace 版本一致                   |
| `npm run verify:local-packages`                                                                       | 通过，6 个私有 workspace 使用本地依赖链接                     |
| `cargo metadata --locked --offline --no-deps --format-version 1`                                      | 通过，12 个 Cargo 包及 28 处版本约束一致                      |
| Cargo/npm 锁文件语义比较                                                                              | 仅本地包版本变化，第三方依赖未变                              |
| `npm run test:release`                                                                                | 12 passed，0 failed                                           |
| `npm run build:desktop-main`                                                                          | protocol、client、highlight、audio 与 Electron 主进程构建通过 |
| `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`                                  | 通过                                                          |
| `npm run test --workspace=@ait/mobile -- --project unit native-release-version.test.ts src/changelog` | 3 文件、46 passed，0 failed                                   |
| 原生版本派生                                                                                          | App `0.0.8`、Android versionCode `8`、iOS buildNumber `8999`  |
| Electron Web 导出                                                                                     | 通过，命令见下                                                |
| 独立 server 构建与打包暂存                                                                            | 通过，实际 `server --version` 为 `server 0.0.8`               |
| `cargo fmt --all --check`、改动 JSON/Markdown 的 `oxfmt --check`、文档链接与 `git diff --check`       | 通过                                                          |

```sh
CI=1 EXPO_NO_TELEMETRY=1 npm exec --workspace=@ait/mobile -- \
  cross-env AIT_WEB_PLATFORM=electron expo export --platform web
CARGO_TARGET_DIR=/private/tmp/ait-release-0.0.8-target \
  cargo build --locked --offline -p daemon --bin daemon
/private/tmp/ait-release-0.0.8-target/debug/daemon --version
AIT_SERVER_BIN=/private/tmp/ait-release-0.0.8-target/debug/daemon \
  node apps/desktop/scripts/prepare-server.mjs
```

准备基线的 [main CI](https://github.com/necokeine/ait/actions/runs/36350606507) 已通过。
正式发布的 Linux/macOS 打包、Apple 签名、公证和成品启动验证由标签触发的发布工作流执行；
本地 server 使用 debug 构建，只验证版本与资源暂存，不作为正式发布资产。

## 发布步骤

1. 合并本次版本 PR，并确认 main 的 Rust/UI CI 通过。Cargo 与 npm manifest 均有变化，
   因此两个 CI job 都会触发。
2. 在包含本次版本提交的 main 上创建新的不可变标签：

   ```sh
   git switch main
   git pull --ff-only
   npm run verify:release -- v0.0.8
   git tag -a v0.0.8 -m "Ait v0.0.8"
   git push origin v0.0.8
   ```

3. 等待 Release Ait 两个平台构建及发布步骤全部通过。工作流创建 `Ait v0.0.8`，上传安装包、
   更新 YAML、macOS blockmap、`BUILD-INFO.json` 和 `SHA256SUMS`。
4. 核对 `BUILD-INFO.json` 的源码提交对应 `v0.0.8`，检查 10 个发布资产、SHA-256 和更新
   元数据版本，并在 Release 说明中补充本文的主要更新和兼容说明。

失败恢复、签名凭据和本机打包说明见[发布操作指南](../../../operations/releasing.md)。

## Test coverage

**本次未测量行覆盖率**：仅版本元数据和文档变化，没有修改 `bins/` 或 `crates/` 中的
Rust 代码；按仓库规则跳过本地 Rust 测试与覆盖率。准备验证结果与覆盖率分开记录，
不把历史覆盖率当作本次版本的新测量。后续行为变更按相应模块及提交准备规则测量；
发布安装包的启动、资源和更新元数据另由发布工作流验证。
