# PR #612：设置页、Vibe 与 BTW 界面验收

初次界面修订基于 `3820d938`，BTW 生命周期与 Follow up / Branch 补充基于 `c4b83e2e`。此报告对应 `ui-*.png`，
替代此前设置页和 Vibe 展开/退出面板的界面取证；旧 Tai 报告保留其模型调用与 worker 证据。
PR 保持未合并。初次界面修订没有 Rust 变更；本次 BTW Branch 增加 Rust 适配器与公开扩展，验证分别记录。

## 当前交互

- 模型齿轮打开 Provider 设置并收起模型弹层，保留主机、Provider、目录和当前配置。
  首页使用同一行式控件展示当前预设、模型角色、账号入口；预设管理和模型目录分别进入下一页。
  搜索仅出现在模型目录；刷新在标题栏，诊断是次级入口。
- 移除常驻的当前作用域解释、存储规则段落、模糊“详情”入口、更新时间页脚和模型 ID 数组。
  项目目录、条目来源、只读原因和操作错误继续显示。预设空态只有一个创建按钮。
- 角色表移除造成文字起点错位的装饰图标，表头与下拉框文字按同一列内边距对齐。
  BTW 滚动容器为焦点轮廓预留空间，保持完整焦点提示，不通过去掉焦点框掩盖裁切。
- 预设编辑保留角色、模型、思考下拉表单，移除表旁整段实现说明。OMP 名称采用有效示例
  `TeamDefault`，不合法输入直接提示并禁止保存；输入上限与原生 128 字符限制一致。
- `Vibe ON`（ON 加粗）使用现有 composer 胶囊样式，只表示原生确认的启用状态，不是按钮。
  没有展开、重复标题或退出按钮。通过 `/vibe` 切换，`/vibe <任务>` 发起任务；空闲不假装关闭。
- UI 统一使用大写 `BTW`，原生命令仍为 `/btw`。BTW 和 Goal 使用同款可操作胶囊。
  BTW 不再有内外两层折叠；胶囊收起/展开保留未发送草稿。结束行为以后文的按需生命周期为准。
  新的原生 reveal 请求会展开 BTW；普通重绘不会反复强迫展开。

## 运行环境与实际流程

Linux Chromium，桌面 1440×1000 与移动浏览器 390×844。使用隔离数据目录的真实 daemon，
用户安装的官方外部 `/usr/bin/omp` 18.17.0，项目目录 `/tmp/pr612-ux-live/project`。
初次设置与空历史界面的模型目录为预览元数据，地址指向不可用回环端口，没有模型请求。
本次 Follow up / Branch 流程改用本地模拟模型 API，画面明确显示“本地协议验收回复”；不声称 Tai 或上游推理。
鉴权 token、配置、原生日志、数据库和浏览器存储均在仓库外，不提交。

实际操作并核对：

1. 从模型选择器齿轮进入设置，底层选择器关闭，主机与项目上下文保留。
2. GUI 创建全局预设 `Daily`，选择模型并保存；通过真实原生列表读回来源和角色值。
   中文名称曾被后端拒绝，本次修正输入示例并补前端即时校验；没有把失败保存算作成功。
3. 在设置首页选择 `Daily`，确认当前值同步；重新打开编辑器，模型和 Low/High 角色值回填。
4. 进入预设管理、打开删除确认后取消、返回首页、进入模型目录、搜索和返回。
   模型名称不附带序列化数组 ID。刷新后分别反馈模型和预设结果。
5. 向同一外部 OMP 会话发送裸 `/vibe`，确认原生 enabled 回执后出现 `Vibe ON`，没有退出或展开按钮。
   本次没有启动 worker，不将该图当作模型推理或任务完成证据。
6. 初次 UI 修订通过原生 `get_btw_history` 验证 BTW 面板；本次补充的用户输入 `/btw` 与结束流程见下文。
   两个视口均完成此流程，没有发送 BTW 问题或伪造回答。

## 定向验证

- 前端 unit：14 suites / 150 项通过，覆盖设置刷新/导航、权限和原生名称、模型齿轮作用域、
  原生操作入口、模型标签、Vibe、BTW、胶囊展开状态、九语种文案，以及会话/草稿应用预设时完整覆盖角色。
- Chromium 组件测试：2 项通过，覆盖 390/1024px 胶囊尺寸、无横向溢出、Tab/Enter 操作、
  Vibe 不可点击与原生关闭状态，以及焦点轮廓不被滚动容器裁切。组件测试里的 Goal 内容是夹具，不冒充原生 Goal 实装。
- 移动端 typecheck、改动文件 oxlint（零警告）、oxfmt、UI 检查通过。
- 本次没有重跑以前的 Rust、Tai、worker、DSH 或 OpenCode 测试，旧通过数量不合并到本次结果。

## 截图

| 页面          | 实际截图                                                                                                               |
| ------------- | ---------------------------------------------------------------------------------------------------------------------- |
| 模型入口      | [桌面模型选择器](../../assets/providers/omp/ui-picker-desktop.png)                                                     |
| Provider 设置 | [桌面](../../assets/providers/omp/ui-settings-desktop.png)、[390px](../../assets/providers/omp/ui-settings-mobile.png) |
| 预设管理      | [项目目录与来源](../../assets/providers/omp/ui-presets-mobile.png)                                                     |
| 角色编辑      | [390px 下拉表单](../../assets/providers/omp/ui-editor-mobile.png)                                                      |
| 模型目录      | [独立目录与搜索](../../assets/providers/omp/ui-catalog-mobile.png)                                                     |
| 刷新          | [真实刷新反馈](../../assets/providers/omp/ui-refresh-desktop.png)                                                      |
| 删除确认      | [确认后取消](../../assets/providers/omp/ui-delete-mobile.png)                                                          |
| 当前状态      | [桌面](../../assets/providers/omp/ui-controls-desktop.png)、[390px](../../assets/providers/omp/ui-controls-mobile.png) |
| BTW           | [草稿恢复后的面板](../../assets/providers/omp/ui-btw-mobile.png)                                                       |

这些截图由当前页面直接截取，没有合成或修改界面。只证明所述操作与画面；
不证明真实模型推理、原生软键盘、屏幕阅读器、macOS/Windows 或签名安装包已验收。
其他逐命令缺口与 worker 原始 JSON 回执问题继续保留在 PR，不因本次界面修改视为完成。

## BTW 按需生命周期补充

用户新增要求后，BTW 的窗口和胶囊仅在新的 `/btw` 请求后出现。仅有历史、错误或迟到结果不显示。
胶囊控制展开/收起，内容提供追问、新问题和“结束 BTW”；结束移除窗口和胶囊，丢弃未发送草稿。
活跃问题通过原生 `btw_cancel` 单独取消，失败保留 UI；不取消主任务，不删除原生历史。
本地仅保存已结束的请求标识（按主机/会话隔离，最多 256 项），不保存问题、回答或草稿。
重新加载先读取该标识，防止旧快照闪现；新的 `/btw` 请求可重新打开。

- 本次直接相关前端 6 suites / 83 项通过，涵盖默认隐藏、结束/重开、重载与主机隔离、取消失败和等待准入。
- 在同一隔离 daemon 与官方外部 OMP 上，从真实 GUI 主输入框提交裸 `/btw`，没有模型调用。
  验证默认隐藏、打开、收起后草稿保留、结束后全部移除、页面重载仍隐藏，以及再次 `/btw` 重开为空草稿。
- 原生运行中的取消由组件回归验证；本次不为截图启动模型请求，不把空历史页面当作真实回答或追问证据。
- 当前图：[默认隐藏](../../assets/providers/omp/ui-btw-hidden-mobile.png)、
  [展开操作](../../assets/providers/omp/ui-btw-mobile.png)、
  [收起胶囊](../../assets/providers/omp/ui-btw-pill-mobile.png)、
  [结束后移除](../../assets/providers/omp/ui-btw-ended-mobile.png)。

此前 150 项属于上一轮 UI 修订，不与本次 83 项累加。

## BTW Follow up / Branch 补充

Follow up 沿用所选原生话题 ID，界面显示所追问的问题；新问题不带旧话题 ID。
Branch 使用原生完整记录，客户端不提交问答正文。单轮问答完成、主会话空闲且 leaf 未变化才可提升；
多轮、运行中、空答案和截短预览不提供分支入口。主任务运行时禁用分支，追问仍可并行。
操作固定在回答滚动区外；成功后新会话进入主时间线并关闭 BTW，失败保留面板和草稿。

外部 OMP 18.17.0 的 TUI 实现提供 `branchFromBtw`，但 RPC 和扩展上下文未开放它。
Ait 通过小型公开扩展的 `newSession(setup)` / `switchSession` 接口补齐，并使用原生 session manager
初始化回调保存 branch entries 与问答。没有 SDK import、私有运行时或宿主日志写入。
公开 new/switch hooks 与 TUI 的 `reason: btw` 不相同，此差异不隐瞒。

源码核对：[OMP v18.17.0 BTW controller](https://github.com/can1357/oh-my-pi/blob/v18.17.0/packages/coding-agent/src/modes/controllers/btw-controller.ts)、
[公开扩展类型](https://github.com/can1357/oh-my-pi/blob/v18.17.0/packages/coding-agent/src/extensibility/extensions/types.ts)。
验收中先按 JSONL 首行查 header 导致探针失败；OMP 首行可为 title 记录，修正为按 type=session 查找，未绕过父归属断言。

### BTW 功能验证（CI 修复前）

- `npm --prefix apps/mobile run test -- --project unit` 定向运行 runtime panel、BTW、原生菜单、dock、结束状态存储与翻译六个 suite：83 项通过。
- Chromium dock 定向浏览器测试 3 项通过：包含新增的长答案后追问输入完整滚入视口，焦点轮廓不被裁切。
- `cargo test --locked -p provider --lib local::omp::actions::branch`：2 项通过；
  `local::omp::tests::side_questions`：6 项通过；`local::omp::tests::actions`：4 项通过；
  公开扩展交付/内部命令隐藏边界：1 项通过。
- `AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -p provider --lib installed_omp_btw_follow_up_and_branch -- --ignored --nocapture`：1 项通过。
  使用官方外部进程与回环模型服务；验证原生追问、拒绝多轮提升、主任务忙碌/leaf 变化拒绝、父日志字节不变、
  新日志 parentSession、时间线替换、重启恢复与续聊。未调用真实上游模型。
- `node --test` 定向运行 BTW 分支与 Vibe 扩展测试：7 项通过，含空闲/归属校验、取消、加载失败恢复与已有 Goal 接线。
- workspace 构建通过，provider lib Clippy `-D warnings` 通过。当前系统 Rust 为 1.99.1，仓库与 CI 固定 1.98.1；
  workspace 全 targets Clippy 在原有 `host-link`/`domain` 测试触发 1.99 的 `assert_is_empty` 新 lint 而失败，
  workspace 构建还有原有 filesystem `fetch_update` 弃用警告。未修改无关文件或把全量 Clippy 写成通过。
- 移动端 typecheck、改动文件 oxlint（零警告）、oxfmt、rustfmt、UI 与文档检查通过。
- 390×844 真实页面操作通过：初始隐藏 → `/btw 问题` → Follow up → 新问题 → Branch → 新会话时间线 →
  裸 `/btw` → 收起保留草稿 → 结束移除 → 重载仍隐藏 → 再次 `/btw` 重开。
  为这条完整交互路径启动了本地模拟模型 API；不将其当作真实 Tai 推理。

| 本次 BTW 状态                      | 页面截图                                                                     |
| ---------------------------------- | ---------------------------------------------------------------------------- |
| 完成的单轮话题与两个操作           | [追问与分支](../../assets/providers/omp/ui-btw-branch-mobile.png)            |
| 追问输入完整可见                   | [Follow up 编辑](../../assets/providers/omp/ui-btw-followup-mobile.png)      |
| 原话题继续追问，多轮不提供单条提升 | [同一话题结果](../../assets/providers/omp/ui-btw-followup-result-mobile.png) |
| 分支后问答进入主会话、BTW 关闭     | [Branch 结果](../../assets/providers/omp/ui-btw-branch-result-mobile.png)    |

## CI 修复：BTW 实装测试的函数长度

提交 `030d1fd9` 的 Rust CI 因新增 BTW 实装测试的 `clippy::too_many_lines` 失败：
单函数 183 行，超过 100 行限制。此前只验证 provider lib 未覆盖测试 lint；系统 1.99.1
产生的基线警告不解释这次 CI 失败，也不能作为跳过全 targets 检查的依据。

按追问、忙碌/父会话变化拒绝、分支历史与归属、重启恢复四个场景提取私有 helper，
复用空闲等待逻辑，保留原用例及全部断言；没有豁免 lint，也没有修改运行时代码或 UI。
使用独立安装、与 CI 相同的 Rust 1.98.1 重新验证：

- `cargo fmt --all --check` 通过。
- `cargo clippy --locked --workspace --all-targets -- -D warnings` 通过，包含测试 targets。
- `AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -p provider --lib installed_omp_btw_follow_up_and_branch -- --ignored --nocapture`：1 项通过；
  官方外部 OMP 18.17.0 与本地模拟模型 API 验证追问、分支限制、父日志不变、历史提升及恢复续聊。
- `cargo build --locked --workspace` 通过。
- 报告 oxfmt 与仓库文档链接检查通过。前轮前端与截图验收不重复计入这次纯测试重构。

## Test coverage

未测量：用户未请求覆盖率。测试通过数量不代表覆盖率；若后续请求，定向测量 BTW 适配器和回执范围。
未运行完整 Rust workspace 测试。
