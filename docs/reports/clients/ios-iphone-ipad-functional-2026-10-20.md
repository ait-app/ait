# iPhone / iPad 原生功能验收

日期：2026-10-20。源码：`974c64f39a7e46bd0139070dc85017a77555df9a`，Ait 0.1.12。
开始测试时通过 `git ls-remote origin refs/heads/main` 确认与远程 main 一致。
初次验收的客户端和独立 Rust daemon 均从这一提交构建。随后按用户要求修复已确认的问题，
修复复测单独记录在文末；本报告记录本地构建与模拟器验收结果。

修复状态：两项已确认问题均已在本工作树修复。标准 Release 构建成功，iPhone 与 iPad
模拟器的中文状态提示复验通过；其余未验收项目仍遵循下文限制。

## 结论

初次验收不能判定“所有功能都没有问题”。主要连接、会话、终端、文件和布局流程通过，
发现一项可复现的原生显示缺陷和一项本地构建兼容性问题。
917 项移动端单元测试、7 项原生配置检查通过，TypeScript 检查通过。
自动化结果与下面的原生操作结果分别记录，不代表真机或全功能覆盖。

## 环境与设备

使用 Xcode 27.0（27A266a）和 Device Hub 操作原生 Release App，应用 ID 为
`dev.ait.mobile`，不依赖 Metro 开发服务器。

后端为当前源码构建的 `daemon 0.1.12`，仅监听 `127.0.0.1:48129`，使用独立数据目录和
临时测试令牌。项目为 ignored `.tmp/ios-functional-20261020/project` 下的临时 Git 仓库，
包含 Markdown、中英文文本、一次未提交修改和测试 PNG。没有修改用户工作项目。

| 设备                 | 系统        | 原生操作范围与结果                                                                                                          |
| -------------------- | ----------- | --------------------------------------------------------------------------------------------------------------------------- |
| iPhone 17 Pro 模拟器 | iOS 26.5    | 启动、直连、工作区、创建终端、软键盘命令、中英文输入、文件、Git、模型选择、Agent 应答、短暂后台恢复通过；Git 状态提示有缺字 |
| iPad mini 模拟器     | iPadOS 26.5 | 启动、直连、跨设备会话读取、终端恢复与命令、文件预览、横竖屏、设置、主题和语言切换通过；Git 状态提示有相同缺字              |
| iPhone 18 Pro 模拟器 | iOS 27.0    | 启动、直连、工作区加载和既有 Agent 会话读取通过；没有重复执行 26.5 的完整功能清单                                           |
| iPad mini 模拟器     | iPadOS 27.0 | 启动、直连、工作区及终端历史恢复通过，首个软键盘字符 `p` 到达；后续按键自动化响应异常，完整命令输入未验收                   |

表中的 iPad mini 使用本机已有的 `Ait main aa79a999 iPad mini 26.5/27.0` 模拟器名称；
名称中的旧提交不代表安装包版本，二者均重新安装了本次构建。

## 真实操作证据

### iPhone 17 Pro / iOS 26.5

- 欢迎页显示 `v0.1.12`，通过 Host、端口和令牌表单连接隔离 daemon，侧栏同步到测试项目。
- 打开工作区并从新标签页创建真实 PTY 终端。软键盘首个 `a` 正常到达，退格可删除；
  通过软键盘逐键输入 `pwd`、点击终端 Enter，实际收到测试项目的完整目录。
- 切换拼音键盘，输入 `nihao`，候选“你好”确认后实际进入终端。
  回车后再输入 `nihao`，下一行首个 `n` 未丢失，仍能确认“你好”。本轮未观察到闪退。
  将“你好”作为 shell 命令产生的 `command not found` 是测试输入的预期 shell 结果。
- 键盘显示和收起、侧栏与文件面板切换正常。
- Git 变化统计从 `+4/-0` 刷新为 `+5/-1`，英文和中文正文 diff、行号和新增/删除底色可见。
- Markdown 预览包含标题、列表、中文和代码块；切换源代码后显示行号和原始内容。
- 标签切换弹窗底部“新建 Agent”“新建 Terminal”完整可见。
- 新建 Agent，打开 provider 和模型列表，选择 Codex / GPT-6-Luna，发送：
  `Reply only with IOS_QA_OK. Do not run tools or modify files.`，实际收到 `IOS_QA_OK`。
- 附件菜单显示添加图片、拍照、粘贴图片、Issue/PR 和 Upload file 入口；未将菜单可见计作附件传输通过。
- 点击模拟器 Home 后观察到主屏幕，再恢复 App，仍可读取同一会话和应答。

### iPad mini / iPadOS 26.5

- 直连后显示 iPhone 创建的工作区、测试消息和 `IOS_QA_OK` 应答。
- 切换终端标签，恢复 iPhone 的输出与中文内容；用软键盘退格清空草稿，再输入 `pwd` 并执行，
  收到正确项目目录。键盘显示和收起正常。
- 从竖屏旋转为横屏，常驻侧栏、横向标签、终端快捷键和右侧文件/Git 面板正常显示，终端按新宽度重排。
- 通过文件列表打开 README，作为文件标签显示 Markdown 预览及中文内容。
- 设置页可导航；深色主题实际生效，随后恢复“系统”。语言切换为 English 后菜单更新，随后恢复“系统”。
- 在线服务设置入口可打开，当前已有账户状态提示需要重新登录；未执行真实账号登录与中继验收。

### iOS / iPadOS 27

- iPhone 18 Pro 的欢迎页、直连及会话内容均已实际读取，未复现启动后停留空白页。
- iPad 完成直连，并实际恢复工作区和终端历史；首字符 `p` 到达真实终端。
- Device Hub / 辅助功能通道多次返回 `AXError.cannotComplete` 或失效元素引用。
  部分输入虽然返回工具错误，但刷新完整辅助功能树后能看到已更新的真实表单值，并最终成功连接。
- iPad 后续终端按键操作无法可靠确认，不能算完整输入通过，也不能仅凭工具错误认定为应用卡死。
  进程仍存在，中央 DiagnosticReports 中未发现晚于本轮测试开始时间的新 Ait 崩溃报告。
  额外进程采样未在等待窗口内完成，不作为诊断证据或通过项。

## 初次验收发现的问题

### 原生 Git 状态行的中文显示为方框

影响：中文界面中，Git diff 的图片等二进制文件状态说明不可读。
已在 iPhone / iOS 26.5 和 iPad / iPadOS 26.5 复现。

复现步骤：

1. 在项目中增加未提交的 PNG 文件。
2. 打开工作区右侧“变更”，展开该图片条目。
3. 文件名与变化统计正常，但本应显示的“二进制文件”呈现为五个方框。

同一面板中 README 的中文增删内容正常，说明问题范围至少可缩小到状态行绘制。
[状态标签构建](../../../apps/mobile/src/git/diff-document/model.ts) 使用翻译文字；
[原生绘制](../../../apps/mobile/src/git/diff-document/paint.native.ts) 的 `row.kind === "status"`
分支直接使用 `drawText` 和基础字体，正文则可以使用已构建的 paragraph。
初次验收将其记录为字体回退的线索，当时尚未修改绘制实现；后续根因验证和修复见文末。

### Xcode 27 下标准模拟器构建失败

复现命令：

```sh
npm run build:ios:simulator
```

第三方资源 targets 的 `IPHONEOS_DEPLOYMENT_TARGET` 低于 Xcode 27 接受的范围：

| Pod target                                | 声明版本 | Xcode 27 报错中的允许范围 |
| ----------------------------------------- | -------- | ------------------------- |
| SDWebImage-SDWebImage                     | 9.0      | 15.0–27.0.x               |
| RNCAsyncStorage-RNCAsyncStorage_resources | 13.4     | 15.0–27.0.x               |
| RNSVG-RNSVGFilters                        | 12.4     | 15.0–27.0.x               |

本次在标准脚本已经完成 prebuild / Pods 后，按项目现有最低版本增加构建参数，成功生成实际验收使用的包：

```sh
CI=1 EXPO_NO_TELEMETRY=1 APP_VARIANT=production \
  EXTRA_PACKAGER_ARGS='--max-workers 2' \
  xcodebuild -hideShellScriptEnvironment \
  -workspace apps/mobile/ios/Ait.xcworkspace -scheme Ait \
  -configuration Release -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath apps/mobile/release/ios/simulator/DerivedData -jobs 4 \
  build ARCHS=arm64 ONLY_ACTIVE_ARCH=YES CODE_SIGNING_ALLOWED=NO \
  IPHONEOS_DEPLOYMENT_TARGET=16.0
```

该命令返回 `BUILD SUCCEEDED`。这是初次验收使用的临时绕行，当时
[标准构建脚本](../../../apps/mobile/scripts/build-ios.mjs) 尚未修改；后续标准命令复测见文末。

## 自动化验证

| 检查                                                                                            | 结果                               |
| ----------------------------------------------------------------------------------------------- | ---------------------------------- |
| `npm run typecheck --workspace=@ait/mobile`                                                     | 退出码 0                           |
| 终端原生渲染/输入、终端按键与虚拟键盘、弹窗布局、原生文本输入、账户连接与下载相关 unit          | 29 文件、274 项通过                |
| composer、file-pane、navigation、新工作区、settings、工作区布局、Combobox 与原生 diff 相关 unit | 68 文件、643 项通过                |
| Scene 生命周期、应用标识的 Node 检查                                                            | 7 项通过                           |
| `cargo build -j 1 -p daemon --bin daemon`                                                       | 成功，`daemon --version` 为 0.1.12 |
| 真实后端 `/v1/server/info`，缺少 / 错误 / 有效令牌                                              | 分别为 HTTP 401 / 401 / 200        |
| 真实 SDK 打开临时项目、查询 provider                                                            | 成功                               |

移动端回归范围的最终命令：

```sh
npm run test --workspace=@ait/mobile -- --project unit \
  src/terminal/native-renderer \
  src/terminal/runtime/terminal-key-dispatch.test.ts \
  src/terminal/runtime/terminal-virtual-keyboard.test.ts \
  src/components/adaptive-modal-sheet-layout.test.ts \
  src/components/ui/combobox-options.test.ts \
  src/components/ui/text-input/text-input.native.test.tsx \
  src/runtime/native-account.native.test.ts \
  src/runtime/account-state.native.test.ts \
  src/runtime/account-browser-login.native.test.ts \
  src/runtime/rust-daemon/native-account-transport.test.ts \
  src/runtime/rust-daemon/native-account-download.test.ts \
  src/runtime/rust-daemon/account-transport.native.test.ts \
  src/stores/download-store.native.test.ts

npm run test --workspace=@ait/mobile -- --project unit --maxWorkers 1 \
  src/composer src/file-pane src/navigation src/screens/new-workspace \
  src/screens/settings src/stores/workspace-layout-store.test.ts \
  src/stores/workspace-layout-persistence.test.ts \
  src/stores/workspace-layout-storage.test.ts \
  src/components/ui/combobox-frame-style.test.ts \
  src/components/ui/combobox-keyboard.test.ts \
  src/git/diff-document/native-hit-testing.test.ts \
  src/git/diff-document/native-slabs.test.ts \
  src/git/diff-document/native-text-runs.test.ts \
  src/git/diff-document/paint.native.test.ts

node --test apps/mobile/plugins/with-ios-scene-lifecycle.test.cjs \
  apps/mobile/scripts/app-identity.test.cjs
```

只统计最终完成的测试运行。未修改 Rust 代码，按仓库规则没有运行 Rust 测试或 workspace 覆盖率。

报告与索引的 Oxfmt 检查通过。全量 `npm run check:docs` 的脚本测试通过，但链接扫描进入了
本次生成的 ignored `apps/mobile/ios/Pods/libwebp/README.md`，因该第三方目录不含
`doc/building.md`、`doc/tools.md`、`doc/api.md` 和 `CONTRIBUTING.md` 而失败。
因此另行对 Git 跟踪的 Markdown 及本次新报告执行同一链接检查函数，413 份仓库文档全部通过。

## 限制和后续验收

- 本次全部为模拟器；未连接实体 iPhone 或 iPad，未验证 TestFlight 分发包。
- 实体相机、麦克风、语音模式、听写和 APNs 推送未验收。
- 真实账号登录、中继、远程下载及系统分享、附件上传完整链路未验收。
- 未执行 Git commit/push/merge、真实 PR 操作或定时任务执行；未将入口可见计为功能通过。
- 仅验证一次 Codex Agent 应答；未逐一验收其他 provider、工具执行、审批、会话分叉和中断。
- 未做持续后台超过两分钟、断网重连、硬件键盘、多窗口、性能及长时间稳定性测试。
- iPadOS 27 完整终端输入仍需复验，当前证据不足以宣布全系统全功能通过。

构建和测试日志保留在本机 `/private/tmp/ait-ios-functional-*-20261020.log`。
临时项目、数据库、凭据和构建产物均未纳入版本控制。

## 修复复测（2026-10-20）

### 原生状态文字

根因已由失败回归及真实界面对照确认：状态行直接使用单一字体的 `drawText`，缺少中文字形时
无法回退到系统字体；正文已经使用支持字体回退的 Skia Paragraph，因此同一面板的中文正文正常。

- 状态行改为复用既有 Paragraph 排版路径，覆盖二进制文件和文件过大两类状态。
- 相同状态文字在可视区域内复用缓存；移出准备窗口或释放布局时同步释放，避免滚动时重复排版或积累原生资源。
- 增加 `status-text.native.test.ts`，从真实模型、布局准备到绘制入口验证文字输出。
  测试模拟主字体缺少非 ASCII 字形，包含中文、日文、阿拉伯文、英文及缓存释放行为。
  修复前为 6 项失败、1 项通过，中文实际输出为 `□□□□□`；修复后 7 项全部通过。

### 标准构建

本地构建脚本读取 Expo prebuild 生成的 `Podfile.properties.json` 中的 `ios.deploymentTarget`，
将其作为 Xcode 构建设置应用于模拟器与真机归档路径中的 targets。
不再需要手工补充最低版本参数，也没有另外硬编码一个与 Expo 配置分离的版本值。

实际执行 `npm run build:ios:simulator`，日志确认 `IPHONEOS_DEPLOYMENT_TARGET = 16.0`，
最终返回 `BUILD SUCCEEDED` 与退出码 0。

### 验证结果

| 检查                                                                           | 结果                                                               |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `npm run test --workspace=@ait/mobile -- --project unit src/git/diff-document` | 16 文件、110 项通过，包含 7 项新增回归                             |
| 移动端 TypeScript 检查                                                         | 通过                                                               |
| 修改源码 ESLint `--max-warnings 0`、Oxfmt、构建脚本 `node --check`             | 通过                                                               |
| 标准 Release 模拟器构建                                                        | 通过，无需手工构建参数                                             |
| iPhone 17 Pro / iOS 26.5 安装本次构建                                          | 同一 `qa-image.png` 条目正常显示“二进制文件”，中文正文 diff 仍正常 |
| iPad mini / iPadOS 26.5 安装同一构建                                           | 竖屏及旋转后的横屏均正常显示“二进制文件”                           |

修复复测日志为本机 `/private/tmp/ait-ios-fix-*-20261020.log`。
本轮没有重新执行完整移动端测试清单或 Rust 测试，也未补齐上文列出的真机、账号中继、
推送及 iPadOS 27 完整终端输入验收，因此不将这两项修复扩大为全功能通过的结论。

### 再次复测

按用户要求，在同一修复工作树再次运行相关检查：

- Git diff 相关单元测试：16 个文件、110 项全部通过。
- iOS 场景生命周期与应用标识检查：7 项全部通过，合计 117 项测试通过。
- 移动端 TypeScript、修改源码 ESLint `--max-warnings 0`、构建脚本语法及 Oxfmt 检查通过。
- 再次执行标准 `npm run build:ios:simulator`，确认最低版本为 16.0，返回
  `BUILD SUCCEEDED` 与退出码 0。日志仍含第三方编译和打包警告，没有构建失败。

本次重跑的是上述自动化检查与标准构建。iPhone、iPad 原生界面操作的验证记录仍为前一节，
本次未重复整套界面操作，也未扩大真机和系统版本的验收范围。
日志保留在本机 `/private/tmp/ait-ios-retest-*-20261020.log`。
