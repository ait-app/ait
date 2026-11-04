# 2026-11-04 移动端审查遗留事项

来源：2026-11-04 对 `apps/mobile`（源码提交 `e034e39a`）的审查，按 runtime/stores、composer、
screens/workspace、hooks/utils、git/terminal/file-pane、components/desktop 六个范围进行。
已确认的缺陷已在同一分支修复，各附回归测试；本清单只记录复核后确认、但尚未修复的问题。
每项写明触发条件、影响和建议做法，完成后勾选并附上修复与回归测试。

## Composer

- [ ] **编辑排队消息会丢失其工作区附件。**
      `queueMessage` 入队时即调用 `clearSentAttachments`，从工作区附件存储移除 review、
      浏览器元素与 PR 上下文附件；`editQueuedComposerMessage` 只返回用户附件。编辑一条带 review
      或浏览器元素的排队消息后，这些附件无法找回。建议编辑时把工作区附件放回原作用域，或推迟到
      实际发送时再清理。（编辑覆盖当前草稿的问题已修复：排队内容现在合并到草稿前部。）
- [ ] **立即执行的客户端斜杠命令失败后不恢复输入。**
      `runClientSlashCommand` 在 `onClientSlashCommand` 完成前清空草稿；失败只显示错误。
      目前只有 `/exit`、`/clear`，影响很小；新增带参数的立即命令前应改为成功后再清空。

## 键盘与快捷键

- [ ] **非 QWERTY 布局录制的快捷键无法触发。**
      `keyboardEventToComboString`（`src/keyboard/shortcut-string.ts`）按 `event.code` 记录物理键，
      匹配（`keyboard-shortcuts.ts`）按 `event.key` 比较字符，只在 Alt、Space、Enter 时回退到 code。
      Dvorak 下录制的 Cmd+「T」被存为 `Cmd+K`，再次按下不触发，徽标也显示 ⌘K。建议可打印键按
      `event.key` 录制，或录制时启用 code 回退匹配；需要确认已保存绑定的兼容方式。

## 侧边栏与桌面

- [ ] **手动刷新的加载指示从未显示。**
      `deriveSidebarLoadingState` 固定返回 `isRevalidating: false`，而 `SidebarWorkspaceList`
      丢弃了 `isRefreshing` 与 `onRefresh`，`refreshAll` 也不返回 Promise。需要让 `refreshAll`
      返回 `Promise.allSettled(...)`，在列表接入 RefreshControl，并由 `left-sidebar.tsx` 跟踪该 Promise。
- [ ] **每个更新 hook 实例仍各自发起挂载检查和「准备中」复查。**
      更新器已改为全局共享，安装与检查的竞争已消除；但侧边栏提示、设置页各自的挂载检查、设置页
      聚焦检查和 10 秒复查计时器仍重复调用 `check_app_update`。建议把自动检查调度也移入共享更新器。
- [ ] **部分桌面设置保存失败会提示两次。**
      `updateSettings` 需要继续抛出（主机页回滚等依赖该行为），错误提示由 mutation `onError` 负责；
      `useBuiltInDaemonManagement` 的 `onError` 和 `DaemonListenRows` 的内联错误又各报一次。

## Git 与本地化

- [ ] **并发丢弃不同文件共享同一个动作状态。**
      in-flight 去重已按参数区分，但 `discard-changes` 的 pending/success 状态仍按动作共享，
      先完成的请求会把状态切为成功。目前没有界面读取该状态。
- [ ] **ru、ar 的 `schedules` 文案沿用英文。**
      该段直接继承英文资源，ru 计数 21 时 i18next 选到继承的英文 `_one`，显示「Every 21 minute」。
      需要翻译该段并补齐对应复数形式；es、fr、pt-BR 缺少 `_many`，只影响百万级计数。

## 测试覆盖

- [ ] **原生网格终端的恢复路径没有单元测试。**
      `terminal-emulator-native-grid.native.tsx` 的 `restoreOutput` 已改为用恢复帧重新推导输入模式，
      与 `TerminalEmulatorRuntime` 的快照路径一致，但该原生组件没有测试夹具，只有运行时路径有回归测试。
