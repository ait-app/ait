# Server 会话空标题修复

- 日期：2026-09-27。
- 基线：`db506dd4d5137211782d59e4d419104095e97f94` 加本次标题与 CI 修复；产物记录测量源码的 SHA-256。
- 范围：独立 `provider` 的 Agent/session 展示标题；不改变领域、协议或存储 schema 边界。

## 原因与修复

新建 session 只保存客户端显式提供的 `config.title`，后续消息没有填充标题；导入只复制
原生 `name`，忽略首条消息预览；刷新与加载历史也没有补齐缺失标题。
已有 `title` 会被正确投影到创建、查询、列表和历史接口，前端 transport 没有丢弃该字段。

现在首次文本输入为尚未命名的公开 session 保存标题；导入及刷新按原生标题、原生首条消息
预览、历史中首个非空用户文本的顺序补齐。空白和控制字符折叠为空格，按协议限制保留最多
200 个 UTF-16 单位，不截断 Unicode 字符。手工命名、已有标题和内部 Agent 不会被覆盖。

启动时只从本地当前 timeline 分代读取旧空标题记录的首个非空用户文本，不启动 Provider，
不改变历史消息、归档状态或活动时间；尚未投影到本地的旧历史在首次加载时补齐。
更新通过 registry 的原子 `update` 再次检查标题，避免覆盖并发手工重命名。

## 限制

本报告对应的初版修复仅提供已有文本的临时标题。后续
[metadata generation](metadata-generation.md) 已接入四类有界生成、配置与 Provider
回退、手工改名保护；本报告的测试与覆盖率仍仅描述初版修订。
没有标题、预览或用户文本的会话仍保留空标题。已有空标题但尚无本地历史的会话，需要打开
历史或显式刷新后补齐；不在启动时逐一访问原生 Provider。

## 验证

回归覆盖首次输入随创建提交、延迟提交首条消息、创建回执及 get/list/history 返回、
后续消息保留原标题、导入和刷新时的三级回退、手工重命名保护、重启补齐且无 Provider
调用、首次加载原生历史补齐、空白及多字节长度限制、内部记录保护、存储失败与当前分代读取。
Provider 使用离线协议替身；未操作用户的运行实例，也未进行桌面 UI 实测。

## Test coverage

准备提交时清理旧覆盖率数据，并成功完成当前 revision 的全 workspace 测量：

| 范围                                           | 已覆盖 / 总行数 | 行覆盖率 |
| ---------------------------------------------- | --------------- | -------- |
| workspace                                      | 57,796 / 67,756 | 85.30%   |
| `provider`                                     | 14,537 / 15,679 | 92.72%   |
| `filesystem`                                   | 7,576 / 8,629   | 87.80%   |
| 新增标题模块 `service/agent_manager/titles.rs` | 88 / 88         | 100.00%  |
| `storage/timeline.rs`                          | 291 / 293       | 99.32%   |
| 文件订阅 `connection/files/connection.rs`      | 241 / 256       | 94.14%   |

精确命令：

```sh
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov clean --workspace
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov --workspace --html
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-pr-workspace-coverage.json
```

范围为本文基线加本次修改、workspace 默认 features、macOS arm64，没有额外文件排除。
工具默认排除测试源码且未插桩 doctest；8 项既有 ignored 测试未启用，Linux 和 Windows
未在本地测量。没有同范围 baseline，故不报告增减。新增标题行为已覆盖；未覆盖部分仍包含
其他原生 session 发现/恢复和错误分支，真实 Provider 与 UI 验收仍需在对应环境执行。

可评审产物：[逐文件行覆盖率与源码 SHA-256](server-session-titles-coverage.json)。
HTML 本地报告位于 `/private/tmp/ait-session-titles-coverage/llvm-cov/html/index.html`；
共享的 JSON 产物不依赖本地 HTML 路径。

测试执行与覆盖率分开记录：

- `CARGO_TARGET_DIR=/private/tmp/ait-agent-interface-target cargo test --workspace`：1717 通过，0 失败，8 ignored。
- 完整 workspace 覆盖率测试：1716 通过，0 失败，8 ignored；与普通测试相差的 1 项为 doctest。
- `CARGO_TARGET_DIR=/private/tmp/ait-agent-interface-target cargo build --workspace` 通过，无编译警告。
- `CARGO_TARGET_DIR=/private/tmp/ait-agent-interface-target cargo clippy --workspace --all-targets -- -D warnings` 通过。
- `cargo fmt --all --check`、`git diff --check` 通过。
- 普通测试和覆盖率均在获准的沙箱外环境执行，以允许本地回环端口和离线进程夹具。
