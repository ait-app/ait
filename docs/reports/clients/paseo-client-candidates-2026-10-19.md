# Paseo 0.12.0 客户端增量借鉴清单

研究基线记录：以下差距按研究时的固定提交描述。后续实施见 [0.12.0 移植与验证](paseo-0.12-port-2026-10-20.md)。

日期：2026-10-19。结论：先补聊天恢复正确性和多 Agent 缓存读取，再迁移长聊天性能、文件预览及移动端操作；Markdown 引擎整换单独评估。

## 范围与依据

- Paseo 本地只读仓库：`/Users/necokeine/Documents/paseo`，干净工作树，HEAD 为 [91154ba4e8ace3484595a05a4366279fb4818745](https://github.com/getpaseo/paseo/commit/91154ba4e8ace3484595a05a4366279fb4818745)，版本 0.12.0。
- 比较起点：[a1da1b8f234afe7a75785022292cdf8a50ee11a5](https://github.com/getpaseo/paseo/commit/a1da1b8f234afe7a75785022292cdf8a50ee11a5)，对应[上一批移植报告](paseo-desktop-app-port-2026-10-10.md)。研究增量为 `a1da1b8f..91154ba4`，并读取当前源码确认最终行为。
- Ait 核验基线：`808b9bcdc7267b68af6f844f9632de0fe4975d53`。任务开始时为 `799558f1`，研究期间外部更新工作树后，差距判断以 `808b9bcd` 重新核验。
- `packages/app` 对应 Ait 的 `apps/mobile`；本报告覆盖共享 Web、桌面 renderer 与原生移动界面。提交链接及 blob 链接用于定位本地已读源码，未以远端最新状态替代本地依据。
- 优先级与成本为研究判断。成本“小”指局部前端逻辑，“中”指多个界面/状态模块，“大”指跨原生依赖或协议后端配合；未测量实际性能，也未复现用户设备故障。

## 建议顺序

| 顺序 | 候选组                                        | 优先级       | 用户价值                                   | 迁移成本           |
| ---- | --------------------------------------------- | ------------ | ------------------------------------------ | ------------------ |
| 1    | 重载、catch-up 后保留完整回复                 | P1，第一批   | 避免最后一段回答消失                       | 小                 |
| 2    | 按目标缓存行失效，避免其他 Agent 饿死读取     | P1，第一批   | 一边运行多个 Agent，一边即时打开旧聊天     | 中                 |
| 3    | 历史窗口、列表投影、布局持久化减少重复工作    | P1           | 长聊天和多工作区流式时降低渲染开销         | 中，可拆           |
| 4    | Markdown 文件中的本地图片、目录及显式文件链接 | P1           | 文档预览真正可浏览、可跳转                 | 中                 |
| 5    | 手机全屏草稿编辑及触控焦点管理                | P1           | 长输入可编辑，切换标签不弹键盘             | 中                 |
| 6    | 手机辅助内容 Sheet 与脚本操作入口             | P1/P2        | 查看文件、子 Agent、服务状态后快速回到对话 | 中到大，可拆       |
| 7    | 手机 Diff 评论 Sheet 与直接发送反馈           | P2           | 在手机完成改动审阅闭环                     | 中                 |
| 8    | Command Center 按匹配层级和近期活动排序       | P2           | 更快找到刚用过的工作区与会话               | 小到中             |
| 9    | 用量刷新失败保留成功快照、稳定卡片排序        | P2           | 刷新失败时仍有参考数据且不误导新鲜度       | 小到中，快照需后端 |
| 10   | 统一 Markdown 引擎及跨段选择                  | P3，独立试验 | 提升原生选择与多端语义一致性               | 大                 |

## 1. 重载和 catch-up 后保留完整回复

**建议优先移植 [#7256 / 90c6de058](https://github.com/getpaseo/paseo/commit/90c6de058039fd6ea57f68e4b3417300835f9805)。** 上游实际修复的是一个可直接对照的合并缺口：缓存前缀已在 tail 展示，实时 chunk 又进入 head，此时 catch-up 投影覆盖完整回答。旧逻辑找到 head 就不查 tail，留下缓存前缀；回合结束去重时，完整 head 又可能被当作已经提交而丢弃。新逻辑同时匹配两端，仅在 epoch 和投影来源序号覆盖条件满足时移除 tail 中的前缀。实现和测试见该提交的 `timeline/session-stream-reducers.ts` 与 `timeline/replica.test.ts`。

Ait 的 [session-stream-reducers.ts](../../../apps/mobile/src/timeline/session-stream-reducers.ts) 仍有 `const tailMatch = headMatch ? null : findMatch(params.tail)`，head 分支直接返回旧 tail，与上游修复前一致。这是源码确认的差距，尚未声称在 Ait 实机复现。应一并迁移上游“缓存前缀 → live chunk → catch-up 完整投影 → turn_completed，始终只保留一条完整回复”的回归用例，并保留 Ait 自有语音/原生历史投影。

## 2. 缓存读取只受目标变化影响

**建议优先迁移 [#7074 / e0de0fdf6](https://github.com/getpaseo/paseo/commit/e0de0fdf6f85d70620aa98312a29372f4682db47)。** 上游不再用整个 host 的 revision 判断一次读取是否过期，而是登记 `{ serverId, kinds, ids }` 的活跃读取，只有命中目标行、相关 baseline 或 host 生命周期变化才失效。测试明确覆盖另一 Agent 连续 200 次流式更新期间，目标 agent/workspace/timeline 仍然能读取；目标自身变化、host 移除后重加、持久化拒绝仍被正确处理。源码见该提交的 `runtime/replica-cache/index.ts` 与同目录测试。

Ait 的 [replica-cache/index.ts](../../../apps/mobile/src/runtime/replica-cache/index.ts) 仍使用 `hostRevisions`、`advanceHostRevision` 与 `canReadHostRevision`。因此同 host 无关 Agent 的提交仍会令目标读取重试，属于真实实现差距。Ait 已有持久 Timeline 页面和先画缓存的能力，不能把这次优化写成“首次支持缓存”；它修复的是已有缓存读取在并发流量下的可用性。迁移应保留失败关闭语义，不能通过放宽所有 revision 校验得到旧数据。

## 3. 长聊天和多工作区减少重复工作

这一组适合按模块拆分，均不需要换 Markdown 引擎：

| 上游实际变化                                                                                                                                                                                                                      | Ait 当前差距                                                                                                                                                                                                                                                             | 成本与收益                                                                                                |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| [#7052 / 8d424e039](https://github.com/getpaseo/paseo/commit/8d424e039ea3f328500b08861689bbe98d65e567)：复用相同 tail 的当前历史 slice；历史计时独立于流式 head；未启用的 outline 不扫描 Timeline；只缓存最近窗口，允许旧窗口回收 | [agent-stream/model.ts](../../../apps/mobile/src/agent-stream/model.ts) 仍每次执行 `tail.slice(historyStart)`，计时缓存同时以 tail/head 为键；[use-chat-outline.ts](../../../apps/mobile/src/agent-stream/chat-outline/use-chat-outline.ts) 仍拼接已加载历史和 live head | 中；降低每个 streamed delta 对不变历史的重复计算。上游用身份/回收断言验证，未在此推导 FPS 数字            |
| [#7342 / 559c0200e](https://github.com/getpaseo/paseo/commit/559c0200e098cd257150c2b63c51470bfac7dd32)：workspace visibility、子 Agent selector 按源 Map 身份复用结果；无关会话变化但行相同时继续返回旧数组                       | [agent-visibility.ts](../../../apps/mobile/src/workspace-tabs/agent-visibility.ts)、[subagents/select.ts](../../../apps/mobile/src/subagents/select.ts) 仍直接派生列表，缺少对应 selector factory                                                                        | 小到中；避免每次聊天更新遍历全部 Agent。上游测试检查 200 个 Agent、100 次 stream commit 没有新增 Map 遍历 |
| [#7157 / 0d7a234e6](https://github.com/getpaseo/paseo/commit/0d7a234e6381576d32015dbd54469497708e416f)：保留整个 sidebar entries 集合的身份                                                                                       | [sidebar-workspaces-view-model.ts](../../../apps/mobile/src/hooks/sidebar-workspaces-view-model.ts) 已复用单行对象，但每次仍返回新 Map                                                                                                                                   | 小；补齐已有行复用，减少重新打开 sidebar 的下游投影失效                                                   |
| [#7284 / 095dd0316](https://github.com/getpaseo/paseo/commit/095dd0316e4681af3b3f7ccaa70e004e1cd3e26f)：`SplitPaneView` memo 与正确的 Unistyles 动态宽度封装                                                                      | [split-container.tsx](../../../apps/mobile/src/components/split-container.tsx) 的 pane 仍为普通函数                                                                                                                                                                      | 小；拖动 Explorer 宽度时保持无关 pane 稳定，需验证 props 身份是否同时满足                                 |
| [#7358 / c575464e8](https://github.com/getpaseo/paseo/commit/c575464e8f58802176859d69f05e7eb1100d87fa)：复用持久化切片投影，在校验/序列化之前跳过未变布局；保存失败清除去重记录                                                   | [workspace-layout-store.ts](../../../apps/mobile/src/stores/workspace-layout-store.ts) 的 `partialize` 每次重建投影；[validated-persist-storage.ts](../../../apps/mobile/src/storage/validated-persist-storage.ts) 无 `skipUnchanged`                                    | 小到中；减少无实际布局变化时的序列化和写入，真实布局修改仍立即保存                                        |

Android 的 [#7278 / bd5061e7b](https://github.com/getpaseo/paseo/commit/bd5061e7bd99b863dd5275de5210c41472788790) 只建议提取“跳转距离超过底部 overscan 时不做动画”的规则。Ait 的 [strategy-native.tsx](../../../apps/mobile/src/agent-stream/strategy-native.tsx) 已是 `windowSize={10}`，但 `scrollToBottom` 仍直接使用传入 `animated`。因此不能宣称 Ait 缺少十屏窗口，也不宜机械套用上游随新 Markdown 原生测量调整的所有批量参数。

## 4. 让 Markdown 文件成为可浏览的文档

上游 [#6750 / 9eaf7ad0c](https://github.com/getpaseo/paseo/commit/9eaf7ad0c1be74995b07dc82c81d9a010e932418) 和 [#6877 / 73221ce5b](https://github.com/getpaseo/paseo/commit/73221ce5bd25b5f003efcb041a0d107d3c766805) 将预览文件路径、workspace root、client 传给 Markdown：相对图片按 Markdown 文件所在目录解析，经当前 session 的文件读取获取，含 URL 编码路径。当前实现见 [file-panels/markdown-preview/index.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/file-panels/markdown-preview/index.tsx) 和 [components/markdown/images.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/components/markdown/images.ts)。

Ait 的 [FileMarkdownPreview](../../../apps/mobile/src/file-pane/markdown-preview/index.tsx) 只接收 `source` 并传 `text`，没有文件位置和读取 client，无法正确解释 Markdown 文件目录下的相对本地图片。可先借鉴路径/读取上下文设计，沿用现有 renderer，避免把该功能绑死在引擎替换上。

文件链接也值得补齐三个较独立的行为：

- [#7205 / fa70b215a](https://github.com/getpaseo/paseo/commit/fa70b215a80fc0b3fe59e1dcff8e286ace020be1) 根据 `listDirectory` 的事实区分文件和目录，打开 Files 并展开内容；生成序号防止较慢的旧点击覆盖新点击。实现见 [path-open.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/assistant-file-links/path-open.ts)。Ait 尚无这层 opener。
- [#7210 / 06cdc5a0b](https://github.com/getpaseo/paseo/commit/06cdc5a0b83a2f2f6875613441a86aaa96e59c15)、[#7233 / a0aeca967](https://github.com/getpaseo/paseo/commit/a0aeca967c9cf0af433c76b1f0ba0c50daa419c4) 区分显式 Markdown 链接与自动识别的裸文本；显式目标可以是无已知扩展名的文件/目录，不必强行经过模糊后缀搜索。Ait 的 [resolver.ts](../../../apps/mobile/src/assistant-file-links/resolver.ts) 尚未向分类器传 `explicitLink`，[parse.ts](../../../apps/mobile/src/assistant-file-links/parse.ts) 也未有此选项。
- 非 ASCII 与百分号路径要采用“显式 href 解码、inline code 保持字面值”的规则，但 [#6520 / 563c014dc](https://github.com/getpaseo/paseo/commit/563c014dc156a18f71745c1f3f23e863308322e5) **已被 Ait 覆盖核心语义**：当前 resolver 已按 `sourceType`、linkify 来源选择 `literal`/`url`，parse 也有安全解码。应复用现状、补边界回归，不能当作新增主功能重复移植。

## 5. 手机全屏草稿与触控焦点

[#6531 / 750cb82fb](https://github.com/getpaseo/paseo/commit/750cb82fbb296b66d38198b07ac632c7dc6efd7a) 新增长草稿全屏编辑；后续修复了键盘移动/清空、提交后回聊天、iOS 键盘覆盖，并最终让 inline/fullscreen 使用同一个编辑器树保留缓冲、选区与焦点。应按当前源码而不是首个实现迁移，见 [composer/input/presentation.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/composer/input/presentation.tsx)、[composer/dock/internal/fullscreen.native.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/composer/dock/internal/fullscreen.native.tsx)。相关收尾提交为 [#6902](https://github.com/getpaseo/paseo/commit/f3ddf99fe35020353954e63829ed64e87f97d58a)、[#6987](https://github.com/getpaseo/paseo/commit/74282dac123a8d1b987f4af4ec8dbf3716b91699)、[#7170](https://github.com/getpaseo/paseo/commit/a1c6e8dfafee91ac250af09553f7975a38fb3c25)。

Ait 的 [composer/input/input.tsx](../../../apps/mobile/src/composer/input/input.tsx)、[composer/types.ts](../../../apps/mobile/src/composer/types.ts) 无对应 fullscreen 编辑状态/入口，属于新能力。建议保持同一编辑器，不以定时同步两个输入框实现，否则容易丢失尚未发布的草稿或 IME 选区；验收要覆盖显式替换为空串、发送失败保留输入、提交成功退出与只由当前 pane 处理 Back/Escape。

触控焦点也应一并借鉴 [#7130 / 6017706cc](https://github.com/getpaseo/paseo/commit/6017706cc147ae64c7b0f0a31fa94e304912b364)、[#7352 / b8bcd204a](https://github.com/getpaseo/paseo/commit/b8bcd204a918dde8fd996362cae284eee83241f0)：触控导航不恢复 composer 焦点，切换标签主动结束旧输入焦点。该交互应与 Ait 已有的 Android 边缘面板手势共同验证，不应统一禁用桌面键盘导航后的焦点恢复。

## 6. 手机辅助 Sheet 与脚本入口

[#6827 / e135e9c10](https://github.com/getpaseo/paseo/commit/e135e9c101700b31671fc374325d17b608467a91) 及 [#6958 / e5124b039](https://github.com/getpaseo/paseo/commit/e5124b039ab20755ac97ee207d03ca9903e5cfb7) 将手机文件、子 Agent 等辅助内容放入可关闭、可转普通标签的 workspace Sheet，并收口叠层管理。最终边界包含 target policy、未加载 Agent 保持根标签、未保存内容关闭保护、只有可见 workspace 消费 Back；见 [workspace-tabs/compact-sheet.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/workspace-tabs/compact-sheet.ts)、[workspace-sheet-host.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/screens/workspace/workspace-sheet-host.tsx)。

Ait 的 [workspace-layout-store.ts](../../../apps/mobile/src/stores/workspace-layout-store.ts) 无 `sheetByWorkspace` 或这套 sheet policy。已有通用 Sheet 堆叠/Android Back 修复，不能等同于 workspace 辅助内容的可返回导航。适合在文件预览之后实施；需要迁移状态机和关闭保护，成本高于只添加一个弹窗，不能恢复已移除 Plugin 目标。

更小、可先交付的子项是 [#7122 / 62ab2deb2](https://github.com/getpaseo/paseo/commit/62ab2deb27bf258262edbb7d260724b1a738eec2)：在手机 workspace header 菜单列脚本、展示运行/健康状态，提供 Run/Restart/Stop/终端/打开服务动作。实现见 [workspace-scripts-menu-pages.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/screens/workspace/scripts/workspace-scripts-menu-pages.tsx)。Ait 的 [workspace-header-menu.tsx](../../../apps/mobile/src/screens/workspace/workspace-header-menu.tsx) 现有新 Agent、终端、浏览器和定时任务入口，但无脚本菜单。迁移时复用 Ait 的 Rust 脚本 RPC 和服务访问机制，不能带回已删除的 service proxy URL 字段。

## 7. 手机 Diff 评论与反馈闭环

[#6455 / b26b9234c](https://github.com/getpaseo/paseo/commit/b26b9234c0f088ed97abbaa981f002da7817498e) 的价值是让窄屏评论不再挤在 Diff 行内，并直接将评论发送到目标 Agent。当前实现 [review/comment-sheet.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/review/comment-sheet.tsx) 明示文件、旧/新侧与行号；[review/feedback/index.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/review/feedback/index.ts) 包含单收件人直发、多收件人选择、共享 sending 所有权、失败保留草稿和仅清除本次已发送评论。

Ait 已有 [review/surface.tsx](../../../apps/mobile/src/review/surface.tsx) 的行内评论和 [review/store.ts](../../../apps/mobile/src/review/store.ts) 的 review attachment；未有对应 comment-sheet/feedback 模块。建议沿用已有附件契约，实现手机 Sheet 与明确的发送动作。提交期间新增评论不能一起清空，同一个 review 从两个 surface 发起不能重复发送。这组是已有 review 的交互完善，不是首次支持行内评审。

## 8. Command Center 排序和可读性

[#7277 / a65478d0b](https://github.com/getpaseo/paseo/commit/a65478d0bfd25fa8b08adac54273e3e490ca341a) 对内建结果采用“匹配层级优先，同层近期活动优先”，保留 PR/MR 数字的精确命中规则，避免隐藏 cwd 命中压过用户看得到的标题；结果增加命中高亮、独立时间标签和项目图标。实现见 [command-center/results.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/command-center/results.ts)、[workspace-search.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/command-center/workspace-search.ts)、[command-center.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/command-center/command-center.tsx)。

Ait 的 [workspace-search.ts](../../../apps/mobile/src/command-center/workspace-search.ts) 已支持 PR/MR 数字识别，但同分仍依赖外传 tiebreak；[command-center.tsx](../../../apps/mobile/src/command-center/command-center.tsx) 仍包含 workspace 标题比较与 Agent 状态优先排序。建议迁移排序和高亮，但先确认 Ait 是否仍要让“待权限处理”在空查询下优先；该项属于产品选择，不能把现有 attention 排序直接视为 Bug。图标和时间更新可复用现有组件，不必引入 Plugin 结果注册能力。

## 9. 用量刷新：保留参考值并明确失败

[#6947 / 208e32303](https://github.com/getpaseo/paseo/commit/208e323035ab40bbf3b3476fb6175330bc79821c) 在完成一次流式刷新时，保留已展示卡片顺序、移除消失项、把新项追加到尾部。Ait 的 [usage/model.ts](../../../apps/mobile/src/usage/model.ts) 仍在 `settleReports` 直接 `finished.map`，会跟随完成列表顺序重排。这个局部前端小改可独立迁移。

[#7246 / cf62b0091](https://github.com/getpaseo/paseo/commit/cf62b009116ad1365928a605b1f4887dcbc810e6) 则增加了独立的最后成功快照：最新尝试决定 error/unavailable 提示，最后成功数据决定额度条与成功时间。失败不能被显示为一次“Updated”，过期 reset 时间不能继续写“resetting now”，旧值也不进入侧栏 pinned summary 冒充当前可用额度。源实现见 [usage/model.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/usage/model.ts)、[usage/card.tsx](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/usage/card.tsx)、[usage/pinned.test.ts](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/usage/pinned.test.ts)。

Ait 的 [usage/card.tsx](../../../apps/mobile/src/usage/card.tsx) 在失败时返回空的 windows/balances/details，仍以 `entry.fetchedAt` 展示刷新时间；[usage/types.ts](../../../apps/mobile/src/usage/types.ts) 与协议尚无 `lastSuccessful`/`successfulAt`。因此不能仅 cherry-pick 卡片 UI：应在 Ait Rust 用量缓存/DTO 增加快照及时间语义，再接展示；旧 daemon 不提供历史时应保留“无成功更新”的状态。Ait 的本地化用量文案和原生账号来源必须继续保留。

## 10. Markdown 整体引擎替换暂缓，保留设计借鉴

[#6973 / eb1a8a2ec](https://github.com/getpaseo/paseo/commit/eb1a8a2ec7618b013a890aaf32d999bc69a7df65) 将三端统一到 `react-native-enriched-markdown`，完成消息保持一个原生文字视图以跨段选择；流式时固定已闭合 head，只更新未完 tail，完成后合并。它同时重写了 chat、plan、preview、selection-copy、Find 与图片路径，多处行为依赖约 2,900 行第三方包 patch。设计与已知限制见上游 [markdown-renderer.md](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/docs/markdown-renderer.md)、[native-text-selection.md](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/docs/native-text-selection.md)。

Ait 的 [components/markdown/renderer.tsx](../../../apps/mobile/src/components/markdown/renderer.tsx) 仍使用 `react-native-markdown-display` 及自有 block renderer，属于明确技术差异。不过上游仍依赖 nightly、原生重编译、Android 测量缓存和手势 patch；原生 Mermaid 会打断选择，嵌套 list/quote 中的图形容器不能完整保留，SVG badge 和段首 inline image 仍有限制。这不是适合混入普通同步的一笔依赖升级。

建议先迁移第 1—9 组中不依赖新引擎的行为；若 Ait 要以跨段文字选择为明确目标，再独立验证 iOS/Android、网页 Find/复制、Mermaid、中文输入、图片到达时滚动锚点与现有聊天搜索，之后决定是否换引擎。尤其 [#7251](https://github.com/getpaseo/paseo/commit/3c48ac3ad28488355b1aaf3435cb6ea07ada33d4) 的 inline-code 路径修复发生在新 renderer 中，Ait 已有旧引擎路径识别，不能脱离上下文机械搬回。

## 已覆盖或不宜直接同步

| 项目                                                                     | 判断及依据                                                                                                                                                                                                                                                                                         |
| ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 上批历史锚点、图片尺寸预留、复制、内容宽度、目录重连、用量入口、速度档位 | 见[已移植报告](paseo-desktop-app-port-2026-10-10.md)，不能重新列为本批新增能力；本报告只列其后续差距                                                                                                                                                                                               |
| 问答失败恢复 #7098                                                       | Ait 已在 [10-13 报告](question-response-timeout-2026-10-13.md)实现 5 秒超时、恢复按钮、保留输入和手动重试，且有 [question-form-card.test.tsx](../../../apps/mobile/src/components/question-form-card.test.tsx)。上游新加的按 agent/request 匹配错误并内联 Alert 尚可补；不能把整个恢复能力列为缺口 |
| 非 ASCII 链接解码 #6520                                                  | Ait 当前 resolver 已区分 literal/url，见第 4 节；不建议重复认领                                                                                                                                                                                                                                    |
| Relay 下载 #6595                                                         | Ait [download-store.ts](../../../apps/mobile/src/stores/download-store.ts) 已按 `activeConnectionId` 选择连接，并有 accountRelay 浏览器、原生流式、desktop 分支；不能据 Paseo 提交宣称 Ait 无 Relay 下载。其他 socket/SSH transport 的完整性不在本客户端报告结论中                                 |
| Android 十屏聊天窗口                                                     | Ait 已是 `windowSize={10}`；仅建议借鉴远距离回底部的动画上限，见第 3 节                                                                                                                                                                                                                            |
| Plugin 图标、插件 Timeline Find、插件重载超时等                          | Ait 已移除 Plugin，且上批移植明确不恢复。新 renderer、sheet、命令中心需要只带内建目标                                                                                                                                                                                                              |
| Paseo 新 Provider 目录/图标、发布版本号、发布脚本                        | 客户端细节不代表可直接替换 Ait 的 Rust Provider 或发布体系；应按 Ait 实际目录和平台分别评估                                                                                                                                                                                                        |

## 本次验证范围

本次只写本报告。依据为本地 Git 提交 diff、当前 Paseo 源码/回归测试及 Ait `808b9bcd` 源码对照；没有执行产品交互、性能 benchmark、原生构建或业务测试。文档修改不涉及 `bins/`、`crates/`，按仓库要求不运行 Rust 测试。后续实施时应按每组列出的契约选择直接相关测试，不能把上游测试存在视作 Ait 已通过验证。

已通过本文件 Oxfmt 格式检查、`node scripts/check-docs.mjs` 本地链接检查（401 份 Markdown）和本文件 `git diff --check`。
