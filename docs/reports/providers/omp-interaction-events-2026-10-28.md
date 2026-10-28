# OMP 交互事件链路验证

- 日期：2026-10-28
- 源码：`71134310` 加本次工作树修改，分支 `fix/omp-interaction-events`。
- 平台：Linux x86_64，官方 `omp/18.12.6`，系统 Rust/Clippy 1.99.1。
- 决策：[ADR-198](../../decisions/providers/adr-198-omp-interaction-progress.md)。

## 修复与证据

修复针对 OMP 共用会话链路。Agent manager 提交后会在开始轮询之前获取 runtime；
同步 `get_state` 和追加输入的确认等待可能让 `extension_ui_request` 留在 transport 缓冲区。
原生在等用户，宿主在等原生命令，最终只显示超时。

离线原生 peer 在对话回答之前延迟 prompt、状态读取和 steer 的确认。
保留修复前 `session.rs` 运行以下测试，两项均在 runtime/steering 等待处失败：

```sh
cargo test --locked -p provider local::omp::tests::interactions -- --nocapture
```

修复后，运行中的 runtime 信息使用同一实例最近验证的快照；追加输入的确认按 ID 和回合
异步消费。原生失败仍上报，旧回合迟到响应不影响新回合，未确认追加输入限制在 128 条。
状态核对和用户取消仍沿用既有异步结算逻辑。

共享夹具 `crates/provider/tests/fixtures/omp_dialogs.json` 覆盖：

| 场景           | 原生类型 | 验证                               |
| -------------- | -------- | ---------------------------------- |
| 写入和执行审批 | select   | 原生选项、单次批准、拒绝、答案回传 |
| 插件选择       | select   | 含逗号的选项、选项说明             |
| 确认           | confirm  | 标题、说明、允许/取消动作          |
| 文本输入       | input    | placeholder、文本回传              |
| 编辑器         | editor   | 多行文本回传                       |
| 结构化提问     | ask      | 多问题、稳定 answerKey、多选       |

Rust 验证夹具和 adapter 的实际输出、原生响应、取消后不能重复回答。
TypeScript 对同一请求执行共享 wire/AOT 校验和前端表单解析、答案生成。
组件测试实际渲染写入、执行、选择和文本输入组件，再点击提交核对原生答案。
多行编辑器通过显式 `multiline` 标记渲染，提交时保留换行。
manager 测试验证原生尚未确认时已经发布 permission attention，重连快照保留待处理请求。

官方 CLI 测试使用临时配置、通用测试插件和 loopback mock 模型，无用户认证或付费推理。
实际调用原生 write/bash 并分别拒绝和批准；批准前文件不存在，批准后文件内容正确，
执行输出返回模型。confirm/input/editor 与结构化 ask 的回传同样通过官方 CLI 验证。

## 验证命令

Cargo 命令使用 `CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target` 复用编译缓存。

```sh
cargo test --locked -p provider local::omp:: -- --nocapture
AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -p provider local::omp::tests::installed -- --ignored --nocapture
npm run test --workspace=@ait/protocol -- src/messages.omp-dialogs.test.ts src/messages.omp-question.test.ts
npm run build --workspace=@ait/protocol
# 在 apps/mobile 执行：
../../node_modules/.bin/vitest run --project unit src/components/question-form-card.test.tsx src/components/question-form-card-core.test.ts
cargo fmt --all --check
cargo clippy --locked -p provider --all-targets -- -D warnings -A clippy::assert_is_empty
```

Rust 定向测试通过 82 项，默认跳过 10 项安装/实际推理测试。
其中 8 项官方安装测试另行显式运行；两个付费推理测试保持跳过。
协议测试通过 8 项；问答组件及表单逻辑测试通过 17 项。

系统 Clippy 比仓库指定的 Rust 1.98.1 更新。无豁免运行会因既有测试中的
`clippy::assert_is_empty` 新 lint 失败；本次新增断言已改为 `assert_eq!`。
验证仅豁免此新 lint，其余 warning 仍视为错误。未修改其他 provider 的既有测试。

没有执行全 workspace 测试，也没有启动或替换用户正在运行的 Ait。
组件验证在 jsdom 中执行；不代表已经在截图对应的桌面安装包上实测。
认证、启动扩展对话及协议未导出的任意 TUI 布局不在本次验证范围。

## Test coverage

**not measured**：用户未请求覆盖率测量，本次仅运行 OMP 和交互组件的定向测试。
通过项数是测试结果，不是行覆盖率。若后续需要，可用 `cargo llvm-cov` 对 `provider`
的 `local::omp::` 测试定向测量并生成可共享报告；本次没有覆盖率百分比或覆盖率工件。
