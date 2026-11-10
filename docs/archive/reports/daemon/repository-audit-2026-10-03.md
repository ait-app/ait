# 仓库审计与修复：2026-10-03

审计起点：`6cfd8a1f`；提交准备时已更新到 `3c2cb5d4cfb9661ae7475fbd40eb1f23e281a4c4`
并保留全部审计修复。平台为 macOS arm64，Rust 1.98.1。
审计以入口、权限、状态流转、并发、文件操作和失败恢复为重点；
覆盖各主要组件的关键路径，不代表逐行证明或第三方依赖漏洞认证。

## 范围

| 组件                                                | 检查内容                                                                               |
| --------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `bins/daemon`、`api`、`protocol`                    | 启动配置、实例锁、Host/Origin/令牌校验、浏览器一次性凭据、下载入口、协议限制           |
| `domain`、`model`                                   | 状态模型、任务准入/取消、订阅与发送队列、目录 generation/sequence 与删除标记           |
| `metadata`                                          | registry 原子写入、观察者失败语义、标签事务日志和失败恢复、目录投影                    |
| `filesystem`                                        | 路径与符号链接约束、写入/重命名、上传限制、文件搜索、Git diff/pathspec、Git 子进程约束 |
| `provider`                                          | 原生进程与消息限制、运行结束与持久化、时间线存储入口、工作区提示的并发读写             |
| `schedule`                                          | 定时状态机、运行中修改配置、下一次执行时间、服务取消和提交                             |
| `terminal`、`browser`                               | PTY 启停/回收、输入队列与 resize 所有权、终端订阅、浏览器请求所有权和数量限制          |
| `voice`                                             | PCM/WAV 校验、离线模型下载/解包限制及状态                                              |
| `packages/client`、`packages/protocol`              | 连接重试、连接所有权、消息与协议边界                                                   |
| `apps/desktop`                                      | 主窗口/preload 导航边界、Workspace webview、外部 opener、更新状态与版本                |
| `apps/mobile`                                       | Rust transport、连接与 workspace 缓存/增量投影；共享 UI 类型检查                       |
| `packages/highlight`、`packages/expo-two-way-audio` | 高亮映射、音频 JS wrapper、Android 音频采样与销毁、iOS 播放/会话恢复的选定路径         |
| 构建与发布                                          | workspace 本地包解析、版本一致性、CI 入口、发布资产校验和 TestFlight 版本/重复构建判断 |

## 已修复的问题

| 问题                                   | 触发与影响                                                                | 修复与证据                                                                                                                                                         |
| -------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 主窗口可导航至外部页面                 | 仅阻止 `file://`，外部页面可进入带桌面 preload 的窗口                     | 固定 renderer 来源，拦截导航和重定向，拒绝主窗口 popup；打包/开发两种来源均有回归，见 [ADR-073](../../../decisions/clients/adr-073-desktop-renderer-navigation.md) |
| 批量清提示使用过期快照                 | 扫描后到达的新权限/完成提示，或已移动的 Agent，可能被旧操作清除           | 在 registry 更新锁内比较当前记录与快照；回归覆盖新权限、新完成事件和 workspace 迁移                                                                                |
| 标为未读使用过期候选记录               | 候选在选择后移动或更新，仍可能被修改，更新时间也可能倒退                  | 更新时重新比较完整候选，冲突返回现有错误；回归覆盖更新时间变化和 workspace 迁移                                                                                    |
| SDK 首次同步连接失败后无法重试         | transport factory 同步抛错时，失败清理被 Promise 字段赋值覆盖             | 先发布待完成 Promise，再启动连接；回归验证第二次连接真正创建 transport 并成功                                                                                      |
| Commit diff 把文件名当作 Git pathspec  | 文件名含 `:(literal)` 等语法时查不到自己的 diff                           | `git show` 使用 `--literal-pathspecs`；真实临时 Git 仓库回归                                                                                                       |
| 重命名把未跟踪文件误判为已跟踪         | 未跟踪的 `[a].txt` 匹配已跟踪的 `a.txt`，随后 `git mv` 失败               | `git ls-files` 使用 literal pathspec；回归确认重命名成功且邻近文件/索引保留                                                                                        |
| 文本内容含 `Binary files` 时 diff 消失 | 普通文档中的文字触发整段输出的二进制判断                                  | 删除子串判断，由 diff 解析器识别实际二进制 header；文本回归通过                                                                                                    |
| 内部目录符号链接显示成文件             | 列表使用链接本身的类型，无法浏览目标目录                                  | 使用经过 scope 校验的目标 metadata；回归同时确保指向 workspace 外的链接仍被排除                                                                                    |
| 运行中修改 cadence 后跳过下一次执行    | 当前 run 完成时把配置更新产生的未来 slot 再推进一次                       | 保留已排定的未来 anchor；回归验证 2:10 的下一次执行不再跳到 4:10                                                                                                   |
| Android 音量错误/空输入 NaN            | PCM16 低字节符号扩展覆盖高字节；空输入除零                                | 无符号低字节组装、空样本返回零；提取无 Android 依赖的计算函数，4 项 JVM 回归覆盖极值、正负对称、静音及混合 RMS                                                     |
| Android 音频资源回收不完整             | 销毁遗漏设备监听器、播放线程池、AudioTrack 和音效资源；跨线程共享普通队列 | 注销监听器、关闭两个 worker、释放原生资源，销毁幂等且拒绝新录音/播放；录音停止释放音效；队列改用并发容器。Android API 编译验证，设备生命周期验证尚未执行           |

除 Android 原生生命周期外，上述修复均以修复前失败、修复后通过的回归确认。
Android 音量测试先使用原有计算表达式复现失败，再验证修正后的计算。

## 测试与静态检查

审计阶段遵循仓库要求只运行变更及直接相关测试，结果如下；提交前全量验证另列于后。
Rust 命令使用 `CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-pr-target`、默认 features，
依赖锁定且离线。

| 检查                 | 命令/范围                                                                                                                                                                         | 结果                                                                          |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| 文件与 checkout      | `cargo test --locked --offline -p filesystem --lib -- local::checkout::tests local::files::tests`                                                                                 | 90 passed，281 filtered                                                       |
| Agent attention      | `cargo test --locked --offline -p provider service::workspace_attention::tests -- --nocapture`                                                                                    | 17 passed，486 filtered                                                       |
| Schedule 状态机      | `cargo test --locked --offline -p schedule engine::tests -- --nocapture`                                                                                                          | 35 passed，35 filtered                                                        |
| SDK 连接             | `npm exec --workspace=@ait/client -- vitest run src/daemon-client.test.ts src/connection/owned.test.ts`                                                                           | 143 passed，2 files                                                           |
| Desktop 导航/webview | `npm exec --workspace=@ait/desktop -- vitest run src/window/window-manager.test.ts src/features/browser-webviews/index.test.ts src/features/browser-webviews/window-open.test.ts` | 41 passed，3 files                                                            |
| Android PCM 音量     | Kotlin 2.2.10 JVM 编译 `AudioVolume.kt` 与 `AudioVolumeTest.kt`；JUnit 4.13.2 `org.junit.runner.JUnitCore expo.modules.twowayaudio.AudioVolumeTest`                               | 4 passed；修复前 3 项失败                                                     |
| Rust 编译            | `cargo build --workspace --locked --offline`                                                                                                                                      | passed，无编译警告                                                            |
| Rust lint            | `cargo clippy --locked --offline -p filesystem -p provider -p schedule --all-targets -- -D warnings`                                                                              | passed                                                                        |
| Rust 格式            | `cargo fmt --all --check`                                                                                                                                                         | passed                                                                        |
| TS 类型              | `npm run typecheck --workspace=@ait/client --workspace=@ait/desktop --workspace=@ait/mobile`                                                                                      | passed                                                                        |
| TS 格式/lint         | 对 5 个改动 TS 文件运行 `node_modules/.bin/oxfmt` 和 `node_modules/.bin/oxlint`                                                                                                   | passed，0 lint warnings/errors                                                |
| Android 编译         | `K2JVMCompiler -Werror -no-stdlib -no-reflect`，Android 36 与 AndroidX annotation 1.9.1 classpath，编译 `AudioEngine.kt` 与 `AudioVolume.kt`                                      | passed；本地 JDK 提示 Kotlin 编译器内部使用已弃用 JVM API，无 Kotlin 源码警告 |
| 本地包/版本          | `npm run verify:local-packages`、`npm run verify:release`                                                                                                                         | 6 个本地私有包解析正确；v0.0.14 版本一致                                      |
| 文档/差异            | `npm run check:docs`、`git diff --check`                                                                                                                                          | passed；更新 main 后 170 份 Markdown 的本地链接有效                           |

以上为 330 项行为回归，另有 1 项文档检查器测试。Android JVM 验证通过临时运行器
`python3 /private/tmp/ait-audio-volume-test.py` 调用本机已缓存的 Kotlin/JUnit JAR，
未修改 Android 应用或用户数据。测试源码及 Gradle `testImplementation` 保留在包内，
可由生成的 Expo Android 工程继续运行；未将临时编译产物纳入仓库。

### 提交前全量验证

用户要求创建 PR 后，在上述最新 main 基线及本 PR 改动上执行：

| 命令                                                                                                                                     | 结果                                                                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-pr-target cargo test --workspace --locked --offline`                                | 1,572 passed，0 failed，3 ignored；包含 daemon 进程集成测试                            |
| `CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-pr-target cargo clippy --workspace --locked --offline --all-targets -- -D warnings` | passed                                                                                 |
| `CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-pr-target cargo build --workspace --locked --offline`                               | passed                                                                                 |
| `npm exec --workspace=@ait/client -- vitest run`                                                                                         | 217 passed，8 files                                                                    |
| `npm exec --workspace=@ait/desktop -- vitest run --exclude 'e2e/**'`                                                                     | 375 passed，13 skipped，53 files；跳过未配置 daemon binary 的本机集成与 Linux 专用用例 |
| 三个应用/SDK 的 typecheck、Android PCM JVM 测试、格式/lint、本地包/版本及文档检查                                                        | passed                                                                                 |

首次在沙箱内执行时，本机监听被拒绝（`EPERM`），导致 API/桌面端口测试失败。
上述成功结果来自允许本机端口的重跑；未为通过测试修改应用行为或跳过失败用例。

## Test coverage

本次提交准备已测量：**Rust workspace 行覆盖率 92.18%（43,974 / 47,707）**。
覆盖率运行另执行 1,572 项测试，全部通过，3 项真实 Provider 测试保持 ignored。

| 范围       | 已覆盖 / 总行数 | 行覆盖率 |
| ---------- | --------------- | -------- |
| workspace  | 43,974 / 47,707 | 92.18%   |
| filesystem | 10,570 / 11,710 | 90.26%   |
| provider   | 18,334 / 19,625 | 93.42%   |
| schedule   | 805 / 823       | 97.81%   |

命令：

```sh
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov --workspace --locked --offline --html
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-audit-pr-coverage-summary.json
```

范围：上述 `3c2cb5d4` 基线加本 PR Rust 改动，默认 features，macOS arm64，
Rust 1.98.1、cargo-llvm-cov 0.8.4；只使用工具默认过滤，无额外文件排除，
不含 TypeScript/Kotlin 或 doctest 插桩。精确 Rust 内容指纹、变更源码 SHA256、
各 crate/变更模块行数与 ignored 原因保存在
[可审查的覆盖率产物](repository-audit-2026-10-03-coverage.json)。

同平台、工具链和 workspace 范围的
[前次记录](../workspace/large-diff-loading-coverage.json)为 92.16093%（43,958 / 47,697），
本次增加 0.01422 个百分点。该比较使用已有记录，未重新运行旧基线。

HTML 已生成在 `/private/tmp/ait-workspace-sidebar-cov-target/llvm-cov/html/index.html`，
该路径只是本地详细报告，共享证据以上述入库 JSON 为准。未覆盖的重要行为包括真实
Provider 认证调用、其他操作系统、打包版 Electron 导航和 Android 音频设备生命周期，
需对应平台和设备验证，见下节。

## 验证限制

- 未执行真实 Provider 付费调用、发布、自动更新安装、用户数据迁移或真机端到端操作。
- Android 音量为真实 Kotlin/JVM 测试；音频资源清理仅做源码检查与 Android API 编译，
  仍需在设备上反复 initialize/record/stop/tearDown，观察线程、音效和蓝牙路由释放。
- 未进行 iOS、Linux、Windows 的原生运行验证；Unix 文件名/符号链接用例的可用平台由测试 cfg 限定。
- 未进行第三方依赖 CVE 数据库扫描，生成代码、品牌资产和原生音频所有设备组合不在逐项证明范围内。
