# Paseo desktop/app 选定改动移植

日期：2026-10-10。AIT 基线：`afa2eb500dfb379d2ac34064f2a4df3a1de63f8f`，测试源码树：`d4b530d8ccf4bfcfed4e797fe3c90eec1c3cde77`，后续只更新报告和覆盖率证据；平台为 macOS arm64，浏览器测试使用 Chromium。

## 范围与来源

用户选择之前清单中的全部推荐项与产品增强：1、2、3、4、6、7、16、19、20、21、22、23、24、26、31、34、35，共 17 项。上游固定为 Paseo 0.11.2 的 [a1da1b8f234afe7a75785022292cdf8a50ee11a5](https://github.com/getpaseo/paseo/commit/a1da1b8f234afe7a75785022292cdf8a50ee11a5)，提交时间 2026-10-09 15:43:26 UTC。来源目录映射为 `packages/desktop` → `apps/desktop`、`packages/app` → `apps/mobile`。

| 编号 | 已接入行为                                                                                                              | 上游 PR                                                                     |
| ---- | ----------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| 1    | 历史分页和虚拟列表改变时保留阅读锚点，流式与历史消息使用稳定 Markdown block 身份，图片预留尺寸                          | #5945、#5970、#6172；依赖 #5146 的消息身份整理                              |
| 2    | 代码选区、跨代码块列表编号、图片 alt/source 与图片下方选区的复制修复                                                    | #6138、#6158、#6181、#6195                                                  |
| 3    | 外观设置中的最大内容宽度，统一聊天、输入框和 Markdown 文件预览                                                          | #5680                                                                       |
| 4    | Explorer 的 Files/Changes 复用 Workspace 标签行，统一拖拽、关闭和新标签菜单                                             | #5942                                                                       |
| 6    | 文件链接缓存包含目标行号，Windows 工作区路径提示使用相对路径                                                            | #5653、#1987                                                                |
| 7    | 从父会话打开子 Agent 时沿用父会话所在分栏                                                                               | #5451                                                                       |
| 16   | Android Back 只关闭最上层 Sheet，叠层弹窗的背景、关闭顺序和菜单恢复                                                     | #5245、#5772、#5805 的弹层部分                                              |
| 19   | macOS Cmd/Option 行编辑序列，OSC 8 外部链接，cursor-agent 终端配置图标                                                  | #3339 系列、#5388、#5379                                                    |
| 20   | 长聊天滚动后 Workspace 顶部仍可拖动窗口                                                                                 | #5233                                                                       |
| 21   | browser_type 将文本发送至目标 WebContents                                                                               | #6207                                                                       |
| 22   | 应用缩放后按正确坐标捕获标注区域                                                                                        | #5703                                                                       |
| 23   | 增量目录追赶保留会话和缓存 Workspace，重连续订标签，新会话交接保持到实际切换                                            | #5189、#5394、#5079、#5168                                                  |
| 24   | Replica store 写失败时终止 flush/read 忙循环                                                                            | #5290                                                                       |
| 26   | 重连后重新查询已跟踪父会话的 Provider 子 Agent 状态                                                                     | #6316                                                                       |
| 31   | 用量入口、桌面 Modal/移动 Sheet、固定额度窗口、已用/剩余百分比、主机选择、单卡/全部刷新、会话实际账号与安全登录恢复提示 | #5465、#5685、#5786、#5805、#5844、#5876、#5903、#5921、#5975、#6156、#6216 |
| 34   | Vue 语法高亮，Astro 嵌套模板字符串之后的表达式高亮                                                                      | #6205、#6219                                                                |
| 35   | 按 Codex 原生模型目录提供 Normal/Fast/Ultrafast 及未来档位，桌面闪电菜单与快速档位黄色状态                              | #5708、#6152                                                                |

全部 PR 对应 `https://github.com/getpaseo/paseo/pull/<编号>`。消息身份整理只作为滚动修复的必要依赖，保留 AIT 的流式空白、语音消息投影和聊天搜索计数。目录同步保留 AIT 的 `workspace.update` 事件名与本地缓存实现。未恢复已经移除的 Plugin 功能。

用量能力与速度档位由 Rust 原生适配，边界见 [ADR-119](../../decisions/providers/adr-119-native-account-usage-and-codex-speed.md)。Lucide 更新到 1.50.0，以使用标签控件的图标 Provider。已合入最新 main 的 OpenCode 原生权限/ACP、iOS 中文输入、终端配色和 crate 可见性重构；保留主分支的 0.0.24 版本。创建 PR 期间 main 新增摘要契约归 model、配置适配归 persistence 与目录读取预算隔离，已再次整合；界面源码与首次验证版本一致。

## 验证

在最新 main 上复查本次改动及直接相关行为，并执行提交准备所需的完整 Rust workspace 验证。资源导出是 JavaScript/资源构建，不能代表 Android APK、iOS IPA 或 Electron 安装包的实机验收。

| 检查                                                                              | 结果                                            |
| --------------------------------------------------------------------------------- | ----------------------------------------------- |
| 移动端相关单元测试及 main 整合回归                                                | 39 个文件，652 项通过                           |
| Chromium：复制选区、终端键盘/链接/配色、聊天查找范围/视口、用量卡片、overlay 焦点 | 6 个文件，89 项通过                             |
| Desktop：浏览器工具 service/IPC 与截图坐标                                        | 3 个文件，77 项通过                             |
| Highlight：Vue/Astro 与解析器                                                     | 2 个文件，32 项通过                             |
| Protocol：用量 DTO、select 图标、终端配置                                         | 3 个文件，79 项通过                             |
| Client SDK：会话作用域/单 Provider/强制刷新请求                                   | 1 项通过，其余 142 项未选中                     |
| TypeScript：mobile、desktop、client                                               | 类型检查通过                                    |
| 构建：共享 UI 依赖、Electron 主进程与 desktop renderer                            | 通过                                            |
| iOS / Android Hermes 资源导出                                                     | 通过，均为 40.2 MB                              |
| Rust workspace 构建、Clippy 与格式                                                | 通过，Clippy 使用 `-D warnings`                 |
| Rust 完整 workspace 测试                                                          | 2009 项通过、0 失败、14 项原生安装/认证测试忽略 |
| Rust 完整 workspace 覆盖率运行                                                    | 2009 项通过、0 失败、14 项忽略；HTML 已生成     |
| TypeScript 格式、Oxlint、文档链接、git diff 空白检查                              | 通过；Oxlint 零警告、零错误                     |

移动端主检查的完整文件清单：

```sh
npm exec --workspace=@ait/mobile -- vitest run --project unit \
  src/agent-stream/chat-find/model.test.ts \
  src/agent-stream/strategy-web.test.tsx \
  src/agent-stream/web-virtualization.test.ts \
  src/appearance/apply.test.ts \
  src/components/markdown/renderer.test.ts \
  src/components/ui/isolated-bottom-sheet-modal/visibility-tracker.test.ts \
  src/composer/agent-controls/utils.test.ts \
  src/hooks/use-settings/storage.test.ts \
  src/panels/panel-manifest.test.ts \
  src/runtime/directory-sync/agent-replica.test.ts \
  src/runtime/directory-sync/index.test.ts \
  src/runtime/replica-cache/index.test.ts \
  src/screens/workspace/workspace-tab-layout.test.ts \
  src/stores/workspace-layout-store.test.ts \
  src/subagents/provider-store.test.ts \
  src/terminal/runtime/terminal-emulator-runtime.test.ts \
  src/timeline/replica.test.ts \
  src/timeline/viewed-timeline-sync.test.ts \
  src/utils/assistant-image-metadata.test.ts \
  src/utils/assistant-message-height-estimate.test.ts \
  src/utils/terminal-keys.test.ts \
  src/workspace-tabs/launcher/internal/catalog.test.ts \
  src/workspace/file-open/index.test.ts \
  src/agent-stream/reading-anchor.test.ts \
  src/assistant-file-links/tooltip-path.test.ts \
  src/usage/copy.test.ts \
  src/usage/format.test.ts \
  src/usage/model.test.ts \
  src/usage/pinned.test.ts \
  src/usage/preferences.test.ts \
  src/agent-stream/presentation.test.ts \
  src/sidebar-nav/model.test.ts \
  src/usage/native-report.test.ts \
  src/lib/overlay-root.test.tsx \
  src/components/ui/pane-content-toolbar.test.ts \
  src/agent-controls/icons.test.ts \
  src/composer/draft/create-flow.test.ts \
  src/terminal/native-renderer/terminal-input.ios.test.tsx \
  src/terminal/native-renderer/terminal-input-platform.test.ts
```

其余可复现命令（所有 Rust 检查使用默认 features；没有额外排除 Rust 源文件）：

```sh
npm exec --workspace=@ait/mobile -- vitest run --project browser src/assistant-selection-copy/content.browser.test.ts src/terminal/runtime/terminal-emulator-runtime.browser.test.ts src/agent-stream/chat-find/ranges.browser.test.ts src/agent-stream/chat-find/viewport.browser.test.ts src/lib/overlay-root.browser.test.tsx src/usage/card.browser.test.tsx
npm exec --workspace=@ait/desktop -- vitest run src/features/browser-automation/service.test.ts src/features/browser-automation/ipc.test.ts src/features/browser-capture.test.ts
npm exec --workspace=@ait/highlight -- vitest run src/__tests__/highlighter.test.ts src/__tests__/parsers.test.ts
npm exec --workspace=@ait/protocol -- vitest run src/usage-reports.test.ts src/agent-feature-schemas.test.ts src/terminal-profiles.test.ts
npm exec --workspace=@ait/client -- vitest run src/daemon-client.test.ts -t sends.provider.usage
npm run typecheck --workspace=@ait/mobile --workspace=@ait/desktop --workspace=@ait/client
npm run build:ui-deps
npm run build:main --workspace=@ait/desktop
npm run build:desktop-assets
npm exec --workspace=@ait/mobile -- expo export --platform ios --platform android --max-workers 2 --output-dir /tmp/ait-pr-native-export-final
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo test --workspace -- --test-threads=4
cargo llvm-cov --workspace --html -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-pr-coverage-raw.json
cargo llvm-cov report --lcov --output-path /tmp/ait-pr-coverage.lcov
npm run check:docs
git diff --cached --check
```

对本次新增或改动且仍存在的 195 个 `.ts`/`.tsx` 文件执行格式与 Oxlint 检查。主分支新增的 iOS 输入桥接样式类型在整合时修正为与共享 `TerminalInputProps` 一致，保持原生输入行为。完整回归还覆盖默认 Codex 会话冷启动加载速度目录、旧 `fast_mode` 持久化兼容，以及未安装 Claude 时返回安全的结构化用量不可用状态。测试中的 Electron IPC 使用 WebContents/IPC 夹具；未启动真实 Electron。React 检查覆盖卡片和弹窗的 Hook 顺序、查询作用域、延迟挂载、按钮/checkbox 标签和悬浮层焦点。浏览器首次依赖预构建触发 Vite reload，预构建完成后同一批检查全部通过。

## Test coverage

实际测量为 macOS arm64 上的 **Rust 完整 13-crate workspace**，默认 features，无额外文件排除，未启用 doctest instrumentation；JavaScript 行覆盖率未测量，界面测试结果单独列于上表。

| 范围      | 当前 covered/total | 当前行覆盖率 | 可比基线 | 变化（百分点） |
| --------- | ------------------ | ------------ | -------- | -------------- |
| Workspace | 56168/59355        | 94.6306%     | 94.7087% | -0.0780        |
| Provider  | 26735/28408        | 94.1108%     | 94.2613% | -0.1505        |

命令：`cargo llvm-cov --workspace --html -- --test-threads=1`。测量对应文首的源码树及 AIT 基线，完整 Rust 源码指纹为 `1b73668657ae5b6a6e2b8277058f0b18cd9c03103b073362a2a85f8aff98cf67`；后续只更新文档与证据。基线使用 [摘要契约与目录准入覆盖率证据](../daemon/summary-contracts-directory-admission-coverage-2026-10-10.json)：其 Rust 源码指纹与最新 main 完全一致，平台、features、覆盖率串行设置及 14 项原生 ignore 范围相同，可以比较。

共享制品：[逐 crate / 文件行覆盖率、源码 SHA-256、改动文件未覆盖行与忽略测试清单](paseo-desktop-app-pr-coverage-2026-10-10.json)。HTML 已生成在 `target/llvm-cov/html/index.html`；运行产物不纳入源码提交。普通测试（`--test-threads=4`）与 instrumented 测试（`--test-threads=1`）均为 2009 通过、0 失败、14 忽略，通过数量与覆盖率分开记录。

重要未覆盖行为：Claude 的真实 HTTPS OAuth 请求及 `native_usage` 内 401/403 投影分支未执行；过期凭据/API-key 路径、HTTP 错误体脱敏和 quota 投影分别由离线测试覆盖，后续应补可注入响应的集成测试并验收已登录账号。Linux/Windows 专用路径未在本机测量。主分支的摘要配置适配器已接入编译并纳入本次完整测量。Codex 速度投影模块为 100%（112/112 行），主机用量缓存模块为 95.8763%（93/97 行）。

仍需真实 Android 返回键、iOS 叠层 Sheet、Electron 窗口拖动及真实已登录 CLI/订阅账号的产品验收；离线夹具、Chromium 和资源导出不能代表这些实机行为。
