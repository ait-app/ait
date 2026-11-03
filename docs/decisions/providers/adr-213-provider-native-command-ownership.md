# ADR-213：DSH 与 OpenCode 的原生命令所有权

状态：接受。日期：2026-10-30，2026-10-31 补充控制隔离与已知版本结算边界。扩展 [ADR-212](adr-212-native-editor-completion.md)，
延续 [DSH Host](adr-082-deepseek-harness-native-host.md) 与
[OpenCode ACP](adr-115-opencode-acp-provider.md) 的协议边界。

## 背景

DSH 命令是插件注册的 Host 事务，不是模型提示词或 OMP 风格的参数树。OpenCode 的
自定义命令则由 ACP 原生展开模板、参数和 Agent/模型选择。共享客户端不能替代这些
处理器，也不能用 Ait 的归档动作截获 Provider 的同名命令。

## 决策

- Session port 增加只读 `refresh_commands`，默认返回最近观察到的目录；已有会话在
  自己的执行 lane 刷新，保留 native owner。其他 Provider 的现有发现接口保持独立。
- DSH interactive Host 调用 `commands/list({agentId})`，保留原生名称、描述、输入
  提示、definitionId 和附件能力。`commands/change` 会触发有界刷新，空目录同样生效。
  草稿通过官方 `--patch` profile overlay 将 `session-persistence-jsonl` 的 root 指向
  adapter 拥有的临时目录，在其中组合原生 Agent。查询后关闭 Host 并删除临时存储，
  不在用户的 session corpus 中创建空会话，也不调用模型。无法应用 overlay 的 profile
  报告失败，不回退到用户的持久存储。
- DSH 命令通过现有 out-of-band port 调用 `commands/execute`，保留原始参数，
  不发送 `session/prompt` 或打断当前模型回合。未知命令明确拒绝。收到原生
  `command/run` 后即可继续发布事件和问题；原生 `command/done` 的结果与历史共用
  投影。目标驱动等自主 `turn/start` 使用实际原生 turn 身份发布 Started。
- 新建 Agent 的首条输入与后续输入共用 `deliver` admission；Provider 声明为
  out-of-band 的命令在首条输入时也必须进入原生命令通道，不走模型 prompt。
- 原生命令改变已确认的 feature 值时，adapter 发布 `FeatureValuesUpdated`；application
  验证会话约束后保存配置，后续提示词不得恢复旧权限值。跨 lane 的目录快照保留实际
  owner 发布的 features 和 availableModes，不以静态空目录覆盖原生控件。
- DSH 目录查询、命令事务和模型执行各自隔离诊断。后台刷新失败保留旧目录；主动查询仍报告错误。
  已受理命令的 HTTP 响应失败显示控制错误，不能终止当前模型；命令结果使用独立投影，不能覆盖模型的最终回答。
- 未显式指定模型/推理时，DSH 保留原生默认值，允许模型目录为空的会话使用控制功能。
  显式选择仍必须通过目录验证，不合成可用模型。automation-only ACP 不冒充 interactive
  Host 的插件命令和业务界面。
- OpenCode 使用标准 ACP `available_commands_update`，保留顺序、命名空间和实际提供
  的 input hint。前台 reader 与宿主通过 watch 同步目录；查询不替换 reader，空更新
  不回退旧值。空闲读取保留其他通知，交还原执行流程处理。
- OpenCode 草稿仅在 native delete 能力存在时创建自己拥有的临时 ACP 会话，并在成功
  或失败后删除。缺少 delete 时不创建持久空会话。命令执行继续由 ACP prompt 交给
  原生处理器，Ait 不展开 `$ARGUMENTS`、不猜子命令或复制命令的 Agent/模型规则。
- 已验证 OpenCode 2.1.3 的自定义命令 ACP 只应答受理，不能据此结算模型。该版本未协商
  原生回执修复时，在执行前拒绝受影响的斜杠输入；普通提示词和 /compact 保留原路径。
  [上游修复提案](../../../third-party/opencode/README.md)由 handler 返回原生输入回执，
  ACP 校验会话和输入身份后等待已有事件流，不增加第二个 writer 或轮询推断。
- OMP、DSH、OpenCode 的斜杠名称由 Provider 拥有，客户端不将 clear/new/exit 等改成
  归档动作。模型、模式、权限等已有控件仍按各自 Provider 的真实能力展示。

## 后果

三者共享编辑契约，不共享命令语法或业务实现。当前 DSH Host 和 OpenCode ACP 不导出
通用动态参数函数，不能把目录回退标为原始多级补全。客户端专属业务面板、OMP 任意
TUI 工厂、未导出的操作以及 DSH 命令附件仍有独立缺口；详见
[逐项验证与差异清单](../../reports/providers/dsh-opencode-capability-adaptation-2026-10-30.md)。
