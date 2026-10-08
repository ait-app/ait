# ADR-109：DSH 原生插件情景模式与权限分离

- 状态：接受
- 日期：2026-10-08
- 修订：ADR-082 中将权限预设作为模式的映射。

## 背景

DSH 情景模式是 Agent preset，决定工具、提示词、技能和插件组合；permission preset
独立控制权限。原有 Ait 将三个固定权限名称作为模式，无法选择用户或插件声明的情景模式。

调研基于官方仓库 `5badb15009ae1756c3afe0ae0cef1faafc290ccc`：

- [registry 与插件组合](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/README.md)
- [目录、默认值和首次对话后的锁定](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/preset/agent-preset-registry/src/index.ts)
- [profile 配置层与插件归属](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/apps/cli/README.md)

## 决策

1. 原生适配器只读 `agentPresets/list` 和 `permissionPresets/catalog`，不创建探测会话。
   模式标签、描述、默认值来自实际目录；损坏的插件组合不可选，未知目录结构报错。
2. `modeId` 使用 `agent-preset:<原生 ID>`，避免与旧持久化权限 ID 冲突。
   创建通过 `session/create.request.agentPreset`，恢复和导入读取原生 `agentPreset` 投影。
   未指定时沿用 DSH 的默认值；绝不按官网列举的模式补齐目录。
3. 权限通过已有 `featureValues.permission_preset` select 表达，选项来自 Host。
   未指定时保持原生权限；旧无前缀 `modeId` 继续作为权限输入，显式 feature 优先。
   原生组合未提供 permissions 投影时隐藏该控件，不补造权限。
4. 已创建会话只显示当前情景模式。Ait 在创建时完成组合选择；要使用其他组合需新建会话。
   原生虽然允许空白会话重组，Ait 当前不提供此操作。禁止在现有会话上静默重组插件。
5. `AgentSession` 可提供会话专属控件和配置更新校验；application 消费此 port，
   不解析 DSH 协议。其他 provider 默认保留 client 级控件。配置目录校验不创建会话。
6. profile 默认 `web`，可用 `AIT_SERVER_DEEPSEEK_HARNESS_PROFILE` 指定已有 Web Host profile。
   插件安装、加载与生命周期仍由 DSH 负责。桌面专属 profile 不跨 profile 合并；
   CLI 不支持直接启动 Electron 专属 `desktop` profile。

## 后果与限制

不新增 crate 或跨功能依赖，不复制 DSH 插件注册表，不修改用户插件配置。
切换 profile 后需保持已有会话所依赖的插件和预设可用，否则由原生恢复明确失败。
目录刷新发现新安装预设；已运行 Host 的插件组合仍遵循原生生命周期。
目前不在 Ait 托管插件安装界面或执行 DSH 插件提供的专属 Web UI；插件的原生工具执行、
审批、问题、历史继续通过现有 Host 通道。ACP 兼容模式不提供原生情景模式。
