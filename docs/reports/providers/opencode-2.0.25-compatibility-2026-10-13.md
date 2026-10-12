# OpenCode 2.0.25 深度兼容性检查

日期：2026-10-13。源码基线：`586161900d569d7f633132b783bbe70ebc74fb38`，加本次工作树修改。
环境：macOS arm64、Rust / Cargo 1.98.1、默认 features。
决策见 [ADR-143](../../decisions/providers/adr-143-opencode-2-0-25-compatibility.md)。

## 已确认的原因与修复

| 问题                                      | 证据                                                                                  | 修改                                                                                   |
| ----------------------------------------- | ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| 2.0.25 被整体拒绝，无法发现目录或创建会话 | 修改前，本机 2.0.25 通过“必须升级到 2.0.26”的旧测试，ACP 根本没有启动                 | 最低 2.x 改为 2.0.25，并同步诊断、夹具和边界测试                                       |
| 多行或长 shell 命令审批触发 AgentIo       | 真实 CLI 用 `# ` 加 80 段注释及换行后的 `pwd` 复现；旧解析器在收到审批时返回 `Failed` | 展示标题不再使用短标识符校验，保留完整输入；ID 校验不变                                |
| 大文件编辑审批触发 AgentIo                | 真实 CLI 先 read 后 edit 115,016 字节临时文件；原解析器失败，恢复修复后通过           | 只对保留的审批字段计算 64 KiB 预算，忽略未保留的全文件 diff；传输帧仍受 8 MiB 上限约束 |
| 初始模型选择失败遗留空原生会话            | `session/new` 成功后 `config::apply` 直接传播错误，未清理                             | 有原生 delete 时删除本次创建的空会话；恢复失败保留原会话与历史                         |

`AgentIo` 是上层对 `AgentSessionError::Failed` 的通用映射，不能当作唯一根因。
这里有两项独立的真实审批复现；不宣称已经逐条解释用户未提供的历史错误日志。

## 为什么不是必须升级到 2.0.26

此前报告验证的是 2.0.20 与 2.0.26，没有在 2.0.25 上运行完整流程，就把整个中间区间拒绝了。
实际修复 [fb72e25c：等待冷目录插件后读取目录](https://github.com/anomalyco/opencode/commit/fb72e25c0607b91b66775a8e401e4af718c690c2)
已包含在 [2.0.24 → 2.0.25](https://github.com/anomalyco/opencode/compare/v2.0.24...v2.0.25) 中。
[2.0.25 → 2.0.26](https://github.com/anomalyco/opencode/compare/v2.0.25...v2.0.26)
没有 ACP 目录下的源码改动，两版 `catalog.ts` 内容一致。
2.0.25 已有表单、会话删除、恢复和动态 config options，实际本机测试确认这些流程可用。

最低版本修正不删除已有的异步配置确认、启动互斥和原生历史结算。
保留早于 2.0.25 的限制，未把本次结果外推到未测试版本。

## 测试范围

本机可执行文件：`/opt/homebrew/bin/opencode`，指向 Homebrew 2.0.25。
对照文件：`/private/tmp/ait-opencode-v26/package/bin/opencode`，已有的官方 2.0.26 临时安装。
没有替换本机 OpenCode 或改写用户认证、项目配置、数据库、已有会话。
另行只读统计本机全局配置结构：旧式 `provider`、1 个 provider、13 个配置模型，显式 `plugin` 列表为空。
为此增加同结构的脱敏 loopback 夹具，两版都发现全部 13 个模型并成功执行，原配置文件内容保持不变。
这个测试验证原生旧格式迁移，不代表已向用户真实服务提交请求或逐项验证其私有配置。

原生测试各自创建临时 XDG / config / test home / SQLite 目录；模型通过测试自己的 loopback HTTP 服务返回确定性结果。
不调用真实模型服务或使用用户推理额度。实际 shell 只在临时目录运行 `pwd`；文件编辑只修改测试创建的文件。

原生流程包括：

- 多轮输入、增量输出、稳定 client message ID、持久 timeline 顺序、原生历史重放、导入与恢复。
- 模型与 effort / mode 切换，权限与 system prompt 改动后的同身份重连。
- 64 个自定义模型的完整目录、最后一个模型的实际执行，以及四个项目共享隔离原生 home 的并发冷启动。
- Provider snapshot ready、刷新确认后重新读取目录、新配置模型出现、临时查询会话全部删除。
- 原生 shell 工具输出、换行及长命令审批、115 KB 文件的 read/edit 审批与重放。
- once 再次询问、always 在下一轮复用原生许可、拒绝审批后继续、原生问答回答到达模型。
- 图片、文本附件和上传文件元信息；已进入模型请求的取消及后续输入。
- 原生认证/模型错误正确失败并保留输入历史；五个相同配置会话保持不同身份。
- 无工具摘要与原生临时历史删除；创建失败清理与恢复失败保留旧历史。

默认无网络夹具改为 2.0.25，仍显式覆盖 1.x、2.0.26 和后续 2.x 配置。
共享 ACP 审批改动额外验证 DeepSeek Harness ACP 用例。
未运行完整 workspace 测试，也未执行其他 provider 的外部安装测试。

## 验证命令与结果

| 范围                                      | 结果                                            |
| ----------------------------------------- | ----------------------------------------------- |
| OpenCode 单元与 ACP 夹具                  | 55 passed；20 个原生安装测试默认 ignored        |
| 共享 ACP 审批                             | 5 passed                                        |
| DeepSeek Harness ACP                      | 8 passed                                        |
| Provider catalog                          | 25 passed                                       |
| 真实 OpenCode 2.0.25                      | 18 passed（17 项流程矩阵，追加 1 项旧格式配置） |
| 真实 OpenCode 2.0.26                      | 18 passed（同上）                               |
| 真实 daemon / WebSocket + 2.0.25 ACP 夹具 | 3 passed                                        |

共 **132 项定向测试通过**，0 failed。两项仅适用于原生 1.x / 旧 2.x 的安装测试未执行。
workspace build、workspace all-targets Clippy（`-D warnings`）、Rustfmt、文档格式、文档链接和 diff 空白检查通过。

原生测试先完成各 17 项矩阵，再单独追加各 1 项旧格式配置用例；下面的 installed 模块命令在最终源码上包含全部 18 项。
初次 loopback 测试被工具沙箱拒绝监听，随后在授权的测试环境重跑；这不是 OpenCode 的产品故障。
多行审批及大文件编辑的失败复现先于修复，后者另用原始审批解析器反向验证一次，确认旧代码返回 `Failed`。
新增 snapshot refresh 测试初版误把 refresh acknowledgement 当作 snapshot；改为确认 acknowledgement 后读取 snapshot，最终通过。

```sh
cargo test --locked --offline -p provider --lib local::opencode
cargo test --locked --offline -p provider --lib local::acp_permissions
cargo test --locked --offline -p provider --lib local::deepseek_harness::tests
cargo test --locked --offline -p provider --lib service::provider_catalog
AIT_TEST_OPENCODE_BIN=/opt/homebrew/bin/opencode cargo test --locked --offline -p provider --lib local::opencode::tests::installed -- --ignored --skip installed_acp_without_native_forms --skip installed_acp_broken_v2
AIT_TEST_OPENCODE_BIN=/private/tmp/ait-opencode-v26/package/bin/opencode cargo test --locked --offline -p provider --lib local::opencode::tests::installed -- --ignored --skip installed_acp_without_native_forms --skip installed_acp_broken_v2
AIT_TEST_OPENCODE_BIN=/opt/homebrew/bin/opencode cargo test --locked --offline -p provider --lib installed_acp_legacy_provider -- --ignored
AIT_TEST_OPENCODE_BIN=/private/tmp/ait-opencode-v26/package/bin/opencode cargo test --locked --offline -p provider --lib installed_acp_legacy_provider -- --ignored
cargo test --locked --offline -p daemon --test process opencode
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo build --locked --offline --workspace
cargo fmt --all --check
node_modules/.bin/oxfmt --check docs/README.md docs/decisions/providers/adr-115-opencode-acp-provider.md docs/decisions/providers/adr-135-opencode-catalog-readiness.md docs/decisions/providers/adr-143-opencode-2-0-25-compatibility.md docs/reports/providers/opencode-2.0.25-compatibility-2026-10-13.md
npm run check:docs
git diff --check
```

## 慢插件的独立原生限制

另行构造一个 2.x 本地 Promise 插件：`setup` 等待 1,500 ms 后，经 `context.provider.transform`
复制配置模型并注册 `local/delayed-model`，将原生 `model` 配置设为这个晚注册的模型。
2.0.25 和 2.0.26 都可在该插件完成前返回一个成功但缺少此模型的 ACP 首次目录，default 还可能退回内置模型。
该失败复现不计入通过数，也未通过放宽断言或添加固定 sleep 将它标为成功。
最终配置目录测试仅验证直接配置模型的发现与刷新，不代表这个异步插件场景已修好。

原生 [catalog.ts](https://github.com/anomalyco/opencode/blob/v2.0.25/packages/cli/src/acp/catalog.ts)
等待插件库存非空，并监听之后的目录变化；它没有等待每个异步插件 setup 完成。
原生 [ACP service](https://github.com/anomalyco/opencode/blob/v2.0.25/packages/cli/src/acp/service.ts)
也未提供“所有模型已注册完成”的 ACP 控制方法。
因此，仅升级到 2.0.26 不能消除此限制，当前一次性目录发现仍不能保证包含任意晚注册模型。
如果用户使用此类插件，应让必需模型先出现在原生 `providers.<id>.models` 配置中，
或在原生 ACP 提供明确就绪能力后再接入；本次没有自动修改用户配置。

对照了 Paseo 提交 `9551b2cc11328170416b3d9753c7ec9a10ed35e7` 的
[readiness](https://github.com/getpaseo/paseo/blob/9551b2cc11328170416b3d9753c7ec9a10ed35e7/packages/server/src/server/agent/providers/opencode/v2/readiness.ts)、
[runtime](https://github.com/getpaseo/paseo/blob/9551b2cc11328170416b3d9753c7ec9a10ed35e7/packages/server/src/server/agent/providers/opencode/v2/runtime.ts)
及插件入口实现。它的 v2 路径管理 HTTP server 并查询 plugin API，并非 Ait 当前的 ACP-only 路径；
普通位置就绪同样检查库存非空，对自有 bridge 才检查 active。
没有证据证明直接照搬这段判断就能解决所有自定义模型插件的异步注册。

## Test coverage

**not measured**。本次请求是详细功能测试，没有请求覆盖率测量；不复用旧报告的百分比。
实际执行范围为上面列出的定向测试与两版真实 CLI，不包含完整 workspace 测试或 coverage。
如果后续需要测量，使用 `cargo llvm-cov` 对 OpenCode、共享 ACP 审批及直接相关 catalog 测试生成当前源码的报告。

Windows / Linux 原生二进制、真实付费服务、用户实际插件配置和桌面 UI 手动操作未验证。
64 KiB 保留字段和 8 MiB 单帧仍是明确资源边界；超限请求仍会拒绝，不宣称支持无限大小工具输入。
