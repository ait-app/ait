# ADR-198：Pi 原生 reasoning 控制与默认值

状态：已实现，评审中。日期：2026-10-26。

范围：Pi adapter、provider session 控制与持久化，以及 `apps/mobile` 控制项。

修订：[ADR-032](adr-032-daemon-native-provider-execution.md) 与
[ADR-091](adr-091-independent-session-execution.md) 中仅用于配置的 session capacity 部分；
[ADR-034](adr-034-agent-config-session-events.md) 中可选的即时 native 控制与部分失败对账；
[ADR-156](adr-156-pi-native-rpc-provider.md) 中的 reasoning 控制与控制等待缓冲；
以及 [ADR-174](adr-174-pi-session-import.md) 中"不支持 thinking"的表述。

沿用：[ADR-179](adr-179-provider-owned-tests.md) 的 provider 自有行为测试，
以及 [ADR-189](adr-189-pi-without-host-mode.md) 的 Pi 自有工具与缺失的 host 模式。

## 背景

Pi 拥有模型 reasoning 能力、thinking level clamping 以及原生 API 映射。
已保存的请求 level 可能与 Pi 实际使用的不同，模型切换或扩展变更后尤其明显。
若把控制推迟到下一条 prompt，UI 在空闲时无法确认 native 选择。
Paseo 的 Pi adapter 提供了参考行为：立即应用 native 控制、回读 state、并采用有效配置。

## 决策

### 目录默认值

通过 `get_available_models` 发现模型，并在临时的 `--no-session` 进程中用 `get_state`
确认 Pi 配置的默认模型。出错即视为发现失败；校验显式选择只需要模型目录。

reasoning 模型暴露 `off`、`minimal`、`low`、`medium`、`high`，但被 `thinkingLevelMap` 中
null 条目禁用的 level 除外。`xhigh` 与 `max` 需要显式的非 null 映射。其他有效的非 null key
视为向前兼容的原生 ID。非 reasoning 模型没有 thinking 选项。

对每个模型沿用 Paseo 的目录提示：从 Medium 开始，选择第一个不低于它的受支持 level，
否则选择剩下的最高 level。该提示与 Pi 当前设置无关，也不声称是某个 live session 的实际 level。
catalog 没有给出默认档位时，表单保留未指定的 level：Pi 不退到第一个选项，
其他 provider 保持自身的回退行为。

### 原生控制与持久化

provider session port 提供可选的 `apply_config` 与 `effective_config` 操作。
`apply_config` 返回明确的处置结果：`Deferred` 保留现有的"仅 host 的下一轮"路径，
`Queued` 持久化 adapter 自有的意图，`Applied` 携带已确认的运行时事实。
manager 返回明确的 applied/deferred 结果给 RPC，而不是从 snapshot 是否存在来推断。
`Queued`、`Applied` 以及失败操作转移过来的缓存观测，都授权持久化 adapter 配置与 native handle；
默认 adapter 不通过该路径新增 handle 读取，也不覆盖已持久化的 native metadata。

client 声明 `supports_live_config`，application 在应用控制前先恢复休眠的 native session。
仅有持久化的 runtime snapshot 不能说明 session 是 live 的。恢复失败时拒绝该 patch，
而不是在 native readback 未变的情况下接受一个 deferred 变更。
没有该能力的 adapter 保留 deferred 的请求配置行为。
仅因配置而恢复的 session 在操作后关闭并释放，包括 native 拒绝或部分失败。
`AgentResumePurpose::Configuration` 标识立即 patch 中的显式模型选择与 thinking 变更；
只有 Pi 自己决定模型选择同时覆盖 thinking。仅配置的恢复不占用交互式 session capacity。

adapter 在注册前对未被触碰的已保存控制做对账，包括以 `--session-id --no-session`
重新打开、但从未 prompt 过的 session 的模型，同时抑制被替换的控制。
这样既保留身份，又不产生仅用于 setup 的 native 文件。
模型选择同时以它的原生默认值覆盖已保存的 thinking level。
只有显式的模型选择才抑制恢复已保存的 native 模型。
清除模型不发送 native 替换命令，因此休眠清除会先恢复该模型，并保持与 live 清除相同的 native 选择。

替换成功之前，adapter 保留原有的持久偏好。
被抑制的字段在注册期间与被拒绝之后，都在有效配置中保留其已保存值；
新进程的运行时默认值仍是独立的事实。
成功的模型命令确认替换后的模型及其默认 level，即使随后的 thinking 失败。
因此启动时的抑制不会在 native 拒绝后抹掉偏好。

临时配置 session 的注册与释放保留 Agent 之前的持久状态、记录的轮次错误与最后的模式，
除非该 patch 明确改变模式；它不发布显式的用户关闭。此前 live 的 session 保留其既有归属。
若持久化失败，立即关闭 native 资源，并保留已关闭 session 的待处理观测，
交由 polling 去持久化和释放；重试不会重放 native 变更。

client port 还提供感知 patch 的校验，默认对既有 adapter 执行完整的选择校验。
Pi 对模型未变的 patch 本地校验；显式模型选择仍需发现原生目录。
因此仅 thinking 的变更不会启动临时 Pi 进程，也不依赖目录可用性。
Pi 通过自己的 live setter 拥有 clamping 行为。

application 校验 patch，请 adapter 应用 live 控制，并把其返回的运行时状态、有效配置与
native handle 一起持久化。它不通过 provider 名字识别 Pi，也不实现 native clamping 规则。

Pi 通过 `set_model` 与 `set_thinking_level` 立即应用显式的模型与 thinking patch，随后读取 `get_state`。
模型变更接受目标模型的原生默认值；adapter 不恢复上一个模型的 thinking。
组合 patch 先选择模型再应用 thinking。
清除 thinking 只请求一次 Medium，Pi 可能对它做 clamping，且不保存 thinking 偏好；
清除模型时保留当前模型，因为 Pi 没有 unset RPC。

第一次 native 输入之前，Pi 不持久化仅用于 setup 的 session。
因此一次已确认的 thinking 清除可以留在 adapter 的 deferred handle 中，
跨临时 writer 的拆除与反复的仅 setup 恢复而保持可恢复。
它在当前 writer 中已经生效，不上报待处理配置，也不创建已保存的 thinking 偏好。
第一次 native 输入会移除该重置意图；之后的恢复遵循 native state 而不再重置。
成功的显式 thinking 选择或模型选择覆盖 setup 重置；被拒绝的替换则保留它。
对重置本身的明确拒绝会消费掉它。
普通轮次对账中缺失 thinking 偏好时不触碰 Pi 的 level；
只有显式的清除 patch 才请求一次性 Medium 重置。

即使显式重选的 thinking 与之前请求的值相同，也照样发送。
host 轮次进行中时，Pi 接受已校验的配置用于下一轮，不发送 native 控制或 readback RPC，
并返回现有的"Configuration applies next turn"提示。
adapter 在其持久化 handle 中保留合并后的命令意图，包括显式清除与重选；
registry 配置通过流式观测与重启保留下一轮的选择。
待处理控制在下一条 prompt 之前或空闲恢复时执行。
后续的模型切换覆盖更早排队的 thinking 命令，与顺序执行空闲控制一致。
接受后，在 polling 之前把被丢弃的 thinking 偏好与合并后的 handle 一起持久化，
而运行时事实保留当前轮次的 level。
deferred 意图还保留最后一次已应用的配置，使恢复阶段拒绝排队替换时仍能知道 thinking 是否为显式。
缺少该字段的旧 handle 仍可读取。
历史路径修复与刷新把新的恢复事实合并进既有 handle metadata，从而保留 adapter 自有的排队意图。

native 变更发生在活跃推理之外，因此 deferred 回复不会让运行中的轮次落后于控制截止时间。
空闲控制仍然立即生效，传输行为有界且 fail-closed。

创建与恢复时，若 Pi 已选中该模型则跳过 `set_model`，包括恢复出的 handle 仍带着
deferred 模型命令的情况。发送控制前先用 native state 对账该 handle，
在没有 thinking override 时保留已恢复的 level。
显式的 thinking 偏好与待处理的清除在模型对账之后仍然生效。
采用 native readback，包括 clamping 结果。
恢复到已选中的模型且没有 thinking override 时，每次恢复都保留 Pi 已恢复的 level。
native state 成为已保存的模型，并在注册时、控制成功后以及运行时观测变化时
确认或 clamping 显式的 thinking 偏好。
未指定或已清除的 thinking 偏好保持 `None`；
native readback 与扩展的 `thinking_level_changed` 事件更新运行时事实，但不制造恢复用的 override。
composer 与目录过滤继续独立于该偏好使用有效的运行时 level。
若 snapshot 缺少有效的模型或 thinking metadata，运行时事实把选择报告为 unknown，
而显式的已保存偏好保持完整。metadata 缺失不等于清除。
此前保存的非 null thinking 值没有来源标记，仍视为显式偏好；
清除一次即可释放该 override，且不猜测哪些值是用户选择的。

控制规划区分创建/恢复、显式 patch 与逐轮直接对账。
所有 native 请求等待都会立即投影其间发生的事件；传输层只保留有界的 reader channel。
session 输出在控制期间与 prompt 接纳期间使用同样的有界缓冲与合并，之后恢复正常轮询预算。
这样无需单独维护 raw-event 控制积压，也能保持流输出与结算的顺序。
返回的控制/运行时 snapshot 把发布权转交调用方。
待处理的 snapshot 与流式运行时观测共用一次受校验的原子持久化操作，
包含 usage 保留与可选的有效配置采用。
返回有效配置的 adapter 授权读取其 handle；
只有与 manager 上次持久化 handle 相比发生变化的 handle 才参与该原子写入。
默认 adapter 不通过运行时观测新增 handle 读取。
未变化的 live handle 不能覆盖 registry 侧的历史修复。
内存中的 handle 缓存只在该写入成功后推进，避免后续存储失败与重启重放已消费的重置。
manager 保留失败写入以便重试；失败的控制则把观测到的部分变更留作待处理。
配置或接纳失败时，`take_runtime_observation` 在不做 native I/O 的情况下把 adapter 缓存的
snapshot 转移给 manager，包括在拒绝 deferred 控制后 readback 失败的情形。
关闭时先持久化该观测的有效配置，再保存已消费的 deferred handle，
因此不做 polling 的关闭不会重放被拒绝的排队 level。
没有缓存观测的 adapter 保持默认的 `None` 行为。
在任何 `get_state` 请求期间，最新的有效 thinking 通知优先于该响应的 thinking snapshot，
包括重复缓存 level 的通知。
该顺序规则适用于启动与同步过程，作用域限于该次请求；
之后没有中间通知的 snapshot 仍然具有权威性。

只读的历史检视把所选分支上最新的 `thinking_level_change` 记录投影进有效配置。
有界的未知原生 ID 予以保留；格式错误的持久化 ID 使检视失败。
live RPC 事件与 JSONL metadata 各自保留其原生写法。

一条 native 命令可能成功，而之后的 native 命令或 registry 写入失败。
此时报告失败并保留 native readback 供后续观测；不得声称 native 执行已回滚，
也不得静默重放被拒绝的请求。
对于恢复期间被明确拒绝的 deferred 命令，adapter 返回其持有的 session，
并附带可选的 `initialization_error`。
注册会先持久化最后已验证的运行时事实、有效配置与已消费 handle，
然后关闭 session 并返回该拒绝。
若拒绝之后发生传输失败，已缓存的已验证事实仍可读取；
不确定的命令失败保留 deferred 意图用于恢复。
若注册写入在初始化改变 provider handle 之后失败，
保留已关闭 session 的已确认 snapshot 与已消费 handle 以便重试存储，
无论 deferred 控制是成功还是被拒绝。
polling、关闭与恢复会先刷出这些保留状态，再释放归属或打开新的 writer。
重试持久化不得重复一次性的 thinking 清除，也不得覆盖之后独立发生的 native 变更。

### 运行时与客户端展示

在 `runtimeInfo.thinkingOptionId` 与 `runtimeInfo.extra.thinkingLevel` 中发布有效 level。
目录过滤跟随该有效 level。
Pi 运行时检视在空闲时刷新 native state；
轮次进行中时返回由流式通知更新的缓存 native 观测，不发起控制 RPC。
因此接纳之后的运行时发布不会让新启动的轮次落后于控制截止时间。

对 Pi，composer 以 native 模型与 native thinking readback 为准，而不是以 catalog 默认值为准。
readback 的档位即使不在 catalog 选项里，也保留自己的标签，不被替换成第一个选项。
在 readback 可用之前，保持既有的 configured/catalog 回退行为。
子 Agent 事实优先使用同一个规范化的 `thinkingOptionId`；
较旧的 snapshot 可以回退到 `extra.thinkingLevel`。其他 provider 保持既有控制行为。

客户端的 provider 专属展示放在
`apps/mobile/src/provider-selection/control-presentation.ts`。
其 Pi adapter 负责解释兼容性 metadata、待处理意图、未知 thinking ID 与缺失的目录默认值。
共享的 composer 控制、事实、创建表单与 profile 表单消费规范化的展示与默认选择，
不按 Pi 身份分支，也不读取 native extras。
memo 化的 composer 与事实 selector 保留原始类型的展示值，
因此控制事实未变化时，新的 runtime 对象不会触发渲染。
显式的 provider 身份决定展示 adapter；外部的 runtime metadata 无法改变另一个 provider 的展示策略。
无需改动 wire schema。

Pi 拥有 API 专属映射：native `XHigh` 可能产生 `reasoning_effort: high`。
Ait 保留 Pi 的原生词汇。凭据、设置、扩展与 session 文件仍归 Pi 所有。
不引入新的 wire schema、daemon 行为测试归属或原生 subagent 枚举。

## 后果

native 模型与 thinking 变更需要空闲轮次，并在下一条 prompt 前完成确认。
deferred 控制保留当前轮次与 native 身份，同时保留用户为下一轮做出的选择。
`runtimeInfo.extra.configPending` 让 composer 展示这些选择，而不曲解当前轮次的 native 事实。
有效的 native 值取代过时的显式保存请求，而缺失的 thinking 偏好仍然缺失。
重新加载与扩展变更为没有 override 的 session 保留 native 默认值。
目录默认值是有用的按模型提示，而 native readback 对 live 控制始终具有权威性。
native 变更与 registry 写入是两个各自可能失败的独立操作，观测负责对账部分成功。

## 验证

- provider 自有测试覆盖目录 clamping、无需 prompt 的即时控制、模型切换、显式重选、
  部分成功后的 native 失败、扩展变更、恢复、daemon 重启后无 prompt 的反复 worker/RPC 模型切换，
  以及持久化的运行时/过滤身份。
- 已安装 Pi 的测试使用隔离设置与 loopback 模型，验证原生 level 与出站请求映射。
- 回归测试还覆盖：通过创建、观测与独立 native 变更后的反复恢复来检验未指定/已清除的 thinking 偏好；
  忙碌轮次下延迟的 native 回复与下一轮配置的接受；持久化的 deferred 清除与命令顺序；
  每次运行时观测只写一次；polling 之前即接受持久化的模型切换；
  在未指定、已清除与显式 thinking 选择下，对已选模型与不同原生模型的 deferred 恢复；
  缓存的活跃轮次检视；缺少 level 的 readback 不触发隐式重置；
  临时配置 session 的清理与存储重试；同一 session 上自然结束的轮次与其后的控制；
  state/abort 控制期间数千条流事件；有界投影失败；基于文件的分支 thinking metadata；
  不依赖目录的 thinking patch；以及不重复 native 执行或持久化的重试。
- 客户端测试覆盖桌面与紧凑布局下的模型选择，以及原生 thinking 标签与勾选标记。
- 边界回归验证默认 adapter 的配置不读取 native handle、不替换其 metadata，
  包括活跃轮次与 registry 写入失败时；
  客户端 adapter 测试验证其他 provider 忽略 Pi 专属 extras 并保持自身的目录回退行为。
- 恢复回归覆盖重启后被拒绝的 deferred 控制（有/无既有偏好两种情况）、部分模型成功、
  拒绝后 readback 失败、不确定的传输失败、改名的历史、metadata 刷新，以及休眠状态下的替换拒绝。

运维行为：[Pi](../../operations/pi.md)。
执行结果：[验证报告](../../reports/providers/pi-reasoning-controls-2026-10-28.md)。
