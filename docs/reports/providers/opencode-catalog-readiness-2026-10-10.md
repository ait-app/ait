# OpenCode 模型目录与执行失败修复验证

2026-10-10；基于 `dfcbb49f3a68be567ffa9dc6a378a3176cbed8a3` 的未提交工作树。
边界决策见 [ADR-134](../../decisions/providers/adr-134-opencode-catalog-readiness.md)。

## PR 整理验证（2026-10-11）

本次 PR 将原始改动整合到 `main` 的 `8cdeff9baac82495e1cef45c3cb9752c66a45a28`，
保留主分支新增的原生错误、五会话所有权和持久化 Timeline 测试，并将冲突的 ADR 编号改为 134。
以下初次验证结果对应 `a9c03d15b0e7afa1aa3b01ab620476e239c7d19d`，macOS arm64 / Rust 1.98.1，默认 features。

- `cargo test --locked -p provider --lib local::opencode`：55 passed，14 ignored，0 failed。
- `cargo test --locked -p provider --lib service::provider_catalog`：25 passed，0 failed。
- `AIT_TEST_OPENCODE_BIN=/private/tmp/ait-opencode-v26/package/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed -- --ignored --skip installed_acp_without_native_forms --skip installed_acp_broken_v2`：12 passed，0 failed。
- `AIT_TEST_OPENCODE_BIN=/opt/homebrew/bin/opencode cargo test --locked -p provider --lib installed_acp_broken_v2 -- --ignored`：1 passed，0 failed；仅检查 2.0.20 的升级提示，不创建原生会话。
- `cargo test --locked -p daemon --test process opencode`：3 passed，0 failed；真实 daemon/WebSocket/持久化与无网络 ACP 夹具。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo build --locked --workspace`、`cargo fmt --all --check`、`npm run check:docs`、`git diff --check` 通过。

共 96 项 Rust 定向测试通过；14 项 ignored 安装用例中，12 项在 2.0.26 运行，1 项在 2.0.20 运行。
原生 1.x 安装用例、Windows 和真实付费模型未运行。原生测试使用临时隔离配置与 loopback 模型，
未使用用户凭据。没有运行完整 Rust workspace 测试。

## Rust CI 断言修正（2026-10-11）

初次 [Rust CI](https://github.com/ait-app/ait/actions/runs/38138998001/job/114468766116)
在 Ubuntu / Rust 1.98.1 完成编译和 Clippy，nextest 执行 2087 项测试：2086 passed、1 failed、23 skipped。
唯一失败是 `daemon_status_config_diagnostics_and_update_match_canonical_contract`：
Provider 目录开始显示适配器诊断后，daemon 状态测试仍期待统一的旧错误文案。
本地定向运行复现了相同断言差异。

修正仅更新测试中四个不可用 Provider 的预期诊断文案，继续精确校验 Provider 顺序、可用性和完整诊断，
没有修改生产行为。修正后的直接相关 daemon 模块使用 CI 相同配置验证：
`cargo nextest run --locked -p daemon --test process --profile ci -E 'test(unix::daemon::)'`，
3 passed、93 skipped。
`cargo test --locked -p daemon --test process daemon_status_config_diagnostics_and_update_match_canonical_contract`
另行确认同一用例通过；`cargo clippy --locked -p daemon --test process -- -D warnings`、
`cargo fmt --all --check`、`npm run check:docs` 和 `git diff --check` 通过。
此前的 96 项验证仍对应上面注明的初次提交。
本次覆盖率状态仍为 **not measured**；未请求覆盖率，也未在本地运行完整 workspace 测试。

## 诊断与结果

上传日志来自 Ait 0.0.26，两台主机均使用 OpenCode 2.0.20。
以隔离配置和 loopback 模型运行原有真实 CLI 多轮测试，创建阶段返回 `Rejected`，没有提交 prompt。
直接原生 ACP 查询在项目配置含两个自定义模型时只返回默认免费模型。

上游 2.0.20 的首次目录缓存未等待配置插件；2.0.26 增加插件就绪等待和原生事件刷新。
依据版本固定的上游实现与本机复现，2.x 最低支持提高到 2.0.26。
旧版本在创建会话前返回 unavailable，Provider 快照和诊断展示版本与升级提示。
1.x 保留原有行为。未修改用户 CLI 安装、认证或配置。

真实 2.0.26 验证还观察到：模型切换成功响应偶尔包含旧模型，之后通知才确认选值。
适配层现在等待同一 session 的确认，响应前后通知均可接收，控制期限内没有确认则关闭连接。
模型、effort、mode 共用该流程，不重发原生写操作或 prompt。
目录查询跟随最新模型列表，发布最终列表和模式；纳入新增模型、删除消失模型，并保留最新名称与描述。

并发压力验证进一步复现 2.0.26 原生共享数据库的冷启动竞争：多个 ACP standalone 进程
在 `initialize` 前报 `SQLiteError: database is locked`，造成目录发现失败。
适配器及其克隆现在只在 ACP spawn 到 initialize 完成期间协调启动；等待有期限，失败释放 gate，
活跃会话、查询和 prompt 保持并发。四个项目同时发现和后续默认模型执行已作为真实 CLI 回归用例。
外部 OpenCode 进程不参与 Ait 的 gate；未宣称解决所有外部数据库锁竞争。

日志还包含旧插件格式的原生加载失败警告和模型旧配置字段被省略的诊断。
其迁移属于 OpenCode 配置；本次不据此改写用户文件。不能由这些警告确认单次 agent 失败原因。
`ResourceExhausted` 在日志中只记录 request ID，缺少方法及预算来源；本次不宣称已确认其触发来源或解决全部资源拒绝。

## 历史验证（2026-10-10）

macOS arm64 / Rust 1.98.1，默认 features；仅运行改动代码与直接相关行为。

- `cargo test -p provider local::opencode -- --nocapture`：52 passed，13 ignored，0 failed。
- `cargo test -p provider service::provider_catalog -- --nocapture`：25 passed，0 failed。
- `AIT_TEST_OPENCODE_BIN=/private/tmp/ait-opencode-v26/package/bin/opencode cargo test -p provider local::opencode::tests::installed -- --ignored --skip installed_acp_without_native_forms --skip installed_acp_broken_v2 --nocapture`：11 passed，0 failed。
- `AIT_TEST_OPENCODE_BIN=/opt/homebrew/bin/opencode cargo test -p provider installed_acp_broken_v2 -- --ignored --nocapture`：1 passed，0 failed。
- `cargo test -p daemon --test process opencode -- --nocapture`：3 passed，0 failed；真实 daemon/WebSocket/持久化与不可变 ACP 夹具，未使用真实 OpenCode CLI。
- `cargo clippy -p provider --all-targets -- -D warnings`、`cargo fmt --all --check`、`git diff --check` 通过。

原生 2.0.26 来自官方 `@opencode/cli-darwin-arm64` npm 包，仅解压至临时目录。
原生测试隔离 XDG、配置目录和 OpenCode test home，模型只调用 loopback 服务。
2.0.20 只验证版本检查及升级提示，不创建 ACP 会话。
共 92 项通过：52 项 OpenCode 定向测试、25 项 Provider 目录测试、12 项真实 native 版本测试和 3 项 daemon 流程测试。
13 项 ignored 原生用例中，11 项在 2.0.26 运行，1 项在 2.0.20 运行；只有 1.x 原生安装用例未执行。

新增的六项真实流程验证具体断言：

- reasoning 模型的两档 effort 正确发现，切换到无 effort 模型后清除原生 effort，恢复后再次选回 reasoning 模型。
- mode、permission 和 system 改动后重连仍保持原生 session ID、先前上下文及实际发送的模型参数。
- 64 个自定义模型均恰好出现一次，可选择最后一个；目录发现不调用模型，不留下原生查询会话。
- 四个项目同时冷启动，互不泄露模型与默认值，各自默认模型能实际执行。
- 文本附件、上传文件元信息和有效 PNG 到达模型，客户端消息 ID 和原生历史回放一致。
- 已到模型端的请求取消后得到原生取消确认，可继续下一回合；401 认证失败有界结束且不报告成功。

夹具还覆盖动态增删/重命名模型、不可用快照、确认顺序、缺失确认、错误 session、启动 gate 超时/失败释放、审批、问答、工具、多轮、恢复和辅助清理。
测试请求按前台 user 内容关联并排除原生标题生成，避免后台模型请求造成假阳性。
Windows、真实付费模型和当前真实用户配置未验证；原生 1.x 安装测试保持 ignored，夹具仍验证两种主版本配置。

## Test coverage

本次基于最新 main 的整理：**not measured**。覆盖率未被请求，采用上面的定向回归测试验证；
未运行 workspace coverage。若后续需要测量，使用 `cargo llvm-cov` 重跑当前 OpenCode 与 Provider catalog
定向测试，并生成对应源码的共享报告。下面保留的结果仅为原始工作树的历史测量。

2026-10-10 原始工作树的定向行覆盖率 **92.65%（3027 / 3267 行）**；此历史测量不代表基于最新 main 整理后的代码。

测量范围为 OpenCode 生产实现、共享 ACP transport 和 Provider catalog 生产实现；测试与其他 crate 不计入该范围。
使用 `cargo llvm-cov` 运行上述两个定向模块，并追加真实 2.0.26 的十一项和 2.0.20 的一项测试。
精确命令、源码指纹、逐文件覆盖数和未覆盖行见 [覆盖率证据](opencode-catalog-readiness-coverage-2026-10-10.json)。
历史测量没有运行 workspace coverage；没有同一范围、同一基准的可比测量。

主要未覆盖项为 OS/管道异常、部分 malformed frame 和 form、意外控制 callback 与关闭超时。
后续可用故障注入补充这些分支；本次未为覆盖率扩展到无关测试或完整 workspace。
