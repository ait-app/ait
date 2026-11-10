---
id: ADR-245
status: accepted
date: 2026-11-10
amends:
  - providers/adr-242-omp-headless-commands-and-side-questions.md
paths:
  - crates/provider/src/local/omp
  - apps/mobile/src/composer
---

# ADR-245：OMP 命令来源、GUI 所有权与启动交互

状态：Accepted。日期：2026-11-10。

## 背景

命令名称不能代表业务语义。用户扩展可以拥有与内置配置或终端命令相同的有效名称，
未知命令也不能仅凭 status 或 toggle 字样推断成查询或开关。另一方面，模型、预设、
思考等级和角色配置必须由 Ait GUI 统一提交，不能通过内置命令绕过校验。

外部 OMP 的 `session_start` 还可以在初始化完成前等待标准选择或输入。如果 Ait
等待初始化结束才公布会话，用户便无法回答，形成互等。

## 决策

- 先使用当前外部 OMP 的有效命令目录和原生顺序，再按 `source` 分类。仅对真正的
  builtin 应用 Ait 的 GUI 所有权与终端限制。菜单、名称与参数补全、执行和 btw/vibe/goal
  特殊路由使用同一个来源判断。扩展、skill、文件、MCP prompt 和项目命令保留原生所有权。
- 被 GUI 承接的内置配置命令从菜单和补全移除；手输返回简短说明，不导航、不执行，
  也不成为模型提示词。保留底层 RPC 供 GUI 调用。不把“命令已屏蔽”计作相应 GUI 已交付。
- 普通扩展从菜单回填主输入框的 `/name `。仅显示原生提供的参数提示；参数补全不得
  通过执行命令探测。没有声明式适配信息时，不按名称猜测查询、动作或带状态开关。
- 标准 `select/confirm/input/editor/ask` 使用统一问题组件，绑定原生请求 ID 和会话。
  空输入、取消和拒绝保持不同语义。状态和文本 widget 按 key 更新与清除，widget 默认
  收起；插件回填主输入框前保留当前草稿的本地恢复入口，恢复不能跨会话执行。
- 初始化任务和唯一 writer 保持原有进程所有权。若启动阶段出现标准交互，先公布一个
  可回答问题的 provisional session；其 runtime model/session ID 在原生确认前保持未知。
  回答经同一 writer 发送，初始化完成后移交状态和待处理对话框。首次用户输入最多保留
  一份，取消或关闭时丢弃，不自动重放。普通模型／命令探测使用原生 headless RPC，
  仍加载项目扩展并遵循其无界面回退；实际 owner 使用 RPC UI。显式预设解析 helper 仍需
  读取自身标准通知，独立标为交互式探测。
- 插件报告模型变化后，adapter 校验原生模型与思考组合，再经 application 的统一配置
  校验和持久化路径提交。不能只改 runtime 摘要而留下过期的下一轮配置。
- 用户发起的已知查询写入持久化会话卡，提交后才完成回执；内部对象候选读取保持静默。
  runtime 开关以原生成功回执为准，提交期间显示等待；拒绝不保存未生效的选择。
- Ait 不承诺将任意终端组件转换成 GUI。上游未发给 RPC 的 TUI 行为无法自动检测，
  需要插件标准回退或明确能力声明。不执行任意插件前端代码，不附带修改版 OMP/SDK/Bun。

## 当前运行控制与历史

原生 Goal 当前状态通过可选 `providerUi.goal` 发送，与时间线分页分离；DSH 与 OMP
复用已有结构化 Goal 事实。输入框上方常驻当前控制，展开提供暂停、恢复、清除或退出；
折叠后在同一横向按钮行保留状态，窄屏横向滚动。Vibe、侧问答、插件状态与 widget
复用同一折叠容器。折叠不卸载内容、不取消任务、不改原生模式，披露状态仅按主机/会话
本地保存。历史卡仍保留取证，当当前 Goal 控制可用时不重复开放历史动作。
OMP 当前 Goal 的 GUI 操作使用原生 RPC，并在写入前核对展示的 Goal ID；不重新解释成
同名插件命令。原生确认快照覆盖其前已缓冲的 Goal 状态事件，保留无关轮次和交互帧。

## 任务模式与限制

Vibe 继续采用 ADR-243 的外部进程编排。模式是否显示由原生确认的 enabled 决定，
与 worker 是否空闲分离。退出先撤销原生队列并取消 director/worker，收到关闭确认后才
清除模式；期间收到的无关 parent turn 帧必须交回拥有者处理。

Goal 使用公开 `goal` RPC；guided goal 使用公开扩展进入主会话访谈，原生 Goal 确认前
不显示启用。RPC 当前只有 get/create/pause/resume/drop，不能把缺少 budget/set 接口或
TUI sessionModes 的能力宣称为已实现。计划、循环等其余任务流程需要各自适配和验收，
不以普通提示词冒充原生控制。

## 验证与后果

验证至少覆盖同名插件、参数补全、启动前交互、空输入与取消、首次输入只发送一次、
会话隔离、配置变化校验、查询卡持久化、开关拒绝和运行中退出。真实模型验收与离线
协议测试分开报告。逐项差距见[命令处置表](../../archive/reports/providers/pr612-omp-command-dispositions-2026-11-10.tsv)。
