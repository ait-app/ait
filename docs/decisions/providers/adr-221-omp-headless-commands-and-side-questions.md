# ADR-221：OMP 的宿主命令边界与独立侧问答

状态：接受。日期：2026-11-06。扩展 [ADR-211](adr-211-omp-command-catalog-and-completion.md)、
[ADR-213](adr-213-provider-native-command-ownership.md) 和 [ADR-218](adr-218-native-ui-and-actions.md)。

## 背景

官方 OMP 的文本命令目录仍包含模型、权限、目录与原生会话控制，以及会启动外部界面的入口。
直接透传会与 Ait 的持久配置和 Workspace 身份冲突。未导出的 TUI 命令被当作普通提示词时，
还可能意外调用模型。`/status` 是 TUI 扩展控制台的别名，不能伪装成 RPC 已导出的通用状态查询。

只读查询原先经过普通 prompt 调度，会中断主任务。`command_output` 没有请求 ID，不能与另一个
命令的输出混接，也不能写成主回答。OMP 已导出 `btw`、`btw_cancel`、`get_btw_history`，但它们的
流式帧没有进入共享客户端；Claude adapter 当前没有可复用的专门侧问答实现。

## 决策

- 采用与 Codex adapter 相同的边界：只公开可由宿主管理的原生命令。OMP adapter 保存完整原生
  目录用于识别，向客户端投影已适配的 builtin，保留真实 extension、skill、file 和自定义命令。
  未知的新 builtin 默认不公开。模型、权限、工作目录、会话身份及外部界面由 Ait 的相应控件管理。
- TUI-only 名称与别名不进入模型 prompt；直接输入会得到独立控制错误卡片。包括 `/status`、
  `/extensions` 与原生 clear/new/exit 等名称，不把它们暗中转换成 Ait 的归档操作。实际注册的
  同名第三方命令仍属于其原生 owner。绝对路径和未知普通斜杠文本不套用这份屏蔽规则。
- 屏蔽官方文本 handler 明确不支持的子命令，例如 advisor configure、todo edit 和部分 MCP
  TUI/OAuth 操作，也屏蔽绕过宿主会话/账号管理的 session delete/pin 及模型交接 prewalk。
  trace 与 stats 均启动外部统计服务，不进入文本白名单。目录与协商的补全结果都应用策略；
  候选先在不可信边界校验一次，再筛选，
  不重新构造或修改候选的编辑范围、插入内容与光标。普通参数值保持原样。
- AgentSession 根据当前原生目录返回权威的 out-of-band 分类；未提供 live 分类的 Provider
  沿用 client 声明。同名 extension/自定义命令的否定分类不能被静态 builtin 名单覆盖。
  静态命中且 owner 尚未激活时先恢复 owner，再决定调度策略；恢复错误明确返回，
  不创建虚假的主 turn。该端口不包含 OMP 名称规则，其他 Provider 保持默认行为。
- `/usage` 等已知只读形式使用独立、有限的请求状态。只允许一个无 ID 文本查询同时执行；
  官方 builtin 的成功 `response` 在明确 `agentInvoked: false` 时已是最终回执，不再等待
  不存在的 `prompt_result`；其他路径继续等待权威 `prompt_result`。结果投影为
  `provider_command`，复用旧客户端兼容出口，不改动主 turn、主回答或审批。错误、超时和
  畸形输出保持独立；超时后仍保留关联直到原生结算，避免迟到输出污染主回答。
  主任务运行期间不执行另一个会产生同类无 ID 输出的 builtin。
- 控制卡由 Ait 生成，不存在于 OMP journal，使用 `host:provider-command:omp:*` 身份保存到
  宿主时间线。原生历史协调只替换原生拥有的内容，保留已生成的宿主控制卡及其身份；
  重复读取与宿主存储恢复不复制或丢弃结果。不将控制卡伪装成 diagnostic，不写回原生日志。
- `/btw <question>` 调用原生侧问答 RPC，`/btw` 打开原生历史。受理等待使用 detached receipt，
  Agent lane 继续轮询、处理问答和发布主流。侧问题不进入前台中断、steer 或后续 prompt 队列。
- `btw_record` 与 `btw_delta` 进入可选的 `providerUi.sideQuestions`，不写成主回答，不新增封闭
  时间线成员。共享界面独立显示流式回答、取消、追问和错误。追问使用原生 topic ID，主输入框
  保持可编辑。较晚的旧历史快照和取消后的增量不能覆盖新结果，提交成功也不清空较新的草稿。
- 原生 sidecar 是侧问答持久化来源；新 owner 只读恢复历史，不增加第二个 writer。连接观察到的
  running 与磁盘旧 running 区分，旧预览显示中断，不能永远锁住新提问。角色变化需要重开进程时，
  先保护尚在进行的侧问答和查询。UI 仅保留有界预览，完整记录仍由 OMP 保存。
- 所有 RPC 子进程显式传入官方 `--cwd`。仅设置进程工作目录不足以表达宿主选择：OMP 在 cwd
  等于 HOME 时会自动改到临时目录。恢复时继续检查原生日志的目录，不放宽归属校验或改写日志。

## 后果

Ait 的共享协议只新增可选 UI 数据，旧客户端仍能读取其它内容。专门的侧问答面板不阻塞主输入，
侧错误也不终止健康的主任务。此实现使用官方导出的接口，不执行任意第三方 `custom()` TUI 工厂，
也不把只读目录回退标记为完整原生多级补全。具体实测与限制见
[验收报告](../../reports/providers/native-delivery-and-omp-roles-2026-11-02.md)。
