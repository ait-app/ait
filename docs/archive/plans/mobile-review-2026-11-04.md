# 2026-11-04 移动端审查遗留事项

来源：2026-11-04 对 `apps/mobile`（源码提交 `e034e39a`）的审查，按 runtime/stores、composer、
screens/workspace、hooks/utils、git/terminal/file-pane、components/desktop 六个范围进行。
已确认的缺陷已在同一分支修复，各附回归测试；本清单只记录复核后确认、但尚未修复的问题。
每项写明触发条件、影响和建议做法，完成后勾选并附上修复与回归测试。

## Composer

- [x] **编辑排队消息会丢失其工作区附件。**
      `queueMessage` 入队时即调用 `clearSentAttachments`，从工作区附件存储移除 review、
      浏览器元素与 PR 上下文附件；`editQueuedComposerMessage` 只返回用户附件。编辑一条带 review
      或浏览器元素的排队消息后，这些附件无法找回。建议编辑时把工作区附件放回原作用域，或推迟到
      实际发送时再清理。（编辑覆盖当前草稿的问题已修复：排队内容现在合并到草稿前部。）
      修复（#635）：`editQueuedComposerMessage` 另行返回 `workspaceAttachments`，编辑时由
      `restoreSentContextAttachments` 放回当前附件作用域。回归：`composer/actions.test.ts` 中
      `editQueuedComposerMessage` 的返回值断言，以及 `composer/attachments/workspace.test.ts` 的
      `restores context attachments cleared by queueing when the message is edited`。
- [x] **立即执行的客户端斜杠命令失败后不恢复输入。**
      `runClientSlashCommand` 在 `onClientSlashCommand` 完成前清空草稿；失败只显示错误。
      目前只有 `/exit`、`/clear`，影响很小；新增带参数的立即命令前应改为成功后再清空。
      修复：`runClientSlashCommand` 改由 `composer/submit.ts` 的 `runImmediateClientSlashCommand`
      执行，只在命令成功后清空草稿文本与附件，失败时保留并显示错误。回归：`composer/submit.test.ts`
      的 `keeps the composer until the command succeeds` 与
      `leaves the command text and attachments in place when the command fails`。

## 键盘与快捷键

- [x] **非 QWERTY 布局录制的快捷键无法触发。**
      `keyboardEventToComboString`（`src/keyboard/shortcut-string.ts`）按 `event.code` 记录物理键，
      匹配（`keyboard-shortcuts.ts`）按 `event.key` 比较字符，只在 Alt、Space、Enter 时回退到 code。
      Dvorak 下录制的 Cmd+「T」被存为 `Cmd+K`，再次按下不触发，徽标也显示 ⌘K。建议可打印键按
      `event.key` 录制，或录制时启用 code 回退匹配；需要确认已保存绑定的兼容方式。
      修复：`keyboardEventToComboString` 对可打印键改按 `event.key` 录制（Shift 字符映射回基键，
      如 `!` 记为 `Shift+1`），与匹配读取的字段一致；Alt 组合、空格/回车等命名键以及无法用键名表示的
      字符仍按 `event.code`。匹配逻辑不变，已保存的绑定按原样继续生效，QWERTY 下录制结果也不变；
      此前在 Dvorak 下录制的绑定需重新录制。回归：`keyboard/shortcut-string.test.ts` 的
      `records a printable key by the character it produced, not its physical position` 等用例，以及
      `keyboard/keyboard-shortcuts.test.ts` 的
      `fires a shortcut recorded on Dvorak when the same keys are pressed again`。

## 侧边栏与桌面

- [x] **手动刷新的加载指示从未显示。**
      `deriveSidebarLoadingState` 固定返回 `isRevalidating: false`，而 `SidebarWorkspaceList`
      丢弃了 `isRefreshing` 与 `onRefresh`，`refreshAll` 也不返回 Promise。需要让 `refreshAll`
      返回 `Promise.allSettled(...)`，在列表接入 RefreshControl，并由 `left-sidebar.tsx` 跟踪该 Promise。
      修复：`refreshAll` 返回 `Promise.allSettled(...)`；新增 `useTrackedRefresh` 按该 Promise 显示指示，
      `left-sidebar.tsx` 改用它并删除固定为 false 的 `isRevalidating`；`SidebarWorkspaceList` 在原生端
      把 `RefreshControl` 接入项目与状态两种列表。回归：`hooks/use-tracked-refresh.test.ts` 与
      `components/sidebar-workspace-list.test.tsx` 的
      `settles refreshAll only after every host's refresh has finished, even a failed one`。
- [x] **每个更新 hook 实例仍各自发起挂载检查和「准备中」复查。**
      更新器已改为全局共享，安装与检查的竞争已消除；但侧边栏提示、设置页各自的挂载检查、设置页
      聚焦检查和 10 秒复查计时器仍重复调用 `check_app_update`。建议把自动检查调度也移入共享更新器。
      修复：共享更新器新增 `retainAutomaticChecks`，统一负责挂载检查（同一渠道 30 分钟内不重复）、
      30 分钟周期检查与「准备中」10 秒复查；hook 只持有该调度，侧边栏提示的周期计时器与设置页聚焦
      检查已删除。回归：`desktop/updates/desktop-app-updater.test.ts` 的
      `automatic check schedule` 一组用例，以及 `use-desktop-app-updater.test.ts` 的
      `runs one automatic check for the sidebar and Settings together`。
- [x] **部分桌面设置保存失败会提示两次。**
      `updateSettings` 需要继续抛出（主机页回滚等依赖该行为），错误提示由 mutation `onError` 负责；
      `useBuiltInDaemonManagement` 的 `onError` 和 `DaemonListenRows` 的内联错误又各报一次。
      修复：`useBuiltInDaemonManagement` 把设置保存失败包装为 `DaemonSettingsPersistError`，
      `onError` 遇到它不再提示；`DaemonListenRows` 保存失败时不再显示内联错误，只保留草稿供回车重试；
      `updateSettings` 仍然抛出。回归：`desktop/hooks/use-built-in-daemon-management.test.tsx`，以及
      `daemon-listen-rows.test.tsx` 中改写的
      `preserves a failed draft without reporting the failure a second time, and retries on Enter`。

## Git 与本地化

- [x] **并发丢弃不同文件共享同一个动作状态。**
      in-flight 去重已按参数区分，但 `discard-changes` 的 pending/success 状态仍按动作共享，
      先完成的请求会把状态切为成功。目前没有界面读取该状态。
      修复：`actions-store.ts` 按请求记录状态，动作状态取所有请求的汇总：任一请求进行中即为
      pending，全部结束且有成功时才为 success。回归：`git/actions-store.test.ts` 的
      `keeps discard pending until every concurrent discard of different paths settles` 与
      `drops a failed discard from the shared status without hiding a running one`。
- [x] **ru、ar 的 `schedules` 文案沿用英文。**
      该段直接继承英文资源，ru 计数 21 时 i18next 选到继承的英文 `_one`，显示「Every 21 minute」。
      需要翻译该段并补齐对应复数形式；es、fr、pt-BR 缺少 `_many`，只影响百万级计数。
      修复：ru、ar、es、fr、pt-BR 的 `schedules` 整段翻译，按各语言 CLDR 类别补齐 `timeline.limit` 与
      `cadence.*` 的复数形式（ru 补 `_few`/`_many`，ar 补 `_zero`/`_two`/`_few`/`_many`，es、fr、pt-BR 补
      `_many`），es、fr、pt-BR 同时补齐 `workspaceLabels.manage.deleteMessage_many`。ja、ko 随后也整段翻译。
      单数间隔在 fr、es、pt-BR、ar 中不再显示数字（如「Toutes les minutes」），ar 的双数形式去掉了括号中的数字；
      占位符检查因此允许 `_one` 形式省略 `{{count}}`。回归：`i18n/resources.test.ts` 的
      `resolves schedule counts with each language's plural forms instead of English`，复数完整性检查
      扩展到 es、fr、pt-BR。

## 测试覆盖

- [x] **原生网格终端的恢复路径没有单元测试。**
      `terminal-emulator-native-grid.native.tsx` 的 `restoreOutput` 已改为用恢复帧重新推导输入模式，
      与 `TerminalEmulatorRuntime` 的快照路径一致，但该原生组件没有测试夹具，只有运行时路径有回归测试。
      修复：恢复帧的解码与输入模式重新推导抽为
      `terminal/native-renderer/terminal-restore-output.ts` 的 `decodeNativeTerminalRestore`，组件改为调用它。
      回归：`terminal/native-renderer/terminal-restore-output.test.ts`。
