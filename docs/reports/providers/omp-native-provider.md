# Oh My Pi 原生 Provider 验证

日期：2026-10-21。分支基于 `origin/main`（`29fa17f6`），
验证范围为本次未提交的 OMP 适配器、装配、Provider 目录和客户端配置修改。
平台：Linux x86_64。原生 CLI 为 OMP `18.12.5`。
Pi 是另一个独立 Provider（ADR-156），不在本报告范围。

## 实现范围

`local/omp` 是独立 Rust 适配器，拥有传输、配置、流式投影、原生历史和测试：

- `--mode rpc-ui` 启动，ready 后协商协议 v2；分块按身份、顺序、数量、总长度和
  base64 校验，物理帧 1 MiB、重组 64 MiB。
- `prompt_result` 与 `session_settled` 区分接纳、yield 和真正空闲；
  `session_settled` 仅在当前 prompt 的关联结果到达后才被接受，
  上一轮迟到的 settled 不会提前结束下一轮（含原生后台续跑）。
- Ait 权限模式映射 `full/write/ask/ask-cached` → `yolo/write/always-ask/always-ask-cached`。
- 原生 `ask/select/confirm/input` 扩展对话映射为 Ait 审批或结构化问答；
  原生子 Agent 生命周期与消息进入 Ait 子 Agent 界面和各自时间线。
- 原生历史直接读取 OMP 会话 JSONL：会话列表、导入、当前分支投影、空会话恢复；
  已提交输入的历史文件丢失时返回错误，不创建替代对话或重发输入。

## Test execution

定向 Provider 测试通过 **27 项**（含真实 CLI 烟雾测试，无模型推理），可复现：

```sh
TMPDIR="$PWD/target/ait-test-tmp" CARGO_INCREMENTAL=0 \
PATH="$PWD/target/omp-smoke/node_modules/@oven/bun-linux-x64/bin:$PATH" \
AIT_TEST_OMP_BIN="$PWD/target/omp-smoke/node_modules/.bin/omp" \
cargo test -p provider --lib -- local::omp composition::tests \
  --include-ignored --skip installed_omp_real_inference --test-threads=4
```

- 离线 Python 协议夹具覆盖 22 项：多轮与后台续跑的回合结束、AgentManager 持久化与
  重启恢复、空 Agent 重开、模式/问答/子 Agent/分块、发现/命令/诊断不写历史、
  富输入与历史投影一致、steer 与取消、问答精确校验、畸形输出不重发输入。
- `local::omp::tests::installed` 默认 ignored，显式提供 `AIT_TEST_OMP_BIN` 后运行，
  使用回环地址的无认证本地模型定义，验证发现、命令、启动、关闭、空历史读取与恢复，
  不提交推理请求，不使用真实 Provider 凭据。
- composition 装配测试确认 `omp` 进入 Provider 目录且不产生启动写入。

daemon 两项实际进程测试通过（新 Provider 进入状态、诊断和配置接口，7 个 Provider）：

```sh
TMPDIR=/tmp/ait-verify CARGO_INCREMENTAL=0 cargo test -p daemon --test process -- \
  daemon::missing_provider_is_reported_and_editors_return_migration_responses \
  daemon::daemon_status_config_diagnostics_and_update_match_canonical_contract
```

客户端测试共 **6 项**通过：`provider-manifest.omp.test.ts`（2 项，OMP 默认启用、
四档权限模式文案与 `getUnattendedModeId`）与
`provider-command-templates.test.ts`（4 项，含 `omp --resume` 模板）。

### 真实模型调用

按用户授权，从本机 OpenCode 配置读取 FuturePPO 的 endpoint 和凭据
（`https://api.futureppo.top/v1`），测试配置与工作目录隔离，
密钥只通过环境传递；模型配置保存环境变量名，不保存密钥值。

- `deepseek-v4.1-flash`：`GET /models` 返回 200；推理收到上游
  `429 rate_limit_exceeded`，输出 token 为 0。绕过 OMP 直接向 `/chat/completions`
  发送最小 HTTP 请求同样得到 429，失败证据指向上游限流。
- 切换 `z-ai/glm-5.3` 后，Ait 的 OMP 适配器完整通过真实付费推理测试（124.76 秒）：
  流式回复与非零 token、临时文件写入与写入审批、结构化 ask、
  历史投影与实时时间线一致、会话列表，以及关闭后恢复上下文——
  恢复后的模型正确返回第一轮生成的随机标记，原生 session ID 保持一致。
- 真实多轮测试暴露上一轮迟到的 `session_settled` 被误用于新 prompt 的缺陷；
  修复要求当前 prompt 的关联结果到达后才接受无关联的 settled，
  保留原生后台续跑与自主回合处理，新增连续四轮（含后台续跑）的离线回归测试。

可复用的付费推理测试默认 ignored，须显式设置 `AIT_TEST_OMP_BIN`、
`AIT_TEST_OMP_REAL_BASE_URL`、`AIT_TEST_OMP_REAL_MODEL`、`AIT_TEST_OMP_REAL_API_KEY`，
并确保 OMP 所需 Bun 在 PATH 中：

```sh
TMPDIR="$PWD/target/ait-test-tmp" CARGO_INCREMENTAL=0 \
cargo test -p provider installed_omp_real_inference_tools_questions_and_resume \
  -- --ignored --nocapture --test-threads=1
```

## 格式、静态检查和构建

- `cargo fmt --all --check`、`git diff --check`、`node scripts/check-docs.mjs` 通过。
- `cargo clippy -p provider --all-targets -- -D warnings -A clippy::assert_is_empty` 通过；
  当前 Rust 工具链新增的 `assert_is_empty` 对仓库既有测试产生告警，
  仅在此次验证中豁免，未修改 workspace lint。
- `cargo check --workspace` 通过（CI 工具链 1.98.1；本机为 1.99.0，
  其新增 lint 对未改动 crate 的告警与本次适配无关）。

## Test coverage

**Not measured**。按仓库规范只执行改动及直接相关行为的定向测试，
未运行 workspace 全套或覆盖率。准备提交时可执行 `cargo llvm-cov --workspace --html`
补充测量。

## 未验证范围

OMP 子 Agent、图片、取消和追加输入仅使用离线协议夹具验证，
未进行这些特性的远程模型测试；未执行桌面人工交互、macOS 或 Windows 运行验证。
未实现的控制不声明支持；会话/文件回退、每会话 MCP/工具策略覆盖、
结构化输出 schema、OMP 子 Agent 独立控制及宿主工具注册等边界见
[运维文档](../../operations/oh-my-pi.md)。
