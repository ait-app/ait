# PR #612：命令来源、启动交互与真实 Tai 验收

> 此报告保留原验证时的界面与数据。Vibe 展开/退出按钮及设置页布局已由后续修订替换；当前界面见[设置页与状态胶囊验收](pr612-ui-controls-and-settings-2026-11-10.md)。

日期：2026-11-10。基线 main：`3737bc0276be56bcf6ca2e6d069f87c4d1368392`。
合入提交：`b872335d`；本轮最终生产源码：`6a330e73`。本报告与其后同批源码及截图一并提交；准确版本以报告所在提交为准。
PR 保持未合并。本轮是定向修复与验收，不代表此前逐项命令要求全部完成。

## 变更范围

- 内置 GUI 配置命令与终端专属命令从菜单及补全移除，手输给拒绝卡，不导航、不推理。
  判断使用外部 OMP 的有效目录来源；同名插件不套用内置限制。btw/vibe/goal 特殊路由也遵循来源。
- 未知插件默认回填主输入框，不按名称猜查询或开关。原生参数提示只作装饰，不进入实际草稿。
  参数补全不通过执行命令探测。普通元数据探测使用 `--mode rpc --no-ui --no-session`，保留扩展加载。
- 启动前标准交互经同一 writer 提前公布；首次输入等待原生初始化，取消不重放。
  插件修改模型/思考等级后回读原生状态，再经统一校验持久化。草稿回填提供同会话恢复入口。
- 已知查询单击执行，结果进持久化卡片；内部候选读取不刷会话。中文搜索匹配实际显示名称。
  文本框方向键与中文组合输入优先于菜单导航。
- 自动压缩/重试等开关等待原生回执后更新，失败不存假状态。保留既有图标；开启用图标颜色，
  关闭用弱化颜色，不加常驻圆圈边框。等待显示进度，并保留 pressed/busy 可访问状态。
- Vibe 已启用但无运行 worker 时仍显示模式；运行中可退出并停止 director/worker。
  同步控制期间收到的无关父轮次帧交还原拥有者，避免丢失取消回执。
- 原生 Goal 通过公开 RPC 创建/查看/暂停/恢复/移除；guided goal 在主会话访谈，确认后才显示 Goal on。
  不把尚无实现的预算、已有目标 set、Plan/Loop 等写成已完成。

边界见 [ADR-245](../../../decisions/providers/adr-245-omp-command-ownership-and-startup-dialogs.md)。
逐项缺口见 [88 个内置命令处置表](pr612-omp-command-dispositions-2026-11-10.tsv)，含别名和附带命令。

## 真实任务与证据

使用用户安装的官方 `/usr/bin/omp` 18.17.0；Tai 端点 `api.inferknock.ai`，
模型 `tai/deepseek-v4.1-flash`。模型凭据只复制到忽略的私有临时目录，不进入提交、日志或截图。
没有模拟模型响应，没有下载、修改、安装或打包私有 OMP/Bun/SDK。

场景为“离线中文三人值班交接卡”：固定甲/乙/丙名册，添加事项，待办/完成切换，
localStorage，原生键盘操作和 390px 布局。先由真实 Tai 访谈、用户确认并建立原生 Goal，
再暂停/移除 Goal，进入 Vibe 交付并验证。访谈时不显示 Goal on。

| 原生会话                               | 用途                  | 结果                                                     |
| -------------------------------------- | --------------------- | -------------------------------------------------------- |
| `01a1c3e3-c8f8-73bb-9176-edb0b1a8a300` | director              | 真实 Tai，非零用量，调用工具并读回校验报告               |
| `01a1c3f1-792a-7796-8805-01549dbddab6` | good / High，实现页面 | 已写出页面；为验证运行中退出，主动取消，不能计为正常完成 |
| `01a1c3f6-ec5e-753a-8a63-57ae92de506b` | fast / Low，独立验收  | 完成，写出 VERIFICATION.md；真实浏览器及静态检查         |

两条 worker 的 session header `parentSession` 均指向上述 director 日志。三者模型记录均为 Tai，
均存在非零用量；父思考选择与 worker High/Low 分开记录。模式重进后恢复历史，不复制 worker。

作者另打开生成的真实页面，以 390×844 浏览器检查 Enter 添加、Space 切换、重载持久化：
`scrollWidth=390`、`innerWidth=390`、完成项保留、`pageerror=[]`、非 file 网络请求 `[]`。
这不是 Ait 原生软键盘验收，也不把 worker 自述当作作者独立检查。

| 文件            | 字节 | SHA-256                                                            |
| --------------- | ---: | ------------------------------------------------------------------ |
| TASK.md         |  312 | `8b1832a1e575b1ae9ef8006b8c407f4d2b998cd098449101e69e99c46f95b5cd` |
| index.html      | 9755 | `9a1345db9f303896d8824046d2c1f2e86efa74439b137f757b752b3d7c217b54` |
| VERIFICATION.md | 7085 | `bce8ab02b4273781aac0bab89180f1555ca12a6732f89d54370847436b3a9875` |

产物和原生日志是临时验收数据，不作为生产代码提交。可分享截图放在
[OMP 取证目录](../../../assets/providers/omp)。本轮照片按功能使用 `tai-toggle-*`、
`tai-guided-goal-inline-hint`、`tai-goal-dock-*`、`tai-plugin-*`、
`tai-vibe-before-active-exit`、`tai-vibe-active-exit-complete`、`tai-vibe-idle-enabled`、
`tai-vibe-real-task-running`、`tai-localized-query-search`、`tai-session-statistics-history-card`
和 `tai-real-task-artifact-390`。其中配置、空闲和查询截图不代表该动作本身调用模型，
它们属于同一真实 Tai 任务上下文。

## 插件交互与实际发现

验收扩展通过公开 API 发出启动 select、普通 input、editor 回填、文本 widget 和 thinking 变更。
实际空输入 `""` 与取消分开；插件回填后的原草稿可恢复；选择 Low 后持久请求值和原生值均为 Low。
启动问题在输入受理前出现，回答后释放真实 Tai 请求。普通命令/模型探测以 `--no-ui` 启动，
官方实测仅收到 ready/目录/协议/get_state，不请求选择，也没有执行命令来猜参数。

定位启动等待时，曾使用忽略目录内的临时 stdio 观察包装，只记录帧类型、请求 ID 和取消标记，
不修改 OMP、不记录凭据，最终运行恢复直接 `/usr/bin/omp`。观察到原生在约 30 秒后主动取消
未回答的启动问题；取消前 Ait 未代答。及时回答可正常初始化，不能承诺无限等待。
隔离 daemon 重启验收还遇到旧进程释放数据锁较慢，第一次新进程启动被锁拒绝；待旧进程退出后恢复，
没有删除锁或同时运行两个拥有者。

## 输入框上方的常驻控制（后续交互修订）

按本轮最终要求，当前 Goal、Vibe、侧问答和插件状态/widget 共用输入框上方控制区。
展开显示当前状态和操作；折叠后保留同一横向按钮行，390px 可横向滚动。
披露状态按主机/会话隔离，仅保存展开项身份，不缓存敏感草稿；折叠不卸载内容、不取消任务。
Goal 状态来自可选的 `providerUi.goal`（显式 null 表示确认没有当前目标），与历史分页独立，DSH 和 OMP 从各自原生事实投影。
OMP 暂停/继续/清除先核对 Goal ID，避免陈旧按钮操作新目标；原生快照覆盖先前缓冲的旧 Goal 更新，
不会丢失无关父轮次帧。当前 Goal 可操作时，历史卡保留显示并收起重复动作。实际完成回执截图为 `tai-goal-completed-receipt.png`；没有伪造一个仍然活跃的完成态面板。

定向补充：OMP Goal 6 项（含陈旧 ID 与缓冲次序），DSH 控制 3 项，前端运行控制 4 suite / 23 项，
协议当前状态/命令 9 项、九语种 56 项通过；这些与此前用例重叠，不累加成覆盖率。
截图新增 `tai-goal-dock-*`、`tai-vibe-dock-expanded` 和 `tai-runtime-controls-row*`，
使用同一实际 Tai 场景，未注入虚构 Goal 或模型回答。实际补充一条 Tai 侧问答，回答完成后与 Vibe 同排；退出 Vibe 后从主输入建立只读验收 Goal，输入区按钮完成暂停、折叠、重开、清除，390px 的 scrollWidth 与 innerWidth 均为 390；清除后侧问答仍可用。

## 定向验证

Rust 工具链 1.98.1。以下通过数量互不重复累加，旧报告结果不计入本轮：

- `cargo test -p provider local::omp`：208 通过，22 忽略（`70937ccb` 的定向范围）；最终完成态修订后，Goal 6 项重新通过。覆盖来源碰撞、启动前对话框、元数据无界面、
  空值/取消、单次首次输入、Goal、Vibe 活跃退出、原生开关回执和持久卡片。
- `node --test crates/provider/src/local/omp/vibe/host/extension.test.mjs`：3 项通过，验证 Goal 工具公开准备、缺失能力拒绝、guided goal 复用与确认约束。
- 最后补拍发现直接 Goal 未激活原生 goal 工具；已在创建前通过公开扩展确认并启用，新增顺序回归。OMP 完成后原生当前 Goal 为空，界面移除当前控制并保留历史完成回执（真实 Tai 已调用 complete，原生 get 返回 goal/state null）；统计查询成功后关闭菜单，相关 15 项组件测试通过。
- `cargo test -p provider observed_model_selection_commits`：1 通过。
- 前端 20 个直接关联 suite / 124 项通过：native-actions、native-action-model、vibe、runtime-panel、
  combobox-editing、agent-controls、command-argument-hint、commands-query 与 autocomplete。
- `messages.provider-command.test.ts`：4 项通过，包含可选 OMP Goal 与兼容分支。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo build --workspace`、rustfmt、
  移动端 typecheck、oxlint、仓库 oxfmt、UI 检查通过。这些构建/lint 不等于全量测试。
- SDK 构建及文档校验单独执行；新 head 的 GitHub CI 状态以 PR 的当前运行记录为准，不能沿用上一 head 的绿色。

未执行完整 Rust workspace 测试、完整 88 命令端到端验收、覆盖率或正式安装包验收。

## Test coverage

**未测量**：没有请求覆盖率测量。上述通过项是执行结果，不是覆盖率，也不存在本轮可比较的覆盖率基线。
若后续请求测量，使用 `cargo llvm-cov` 对本轮受影响的 provider 模块与关联用例定向生成可分享报告，
记录提交、行覆盖率、covered/total、排除及忽略范围；不自动扩大为 workspace 全量运行。

## 剩余范围与限制

- 命令处置表仍有未完成项：公开 RPC 缺少的 Goal budget/已有目标 set，以及 Plan、Loop、Queue、Tan、
  OMFG、Cleanse、扩展状态、完整统计仪表盘、远程 trace 等。部分 GUI 管理功能尚未完整承接；屏蔽入口不算交付。
- `ait-vibe-result` 原始 JSON 仍可能进入主时间线，需要后续整理。早期验收发现的 Goal 缓冲事件回退与重复卡已增加确认快照覆盖修复；旧历史取证不删除。
- 旧 daemon 的 Goal 历史卡保留文本兼容动作。新当前 Goal 控制使用原生快照；OMP 操作在写入前核对 Goal ID，当前控制可用时历史卡不重复提供动作。
- `--no-ui` 本轮只验收官方 18.17.0。显式预设解析 helper 仍使用交互式通知探测，
  没有承诺任意项目插件启动问题在该独立探测中都可回答。纯 TUI 方法仍受上游 RPC 限制。
- Director 工具可见列表不是安全边界；真实首轮曾发现原生 deferred 工具发现可使 bash 仍可调用。
  后续验收提示限定 read/todo/vibe\_\*，不是对任意模型的强制安全隔离证明。
- Ait 输入区 390px 参数提示没有横向溢出；完整菜单、设置页、辅助技术、原生软键盘、macOS/Windows 未全面验收。
- 场景产物有静默处理损坏存储、无 noscript、无 aria-live 三项改进建议，属于验收产物，不是本 PR 的 Ait 产品变更。
