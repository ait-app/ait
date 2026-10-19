# Paseo 0.12.0 选择性移植与验证

日期：2026-10-20。已按“全部做掉”的范围实施研究清单中的八组改动、Markdown 引擎迁移、移动辅助面板／Scripts／Command Center／子 Agent 信息、Setup 按需日志，以及两项桌面修复。

## 固定范围

- Ait 初始基线：`808b9bcdc7267b68af6f844f9632de0fe4975d53`；下列实施验证针对该基线上的本次变更。
- PR 准备时无冲突同步到最新 `main`（`4475bff1`），保留新增的 attention-clear 响应映射修复，并补跑 transport 定向回归。
- Paseo：本地 `/Users/necokeine/Documents/paseo`，固定 `91154ba4e8ace3484595a05a4366279fb4818745`，相对上一批 `a1da1b8f234afe7a75785022292cdf8a50ee11a5` 选择性移植。
- 研究依据：[增量借鉴清单](paseo-upstream-opportunities-2026-10-19.md)、[客户端候选研究](paseo-client-candidates-2026-10-19.md)。研究文档记录的是实施前的固定基线。
- 没有引入 Paseo 的 Node 后端、Hub、Plugin 管理或旧服务代理字段。后端行为由现有 Rust 能力组实现。

## 最终行为

| 范围            | 改动与约束                                                                                                                                                                                                         |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Timeline 完整性 | catch-up 同时合并缓存 tail 和实时 head 的正文，避免命中实时副本后漏掉历史尾部。                                                                                                                                    |
| 缓存并发        | replica-cache 按目标实体判断读取失效，另一个 Agent 的流式更新不再废弃当前目标的有效缓存读取。                                                                                                                      |
| 终端            | 恢复和复制跳过宽字符 continuation，选中任一半可取得完整字形；修复词选择与长按释放。原生远距离跳到底部采用非动画定位。                                                                                              |
| Git             | Rust 解码 C-style quoted path；从 worktree 根目录执行 diff／discard；提交 diff 查询完整 rename 关系并包含前后路径，保留纯重命名。不可变 SHA 的 rename 发现使用 100 项、60 秒、并发共享缓存，错误和符号引用不缓存。 |
| 长聊天与布局    | 复用历史切片、工作区／子 Agent 投影与未变面板；避免流式 tick 重算整段历史和重复持久化布局。新增基准源文件可供后续测量，本次没有声称取得某个帧率或毫秒收益。                                                        |
| 用量            | 稳定已显示的卡片顺序；失败仍显示错误，同时可展示同一账号的上次成功值与成功时间。Host 和 live session 各自拥有缓存，账号变化、未知身份、会话恢复清除历史；旧值不进入 pinned summary。                               |
| 文件链接与预览  | 显式链接支持无已知扩展名的文件和目录；目录进入 Files 并展开目标；href 解码与原始 inline-code 路径分开。Markdown 相对图片以文件所在目录解析。                                                                       |
| Markdown        | 三端统一到固定的 enriched-markdown；保留 Ait 高亮、代码复制、Mermaid、Find、附件和远程用户图片。原生流式分段与三端渲染上限分别处理。图片高度缓存保留 Host／Workspace 隔离。                                        |
| 手机编辑与评审  | 长提示词全屏编辑、选区和草稿生命周期；Diff 评论 Sheet 和向 Agent 提交反馈，确认成功后只移除本次提交的评论版本，提交期间的新编辑保留。                                                                              |
| 手机工作区      | 浮动头部、文件／子 Agent 等辅助 Sheet、关闭后恢复焦点和显式转为标签页；Scripts 菜单提供启动、停止、重启、终端、服务与复制 URL。保留 Ait 日程、Bot 和服务访问方式。                                                 |
| 查找和信息      | Command Center 按匹配质量和近期使用排序并突出匹配；子 Agent 展示模型／effort；补齐代理通信工具的展示信息。                                                                                                         |
| Setup           | 全局只观察轻量状态；活动且应用可见的面板拥有详细日志订阅。订阅按物理连接和 Workspace 隔离，重连重建，revision 拒绝旧排队事件，空快照清除旧日志，后台失败提示继续有效。                                             |
| 桌面与问答      | Linux 在页面加载完成后补窗口显示兜底；主入口保护关闭 console 管道的 EPIPE。问题提交失败在对应表单内显示，保留答案和原有超时／重试能力。                                                                            |

耐久边界记录在 [ADR-169](../../decisions/clients/adr-169-shared-markdown-and-composer-lifetimes.md)、[ADR-167](../../decisions/providers/adr-167-account-scoped-successful-usage.md) 和 [ADR-168](../../decisions/workspace/adr-168-scoped-setup-observation.md)。来源说明更新在 [Paseo NOTICE](../../../third-party/paseo/NOTICE)。

## 兼容性与平台范围

- `successfulAt`、`lastSuccessful` 和 Setup revision 都是兼容性字段；旧报告不会被当作一次成功更新。Setup 新接口由方法目录协商；旧主机保留旧状态读取和全局 progress 路径。
- 无法独立确认原生账号身份时继续返回当前配额，但不缓存复用或保留上次成功值。当前包括 DSH 原生网页登录、Antigravity 与未实现身份预检的适配器。
- enriched-markdown 与 bottom-sheet 补丁一起纳入版本控制；数学与视频关闭，代码语言资源按应用配置准备。旧 Markdown 原生依赖已移除。iOS／Android 必须重建客户端二进制。
- 本次在 macOS 上执行 Rust、SDK、组件、Chromium 和打包检查。没有将生产 JS 导出等同于 iOS／Android 真机验收；原生跨段选择、键盘转场和 Linux 窗口实际显示仍需对应平台验证。
- 不包含完整 Rust workspace 测试或 workspace coverage；没有借用历史覆盖率，也没有把上游性能数据作为 Ait 的实测结果。

## 验证

本机验证结果如下。各行是独立范围，部分用例被相关 filter 重复覆盖，不将它们相加冒充唯一测试总数。

| 验证范围            | 命令／筛选                                                                                                        | 结果                                                                            |
| ------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| 移动端单元与组件    | `vitest run --project unit`，本次修改的测试及 file-explorer/tree、原桌面 Scripts 按钮、question-response 相关回归 | 57 文件，1151 项通过                                                            |
| Chromium            | Markdown 实际组件、Usage 卡片、Chat Find ranges／viewport、selection-copy、composer input                         | 6 文件，76 项通过                                                               |
| SDK 订阅            | `vitest run src/connection.test.ts src/connection/owned.test.ts`（client workspace）                              | 46 项通过                                                                       |
| Rust transport 适配 | `vitest run --project unit src/runtime/rust-daemon/transport.test.ts`（mobile workspace）                         | 42 项通过，包含最新 attention-clear 修复与新增 Setup 接口后的方法清单和通道预算 |
| 桌面                | `vitest run src/window/show-when-ready.test.ts src/diagnostics/console-streams.test.ts`                           | 8 项通过                                                                        |
| 终端协议            | `vitest run src/terminal-snapshot.test.ts`（protocol workspace）                                                  | 4 项通过                                                                        |
| Rust Git            | `cargo test -p filesystem git::local::checkout --lib`                                                             | 101 项通过                                                                      |
| Rust 用量           | `cargo test -p provider usage --lib`                                                                              | 33 项通过，3 项既有原生 CLI 冒烟测试 ignored                                    |
| Rust 认证／配额     | `cargo test -p provider inspection::tests --lib`；`cargo test -p provider native::quota --lib`                    | 分别 10 项、11 项通过                                                           |
| Rust 会话注册／恢复 | `cargo test -p provider service::agent_manager::tests --lib`                                                      | 73 项通过                                                                       |
| Setup               | `cargo test -p metadata automation:: --lib`                                                                       | 54 项通过，包含详细订阅的响应／事件顺序、Workspace 和物理连接隔离、释放及预算   |
| 共享 session        | `cargo test -p model session:: --lib`                                                                             | 8 项通过                                                                        |
| API session         | `cargo test -p api tests::session --lib`                                                                          | 1 项通过                                                                        |
| 能力组边界          | `cargo test -p metadata --test module_boundaries`                                                                 | 2 项通过                                                                        |

Rust 命令使用 `RUSTC_WRAPPER=` 避开沙箱不可用的本机 sccache。需要语音静态库的 build、Clippy 与 API 检查使用已有本地目录：`SHERPA_ONNX_LIB_DIR=/Users/necokeine/.ait-server-desktop/worktrees/2vfcj658/eatable-kangaroo/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib`。需要本地回环端口的测试在正常本机权限下运行；沙箱中的端口拒绝未当作业务失败。

- `cargo build --workspace` 通过；`cargo clippy --locked -p filesystem -p provider -p metadata -p model -p domain -p api --all-targets -- -D warnings` 通过。
- SDK／UI 依赖构建、移动端和桌面端 TypeScript 检查通过；修改文件 Oxfmt、UI lint、`cargo fmt --all --check`、源码 diff 空白检查与文档链接检查通过。生成的依赖 patch 保留补丁语法要求的空白上下文行，单独通过 patch-package 安装验证。
- `npm install --ignore-scripts --no-audit --no-fund` 后 `npm run postinstall` 在干净的 enriched-markdown 包上通过。补齐上游补丁遗漏的 Metro `src/web/renderers/BlockRenderers.tsx`，使源入口也使用 Ait 的代码块覆盖；实际 Chromium 测试验证语言标记、表格和跨段 Markdown／HTML 复制。
- `INIT_CWD="$PWD" node node_modules/react-native-enriched-markdown/postinstall.mjs` 完成固定语言资源准备，数学资源按配置跳过。
- `npm exec --workspace=@ait/mobile -- expo export --platform all --output-dir /private/tmp/ait-paseo-export-final` 通过，包含 Web 产物和 iOS／Android Hermes 包；本地产物是验证输出，不纳入 Git。

## PR CI 修复

修复推送时 `main` 新增后台错误策略与 ADR-166，造成决策索引冲突并阻止新 CI 运行。已合入 `cbd62e46`，保留主线后台错误策略，将本次 Markdown 决策改为 ADR-169 并同步索引与引用。合并后 query-client-errors 与 install-error-handlers 两文件的 5 项直接相关回归通过，407 篇 Markdown 的本地链接检查通过。

PR #394 首次 CI（[run 39287465659](https://github.com/ait-app/ait/actions/runs/39287465659)）出现三类遗漏：

- 非英语资源的 `plugin-settings.shared` 误含四个已经位于根层的通用翻译键，导致与英语键集不一致。删除误嵌套副本，保留各语言正确位置的翻译；`vitest run --project unit src/i18n/resources.test.ts` 从 35 通过、1 失败变为 36 项通过。
- 新增两个 Setup 请求后，API／metadata／daemon 的目录数量断言未同步。更新为 207 个发布能力、206 个实现方法、194 个业务目录方法及 47 个 metadata 方法；继续检查唯一性、占位方法错误和安装边界，并显式确认两个新请求由 metadata 声明且出现在生产 daemon 中。
- Antigravity 和 DSH 原生网页登录的旧配额测试仍要求跨读取复用缓存，与 ADR-167 的未知账号身份规则冲突。改为检查非强制读取仍查询原生配额、退出登录立即返回不可用且不携带旧成功值，保留 host／live session 隔离检查。

Rust 定向回归只选择 API capabilities／connection validation／HTTP readiness／placeholders、metadata capabilities、Antigravity／DSH native quota、daemon composite installation 与 production catalog：28 项通过。其余 1582 项由过滤器排除，并非本次执行或验证范围；没有运行完整 Rust workspace 测试。

```sh
cargo nextest run --locked -p api -p metadata -p provider -p daemon -E '
(package(api) & (test(capabilities::tests::) | test(connection::validation::tests::) | test(=tests::http_authentication_origins_and_readiness) | test(tests::placeholders::))) |
(package(metadata) & test(capabilities::tests::)) |
(package(provider) & (test(local::antigravity::tests::quota::) | test(local::deepseek_harness::native::tests::quota::))) |
(package(daemon) & (test(=host::tests::each_composite_service_installs_all_its_methods_without_other_components) | test(=unix::catalog::production_installs_every_in_scope_method_without_placeholders)))'
```

`cargo clippy --locked -p api -p metadata -p provider -p daemon --all-targets -- -D warnings`、Rust／UI 格式与 lint、文档链接和 diff 空白检查通过。客户端与 Rust 方法声明一致性、声明解析及 crate coverage 配置脚本也通过；其中 coverage 配置脚本是静态配置检查，不是覆盖率测量。

## Test coverage

**Not measured。** 用户没有请求覆盖率测量；按仓库规范只运行修改代码及直接相关行为的定向测试，不扩展为完整 Rust workspace 测试或 coverage。当前没有可报告的行覆盖率、覆盖／总行数、可比较基线或覆盖率产物。若后续需要测量，应对 filesystem Git、provider 用量／会话注册、metadata automation 和 model session 的相关测试使用 `cargo llvm-cov`，单独保存可共享报告；全 workspace 测量仍需明确请求。

测试执行数量与通过结果在上一节独立列出，不代表覆盖率。尚未实际验证的主要行为是设备上的原生选择与键盘、Linux 窗口系统以及真实原生账号切换；对应平台验收不能由纯函数测试或 JS 打包替代。
