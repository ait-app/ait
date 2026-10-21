# ADR-178：OpenCode 轻量可用性探测与冷恢复预算

- 状态：Accepted，2026-10-21。
- 范围：OpenCode 私有适配器；细化 [ADR-153](adr-153-opencode-acp-cold-start-budget.md)，扩展
  [ADR-159](adr-159-opencode-acp-native-observation.md) 的只读目录来源；ACP 仍是唯一写入通道。

## 背景

Provider catalog 先检查可用性，再发现指定目录的模型。两步原先各启动一个 ACP 子进程；
第一步使用 daemon 的当前目录，会额外初始化该目录的原生配置、插件和数据库。
1.x 的只读目录回退还顺序执行模型查询与模式查询。

首次 `session/new` 已允许 120 秒，但恢复时的 `session/load`、配置变更重连时的
`session/resume` 和辅助生成的 `session/new` 仍沿用 30 秒普通控制预算。
这使相同原生冷初始化在不同入口得到不同结果。

## 决策

1. OpenCode 可用性只检查可执行文件和受支持版本。指定目录的 ACP 发现负责验证协议，
   并按实际 `initialize` 响应记录历史、列表与删除能力。协议启动失败仍使目录发现失败；
   版本探测成功不等于 catalog 已就绪。
2. 已验证的 1.18 原生 profile 优先从同一个受管 ACP 子进程内部 GET `/config/providers`
   和 `/agent`，沿用 ADR-159 的 loopback、独立随机认证、禁用代理及重定向和响应大小限制。
   不创建查询会话，不执行模型请求，不读取原生数据库或认证文件。HTTP 不可用或元数据
   无效时回退到原有只读 CLI；其他 1.x 继续使用 CLI，2.x 继续使用原生 ACP 目录。
3. CLI 回退中的模型与模式查询并行执行；模式元数据查询继续最多并发四个。
   任一查询失败均不发布部分目录。维持原生配置、模式可见性和推理强度。
4. 新连接上的历史恢复、检查历史和配置重连使用 120 秒启动预算；辅助生成的会话创建
   也使用同一启动预算。请求返回后恢复普通控制预算。已有连接上的轮次历史结算保持
   30 秒，前台模型生成继续不设置总运行时限。
5. 保留 2.x 数据库初始化 gate、进程回收、临时会话删除和失败后不自动重发输入的规则。
   不关闭用户插件或 MCP，不改写用户配置，不附着外部 server。
6. 为版本探测、启动排队和 ACP 初始化记录独立调试耗时。记录固定阶段和耗时，
   不记录原生配置、请求正文或认证信息。

## 后果

Catalog 少启动一次 ACP，避免不相关目录的插件冷启动；1.18 还复用该进程的原生目录，
省去逐条读取模式元数据时重复加载 CLI 运行时。能力仍由实际协议决定。
启动慢的原生恢复与辅助会话不会再被旧的 30 秒预算提前中断。
没有引入跨会话 ACP 复用，因此每个交互会话仍拥有自己的原生进程与配置隔离。

[官方 ACP 说明](https://dev.opencode.ai/v2/docs/cli/acp/) 指明 ACP 启动私有 server，
与 `run --attach` 使用的共享后台服务不同；本次不切换执行协议。
实际验证、外部 API 限制和覆盖率状态见
[加载速度验证](../../reports/providers/opencode-startup-real-usage-2026-10-21.md)。

1.18 目录来源：
[模型与 provider](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/server/routes/instance/httpapi/handlers/config.ts)、
[原生 agent 列表](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/server/routes/instance/httpapi/handlers/instance.ts)。
