# Workspace Bot 重设计 · 阶段 2：客户端 Bot 标签页 v2 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让客户端接入阶段 1 的 `workspace-bots-v2`：每个 Workspace 一个 Bot 标签页，未配置时显示设置向导，配置后为对话与设置（模型、暂停／恢复、记忆、Telegram／飞书 Channel、移除），整体仍在 `experimentalFeatures` 开关之后。

**Architecture:** TS 协议包按 v2 重写 Bot 请求／响应 schema，SDK 只改能力门控；移动端 Bot 标签页以 `workspaceId` 为唯一身份（单例标签），旧持久化标签在读取时归一化。组件沿用现有 `bots/` 目录的文件划分：标签外壳、设置向导、对话、设置、Channel 管理。

**Tech Stack:** TypeScript、zod、zod-aot（`npm run generate:validators`）、React Native（Expo）、@tanstack/react-query、vitest、Playwright。

**Spec:** [workspace-bot-manager.md](workspace-bot-manager.md)（“Bot 标签页（客户端）”“Channel 与对话”“记忆”“生命周期”）；服务端契约见 [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md) 与 [Workspace Bot](../operations/bots.md)。

## 范围

- 包含：v2 协议 schema、能力门控、单例标签与旧标签迁移、设置向导、对话、设置页（模型、暂停／恢复、记忆、移除）、外部 Channel 管理、浏览器 e2e、文档。
- 不包含：权限分组与模式上限、启动 Agent 默认值（阶段 3 向导再增加这两步）；通知唤醒与来源展示（阶段 4）；退役 Bot 历史查看（服务端无接口）；在 Ait 中只读查看 Telegram／飞书对话（规格“Channel 与对话”，与阶段 4 的回复路由一起做）。

## Global Constraints

- 能力门控：`workspaceBots = features.has("workspace-bots-v2") && BOT_METHODS.every(has)`；旧 Host（只有 v1）显示 `bots.unsupported`。
- 实验开关关闭：隐藏新建入口；已恢复的 Bot 标签页显示 `bots.disabled`，不发起任何 Bot 请求；关闭开关不影响服务器上的 Bot。
- 每个 Workspace 至多一个 Bot 标签页（`singleton: true`，`resourceKey` 固定为 `"bot"`）；标签目标为 `{ kind: "bot" }` 或 `{ kind: "bot", view: "settings" }`，不再包含 `botId`。
- 旧持久化目标 `{ kind: "bot", botId?, view?: "list" | "create" | "settings" }` 必须能被读取：`view: "settings"` 且有 `botId` → `{ kind: "bot", view: "settings" }`，其余 → `{ kind: "bot" }`。
- 客户端请求一律用 `workspaceId`，不出现 `botId`；客户端类型中不存在 `botId`、`name`、`instructions`、`enabled`、`archivedAt`。
- `bot.get` 返回 `bot: null` 表示未配置；`state.kind` 为 `active` / `paused`（`reason: "user" | "directory_missing"`）/ `retired`。
- 访客（`!canManageBackgroundTasks(serverId)`）只读：可看对话与设置，不显示设置向导提交、发送、暂停、移除、记忆保存、Channel 编辑。
- 文案只在 `en.ts` 与 `zh-CN.ts` 维护（其他语言 `bots: en.bots`）；新增或删除的 key 两个文件同步。
- 消息 16 KiB、记忆 32 KiB 的客户端校验保持不变。
- 只运行受影响包的测试：`npm run test --workspace=@ait/protocol`、`--workspace=@ait/client`、`apps/mobile` 中 `npx vitest run src/bots src/workspace-tabs src/stores src/panels`（按改动收窄）与对应 `typecheck`；格式用 `npx oxfmt --check <file>`，lint 用 `npx oxlint <file>`。本阶段不改 Rust，不运行 Rust 测试。

## Review Focus

1. **旧持久化标签**：升级前打开了 `{botId, view:"list"}` 的标签，升级后必须恢复为 Bot 标签而不是丢弃或崩溃；两个旧 Bot 标签合并为一个单例。→ Task 2 测试 `legacy_bot_targets_normalize_to_singleton`。
2. **配置在另一台设备完成**：标签停在向导时，`bot.get` 变为非 null 后自动切到对话，不能重复提交 setup（重复提交时服务端返回 `created:false` 也要正常进入对话）。→ Task 3 测试 `wizard_switches_to_chat_when_bot_appears`。
3. **暂停中的 Bot**：对话可读，输入框禁用并显示恢复按钮；`directory_missing` 显示目录缺失说明，恢复失败（`WorkspaceNotFound`）显示错误且保持暂停。→ Task 4 测试 `paused_bot_disables_composer_and_offers_resume`。
4. **移除后**：标签回到向导（`bot.get` 为 null），旧对话不可见；再次设置默认保留记忆，勾选“清空记忆”才清空。→ Task 5 测试 `remove_returns_to_wizard`、Task 3 测试 `setup_sends_clear_memory_only_when_checked`。
5. **Channel 凭据引用**：只接受 `env:AIT_SERVER_CREDENTIAL_*` 形式，允许列表为空时不能提交；界面从不显示或要求输入明文 token。→ Task 6 测试 `channel_form_requires_env_reference_and_senders`。

---

### Task 1: v2 协议 schema 与能力门控

**Files:**

- Modify: `packages/protocol/src/bot/types.ts`、`packages/protocol/src/bot/rpc-schemas.ts`
- Test: `packages/protocol/src/bot/rpc-schemas.test.ts`
- Modify: `packages/client/src/protocol/messages.ts:82`
- Test: `packages/client/src/protocol/connection.test.ts:340-350`
- Regenerate: `npm run generate:validators --workspace=@ait/protocol`（生成文件随提交）

**Interfaces:**

- Produces（`@ait/protocol/bot/types`）：

```ts
export const BotStateSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("active") }),
  z.object({ kind: z.literal("paused"), reason: z.enum(["user", "directory_missing"]) }),
  z.object({ kind: z.literal("retired"), at: z.string() }),
]);
export const BotViewSchema = z.object({
  workspaceId: z.string(),
  execution: BotExecutionSchema,
  builtinChannelId: z.guid(),
  state: BotStateSchema,
  createdAt: z.string(),
});
export type BotView = z.infer<typeof BotViewSchema>;
export type BotState = z.infer<typeof BotStateSchema>;
```

`BotChannelSchema`、`BotConversationSchema` 删除 `botId`；删除 `BotSummarySchema`、`BotSchema` 及其类型。

- Produces（请求，全部 `z.strictObject`，带 `type` 与 `requestId`）：

| 方法                                                              | 参数                                                                |
| ----------------------------------------------------------------- | ------------------------------------------------------------------- |
| `bot.get.request`                                                 | `workspaceId`                                                       |
| `bot.setup.request`                                               | `workspaceId`, `execution`, `clearMemory?: boolean`                 |
| `bot.update.request`                                              | `workspaceId`, `execution`                                          |
| `bot.pause.request` / `bot.resume.request` / `bot.remove.request` | `workspaceId`                                                       |
| `bot.channel.list.request`                                        | `workspaceId`                                                       |
| `bot.channel.types.request`                                       | （无）                                                              |
| `bot.channel.create.request`                                      | `workspaceId`, `channelId`, `config`, `enabled?`                    |
| `bot.channel.update.request`                                      | `workspaceId`, `channelId`, `config?`, `enabled`                    |
| `bot.channel.delete.request` / `bot.channel.status.request`       | `workspaceId`, `channelId`                                          |
| `bot.conversation.list.request` / `bot.conversation.open.request` | `workspaceId`                                                       |
| `bot.conversation.reset.request`                                  | `workspaceId`, `conversationId`                                     |
| `bot.message.send.request`                                        | `workspaceId`, `conversationId`, `messageId`, `text`                |
| `bot.history.get.request`                                         | `workspaceId`, `conversationId`, `afterSequence?`, `limit?`（1–50） |
| `bot.turn.get.request` / `bot.turn.cancel.request`                | `workspaceId`, `turnId`                                             |
| `bot.memory.get.request`                                          | `workspaceId`                                                       |
| `bot.memory.update.request`                                       | `workspaceId`, `text`, `expected`                                   |

`workspaceId` 一律 `z.string().trim().min(1)`。删除 `bot.create`、`bot.list`、`bot.archive`、`bot.permission.resolve` 的请求与响应。

- Produces（响应 payload，均含 `requestId`）：`get` → `{ bot: BotViewSchema.nullable() }`；`setup` → `{ bot, created: boolean }`；`update` → `{ bot, configurationAppliesToNewSessions: boolean }`；`pause`／`resume`／`remove` → `{ bot }`；其余与现有 v1 响应形状相同（channel／conversation 用去掉 `botId` 的 schema）。

- [ ] **Step 1: 写失败测试**（`rpc-schemas.test.ts`）

```ts
it("parses v2 workspace-addressed requests and rejects removed v1 methods", () => {
  expect(
    BotRequestSchema.parse({
      type: "bot.setup.request",
      requestId: "r1",
      workspaceId: "wks_1",
      execution: { provider: "codex", model: null, modeId: null, thinking: null },
      clearMemory: true,
    }).type,
  ).toBe("bot.setup.request");
  expect(() =>
    BotRequestSchema.parse({ type: "bot.list.request", requestId: "r", workspaceId: "w" }),
  ).toThrow();
  expect(() =>
    BotRequestSchema.parse({ type: "bot.get.request", requestId: "r", botId: crypto.randomUUID() }),
  ).toThrow();
});

it("parses unconfigured and paused bot views", () => {
  const unconfigured = BotResponseSchema.parse({
    type: "bot.get.response",
    payload: { requestId: "r", bot: null },
  });
  expect(unconfigured.payload).toMatchObject({ bot: null });
  const paused = BotResponseSchema.parse({
    type: "bot.pause.response",
    payload: {
      requestId: "r",
      bot: {
        workspaceId: "wks_1",
        execution: { provider: "codex", model: null, modeId: null, thinking: null },
        builtinChannelId: crypto.randomUUID(),
        state: { kind: "paused", reason: "directory_missing" },
        createdAt: "2026-11-05T00:00:00Z",
      },
    },
  });
  expect(paused.type).toBe("bot.pause.response");
});
```

`connection.test.ts`：把 `h.ready(undefined, ["workspace-bots-v1"])` 用例改为 v2 时 `workspaceBots: true`，并新增只有 `workspace-bots-v1` 时 `workspaceBots: false`。

- [ ] **Step 2: 运行确认失败**

Run: `npm run test --workspace=@ait/protocol -- src/bot && npm run test --workspace=@ait/client -- src/protocol/connection.test.ts`
Expected: FAIL（schema 仍为 v1）。

- [ ] **Step 3: 实现** schema、类型与门控；运行 `npm run generate:validators --workspace=@ait/protocol` 并提交生成文件。

- [ ] **Step 4: 运行确认通过**

Run: 同 Step 2，加 `npm run typecheck --workspace=@ait/protocol && npm run typecheck --workspace=@ait/client`
Expected: PASS。`apps/mobile` 此时类型检查会失败（Task 2–6 修复），不在本任务验收范围。

- [ ] **Step 5: Commit** — `feat(protocol): Bot 协议改为 workspace-bots-v2`

---

### Task 2: 单例 Bot 标签与查询层

**Files:**

- Modify: `apps/mobile/src/workspace-tabs/model.ts:50`、`apps/mobile/src/workspace-tabs/identity.ts:62-68`、`apps/mobile/src/stores/workspace-layout-storage.ts:30-34`、`apps/mobile/src/panels/panel-manifest.ts:117-125`、`apps/mobile/src/workspace-tabs/launcher/index.tsx:77`
- Modify: `apps/mobile/src/bots/queries.ts`
- Test: `apps/mobile/src/workspace-tabs/identity.test.ts`、`apps/mobile/src/stores/workspace-layout-storage.test.ts`（均为现有文件，追加用例）、`apps/mobile/src/bots/queries.test.ts`

**Interfaces:**

- Produces（`workspace-tabs/model.ts`）：`{ kind: "bot"; view?: "settings" }`。
- Produces（`bots/queries.ts`）：

```ts
export const botKeys = {
  bot: (serverId: string, workspaceId: string) => ["bot", serverId, workspaceId],
  conversation: (serverId: string, workspaceId: string) => [
    "bot-conversation",
    serverId,
    workspaceId,
  ],
  history: (serverId: string, workspaceId: string, conversationId?: string) => [
    "bot-history",
    serverId,
    workspaceId,
    conversationId,
  ],
  memory: (serverId: string, workspaceId: string) => ["bot-memory", serverId, workspaceId],
  channels: (serverId: string, workspaceId: string) => ["bot-channels", serverId, workspaceId],
};
export function useWorkspaceBot(
  serverId: string,
  workspaceId: string,
  enabled: boolean,
): UseQueryResult<BotView | null>; // bot.get；refetchInterval 10_000，bot 为 null 时 5_000
export function fetchBotHistory(
  client: Pick<DaemonClient, "botRequest">,
  workspaceId: string,
  conversationId: string,
  cached?: BotTurn[],
): Promise<BotTurn[]>;
export function useBotHistory(
  serverId: string,
  workspaceId: string,
  conversationId: string | undefined,
  enabled: boolean,
): UseQueryResult<BotTurn[]>;
```

`isBotTurnPending` 不变；删除 `useBotDetail`。

- [ ] **Step 1: 写失败测试**

```ts
it("legacy_bot_targets_normalize_to_singleton", () => {
  expect(normalizeWorkspaceTabTarget({ kind: "bot", botId: "b1", view: "list" })).toEqual({
    kind: "bot",
  });
  expect(normalizeWorkspaceTabTarget({ kind: "bot", view: "create" })).toEqual({ kind: "bot" });
  expect(normalizeWorkspaceTabTarget({ kind: "bot", botId: "b1", view: "settings" })).toEqual({
    kind: "bot",
    view: "settings",
  });
  expect(getPanelManifest("bot").singleton).toBe(true);
  expect(panelResourceKey({ kind: "bot", view: "settings" })).toBe("bot");
});
```

（`normalizeWorkspaceTabTarget` 来自 `workspace-tabs/identity.ts`，接受持久化的原始值；若其参数类型不接受旧字段，测试以 `as never` 传入旧形状。）存储测试：含旧 `{kind:"bot",botId:"b1",view:"list"}` 的布局能通过 schema 解析。
`queries.test.ts`：把现有 `fetchBotHistory` 用例改为以 `workspaceId` 调用，并断言每个 `botRequest` 参数都含 `workspaceId` 且不含 `botId`。

- [ ] **Step 2: 运行确认失败**

Run: `cd apps/mobile && npx vitest run src/workspace-tabs src/stores/workspace-layout-storage src/bots/queries`
Expected: FAIL。

- [ ] **Step 3: 实现**：存储 schema 接受旧字段（`botId` 可选、`view` 枚举含 `list`/`create`/`settings`）后由 identity 归一化；manifest 改为单例；launcher 目标改为 `{ kind: "bot" }`；查询层按上面接口改写。若同一 Workspace 恢复出两个 Bot 标签，沿用现有单例去重逻辑（按 `resourceKey` 合并）。

- [ ] **Step 4: 运行确认通过**

Run: 同 Step 2。Expected: PASS。

- [ ] **Step 5: Commit** — `feat(mobile): Bot 标签改为每个 Workspace 单例并迁移旧标签`

---

### Task 3: 标签外壳与设置向导

**Files:**

- Modify: `apps/mobile/src/bots/workspace-bot-tab.tsx`（删除 `BotList`、`botId` 属性与 `onCreate`／`onOpen`／`onBrowse`）
- Create: `apps/mobile/src/bots/bot-setup.tsx`（由 `bot-form.tsx` 改写：去掉名称与指令，保留 `BotSelect` 与执行配置选择）
- Delete: `apps/mobile/src/bots/bot-form.tsx`
- Modify: `apps/mobile/src/panels/bot-panel.tsx`（新属性、descriptor 标签固定为 `t("bots.title")`）
- Modify: `apps/mobile/src/i18n/resources/en.ts`、`zh-CN.ts`
- Test: `apps/mobile/src/bots/workspace-bot-tab.test.tsx`、`apps/mobile/src/bots/bot-setup.test.tsx`

**Interfaces:**

- Consumes: Task 2 `useWorkspaceBot`、`botKeys`。
- Produces:

```ts
export interface WorkspaceBotTabProps {
  serverId: string;
  workspaceId: string;
  cwd: string;
  view?: "settings";
  interactive: boolean;
  onChat: () => void;
  onSettings: () => void;
}
export function BotSetup(props: {
  serverId: string;
  workspaceId: string;
  cwd: string;
  interactive: boolean;
  onConfigured: (bot: BotView) => void;
}): JSX.Element;
```

- 外壳状态：加载中 → 加载动画；`bot === null` → 只读用户显示 `bots.notConfigured`，可管理用户显示 `BotSetup`；`bot` 存在 → `view === "settings"` 显示设置（Task 5），否则显示对话（Task 4）；工具栏有 `bots.chat`／`bots.settings` 切换。
- `BotSetup` 字段：Provider、Model、Permission mode、Reasoning（沿用现有选择逻辑与 `useProvidersSnapshot`）、“清空已保存的记忆”勾选框（默认不勾选）；提交 `bot.setup.request`，成功后 `queryClient.setQueryData(botKeys.bot(...), bot)` 并调用 `onConfigured`。
- 文案：新增 `bots.setupTitle`、`bots.setupHint`、`bots.setup`（按钮）、`bots.clearMemory`、`bots.notConfigured`；删除 `bots.browse`、`bots.new`、`bots.create`、`bots.name`、`bots.instructions`、`bots.instructionsHint`、`bots.empty`、`bots.archive`、`bots.archiveConfirm`、`bots.archived`。英文与中文文案由实现者撰写，含义：设置向导标题“Set up the workspace Bot”／“设置 Workspace Bot”，提示说明 Bot 是该 Workspace 的管理者、关闭标签后仍在运行。

- [ ] **Step 1: 写失败测试**

```tsx
it("shows the setup wizard when the workspace has no bot", () => {
  mockWorkspaceBot({ data: null });
  renderTab();
  expect(screen.getByTestId("bot-setup")).toBeTruthy();
});
it("shows a read-only hint instead of the wizard for guests", () => {
  mockWorkspaceBot({ data: null });
  mockCanManage(false);
  renderTab();
  expect(screen.queryByTestId("bot-setup")).toBeNull();
  expect(screen.getByText("bots.notConfigured")).toBeTruthy();
});
it("wizard_switches_to_chat_when_bot_appears", () => {
  mockWorkspaceBot({ data: null });
  const { rerender } = renderTab();
  mockWorkspaceBot({ data: activeBot });
  rerender(tabElement());
  expect(screen.getByTestId("bot-history")).toBeTruthy();
});
it("setup_sends_clear_memory_only_when_checked", async () => {
  // 渲染 BotSetup，选择 provider 后提交：请求参数不含 clearMemory 或为 false；
  // 勾选 bots.clearMemory 后再次提交：clearMemory === true；两次参数都含 workspaceId、不含 botId。
});
```

（`mockWorkspaceBot`、`mockCanManage`、`renderTab`、`tabElement`、`activeBot` 为测试文件内的辅助，沿用现有 `vi.mock` 写法。）

- [ ] **Step 2: 运行确认失败** — `cd apps/mobile && npx vitest run src/bots` → FAIL。

- [ ] **Step 3: 实现** 外壳、向导、panel 接线与文案。

- [ ] **Step 4: 运行确认通过** — 同 Step 2 → PASS。

- [ ] **Step 5: Commit** — `feat(mobile): Bot 标签外壳与设置向导`

---

### Task 4: 对话视图 v2

**Files:**

- Modify: `apps/mobile/src/bots/bot-chat.tsx`
- Modify: `apps/mobile/src/i18n/resources/en.ts`、`zh-CN.ts`
- Test: `apps/mobile/src/bots/bot-chat.test.tsx`

**Interfaces:**

- Consumes: Task 2 `useBotHistory`、`botKeys`；`BotView`。
- Produces: `export function BotChat(props: { serverId: string; workspaceId: string; bot: BotView; interactive: boolean }): JSX.Element;`

行为：

- 所有请求带 `workspaceId`；可管理用户 `bot.conversation.open`，只读用户 `bot.conversation.list` 取 builtin Channel 的对话。
- `state.kind === "paused"`：输入框禁用；`reason === "user"` 显示 `bots.paused`，`reason === "directory_missing"` 显示 `bots.directoryMissing`；可管理用户显示 `bots.resume` 按钮，调用 `bot.resume.request`，成功后更新 `botKeys.bot` 缓存；失败显示错误，状态不变。
- 回合 `status === "cancelled"` 且有 `error` 时显示该文本（目录缺失取消原因）。
- 头部不再显示 Bot 名称。
- 文案：新增 `bots.resume`、`bots.directoryMissing`。

- [ ] **Step 1: 写失败测试**

```tsx
it("paused_bot_disables_composer_and_offers_resume", async () => {
  renderChat({ bot: pausedBot("user") });
  expect(screen.getByTestId("bot-message-input")).toHaveProperty("disabled", true);
  fireEvent.click(screen.getByText("bots.resume"));
  await waitFor(() =>
    expect(botRequest).toHaveBeenCalledWith("bot.resume.request", { workspaceId: "workspace" }),
  );
});
it("explains a missing directory and keeps the bot paused when resume fails", async () => {
  botRequest.mockRejectedValueOnce(new Error("WorkspaceNotFound"));
  renderChat({ bot: pausedBot("directory_missing") });
  expect(screen.getByText("bots.directoryMissing")).toBeTruthy();
  fireEvent.click(screen.getByText("bots.resume"));
  expect(await screen.findByText("WorkspaceNotFound")).toBeTruthy();
});
it("sends messages with workspaceId and shows cancellation reasons", async () => {
  // 发送一条消息：bot.message.send.request 参数含 workspaceId、conversationId、messageId、text，不含 botId；
  // 历史中 cancelled 且 error 为 "Workspace directory missing; Bot paused" 的回合显示该文本。
});
```

（`testID` 以组件现有值为准；`renderChat`、`pausedBot`、`botRequest` 为测试内辅助。）

- [ ] **Step 2: 运行确认失败** — `cd apps/mobile && npx vitest run src/bots/bot-chat` → FAIL。
- [ ] **Step 3: 实现**。
- [ ] **Step 4: 运行确认通过** — 同 Step 2 → PASS。
- [ ] **Step 5: Commit** — `feat(mobile): Bot 对话视图支持暂停、恢复与取消原因`

---

### Task 5: 设置页：模型、暂停、记忆、移除

**Files:**

- Modify: `apps/mobile/src/bots/bot-settings.tsx`
- Modify: `apps/mobile/src/i18n/resources/en.ts`、`zh-CN.ts`
- Test: `apps/mobile/src/bots/bot-settings.test.tsx`（新建）

**Interfaces:**

- Consumes: Task 2 `botKeys`；Task 3 抽出的执行配置选择组件（若 Task 3 将其放在 `bot-setup.tsx`，在此导出复用，不复制）。
- Produces: `export function BotSettings(props: { serverId: string; workspaceId: string; cwd: string; bot: BotView; interactive: boolean; onRemoved: () => void }): JSX.Element;`

分区：

1. **模型**：执行配置，保存调用 `bot.update.request`，显示 `bots.configurationHint`（新对话生效）。
2. **状态**：`active` 显示 `bots.pause`，`paused` 显示 `bots.resume`；分别调用 `bot.pause.request`／`bot.resume.request` 并更新缓存。
3. **记忆**：沿用 revision 编辑器，请求改为 `workspaceId`；`memoryHint` 改为“Bot 每次回复前读取的偏好，例如 PR 标题语言、回复风格”。
4. **Channel**：放置 Task 6 的 `<BotChannels>`（本任务留占位不渲染，Task 6 接入）。
5. **移除**：`bots.remove` 按钮，确认文案 `bots.removeConfirm`（说明对话历史将不再可见、记忆保留、Telegram／飞书连接会断开），调用 `bot.remove.request`，成功后 `setQueryData(botKeys.bot(...), null)` 并 `onRemoved()`。

只读用户：所有编辑控件禁用、不显示暂停与移除。文案：新增 `bots.pause`、`bots.remove`、`bots.removeConfirm`；更新 `bots.memoryHint`、`bots.configurationHint`（去掉“Instructions”）。

- [ ] **Step 1: 写失败测试**

```tsx
it("updates execution with workspaceId", async () => {
  /* 改模型并保存 → bot.update.request {workspaceId, execution} */
});
it("pauses an active bot and resumes a paused bot", async () => {
  /* 按钮文案与请求方法随 state 切换 */
});
it("remove_returns_to_wizard", async () => {
  // 点击 bots.remove 并确认 → bot.remove.request {workspaceId}；
  // queryClient 中 botKeys.bot 变为 null；onRemoved 被调用一次。
});
it("hides pause and remove for read-only users", () => {
  /* canManage=false → 不渲染两个按钮 */
});
```

- [ ] **Step 2: 运行确认失败** — `cd apps/mobile && npx vitest run src/bots/bot-settings` → FAIL。
- [ ] **Step 3: 实现**，并在外壳中把 `onRemoved` 接到 `onChat`（回到默认视图，此时 `bot` 为 null 即显示向导）。
- [ ] **Step 4: 运行确认通过** — 同 Step 2 → PASS。
- [ ] **Step 5: Commit** — `feat(mobile): Bot 设置页支持模型、暂停、记忆与移除`

---

### Task 6: 外部 Channel 管理

**Files:**

- Create: `apps/mobile/src/bots/bot-channels.tsx`
- Test: `apps/mobile/src/bots/bot-channels.test.tsx`
- Modify: `apps/mobile/src/bots/bot-settings.tsx`（接入）、`apps/mobile/src/i18n/resources/en.ts`、`zh-CN.ts`

**Interfaces:**

- Consumes: Task 2 `botKeys.channels`。
- Produces: `export function BotChannels(props: { serverId: string; workspaceId: string; interactive: boolean; readOnly: boolean }): JSX.Element;`

行为：

- 列表：`bot.channel.list.request`，过滤 `config.kind === "builtin"` 与 `deleted`；每项显示类型、启用状态与 `bot.channel.status.request` 的连接状态（`connection` 原样以文本摘要显示：存在 `error`/`state` 字段时显示其值，否则显示 `bots.channelConnected`／`bots.channelIdle`）。
- 新建：选择类型（`bot.channel.types.request` 的 `supported` 去掉 `builtin`）→ Telegram 字段 `tokenRef`、`allowedSenders`（逗号或换行分隔）、`allowGroups`；飞书字段 `appId`、`secretRef`、`allowedSenders`、`allowGroups`。凭据引用必须匹配 `/^env:AIT_SERVER_CREDENTIAL_[A-Z0-9_]{1,96}$/`，允许列表非空且不超过 256 项，否则禁用提交并显示 `bots.channelRefHint`。新建时 `channelId = randomUUID()`、`enabled: false`（服务端约定新 Channel 默认停用），列表中提供启用开关。
- 启用／停用：`bot.channel.update.request {workspaceId, channelId, enabled}`。
- 删除：确认后 `bot.channel.delete.request`。
- 文案：新增 `bots.channels`、`bots.addChannel`、`bots.channelType`、`bots.telegram`、`bots.feishu`、`bots.tokenRef`、`bots.secretRef`、`bots.appId`、`bots.allowedSenders`、`bots.allowGroups`、`bots.channelRefHint`、`bots.channelEnabled`、`bots.channelConnected`、`bots.channelIdle`、`bots.deleteChannel`、`bots.deleteChannelConfirm`。`channelRefHint` 说明凭据由启动 daemon 的环境变量提供，此处只填变量引用。

- [ ] **Step 1: 写失败测试**

```tsx
it("channel_form_requires_env_reference_and_senders", () => {
  renderChannels();
  fireEvent.click(screen.getByText("bots.addChannel"));
  typeInto("bots.tokenRef", "123456:plain-token");
  typeInto("bots.allowedSenders", "42");
  expect(submitButton()).toHaveProperty("disabled", true);
  typeInto("bots.tokenRef", "env:AIT_SERVER_CREDENTIAL_TELEGRAM");
  expect(submitButton()).toHaveProperty("disabled", false);
  typeInto("bots.allowedSenders", "");
  expect(submitButton()).toHaveProperty("disabled", true);
});
it("creates a disabled telegram channel with workspaceId", async () => {
  // 提交后 bot.channel.create.request 参数：workspaceId、channelId(uuid)、enabled:false、
  // config {kind:"telegram", tokenRef, allowedSenders:["42","43"], allowGroups:false}
});
it("lists only external channels and toggles enabled", async () => {
  /* builtin 与 deleted 不显示；开关 → channel.update */
});
it("is read-only for guests", () => {
  /* readOnly → 无新建、开关禁用、无删除 */
});
```

- [ ] **Step 2: 运行确认失败** — `cd apps/mobile && npx vitest run src/bots/bot-channels` → FAIL。
- [ ] **Step 3: 实现** 并接入设置页。
- [ ] **Step 4: 运行确认通过** — `cd apps/mobile && npx vitest run src/bots && npm run typecheck`（mobile 类型检查此时应全部通过）→ PASS。
- [ ] **Step 5: Commit** — `feat(mobile): Bot 设置页管理 Telegram 与飞书 Channel`

---

### Task 7: 浏览器端到端测试

**Files:**

- Modify: `apps/mobile/e2e/browser/workspace-bots.spec.ts`（取消 `test.skip`，按 v2 重写）

**Interfaces:**

- Consumes: Task 1–6 的界面 `testID` 与 v2 RPC。

- [ ] **Step 1: 重写两个用例**（沿用该文件现有的隔离 daemon 与离线 Codex 夹具）：
  1. `workspace bot setup, conversation, settings and persistence`：开启实验开关 → 打开 Bot 标签 → 向导提交 → 发送消息并看到离线 Codex 回复 → 设置页保存记忆 → 刷新页面后标签恢复为同一 Bot 且对话仍在 → 暂停后输入框禁用、恢复后可发送 → 移除后回到向导，再次设置后记忆仍在。
  2. `bot denies approvals automatically and protects memory from stale edits`：保留原用例意图，改用 `bot.setup` 与 `workspaceId`。
- [ ] **Step 2: 运行**

Run: `cd apps/mobile && npm run test:e2e -- e2e/browser/workspace-bots.spec.ts`
Expected: 2 passed。若本机缺少 e2e 运行条件（daemon 构建或浏览器），在报告中写明缺失项与已执行的替代检查，不得标记为通过。

- [ ] **Step 3: Commit** — `test(mobile): 恢复 Workspace Bot v2 浏览器端到端测试`

---

### Task 8: 文档

**Files:**

- Create: `docs/decisions/clients/adr-214-workspace-bot-tab-v2.md`
- Modify: `docs/decisions/clients/adr-141-experimental-workspace-bots.md`（关系行）、`docs/decisions/README.md`、`docs/operations/bots.md`、`docs/plans/README.md`、`docs/plans/workspace-bot-manager.md`（状态行）、`docs/decisions/bot/adr-213-single-workspace-manager-bot.md`（状态行与“移动端仍要求 v1”的后果说明）、`CHANGELOG.md`

- [ ] **Step 1: 写 ADR-214**：状态“已实现”，关系“修订 ADR-141 的标签身份与界面范围，延续 ADR-213”。决策：v2 能力门控、单例标签与旧标签归一化规则、向导／对话／设置／Channel 范围、只读访客规则；后果：阶段 3 向导增加权限与启动默认值、阶段 4 增加通知展示。
- [ ] **Step 2: 更新其余文档**：ADR-213 与设计稿状态改为“阶段 1 已发布（0.1.30）、阶段 2 已实现”，删除“待合并”字样与“移动端标签仍要求 v1”的过渡描述；`operations/bots.md` 增加“在 Ait 中使用”小节（打开标签 → 设置 → 对话 → 设置页 Channel）；计划索引与 CHANGELOG（客户端可用、旧标签自动迁移）。
- [ ] **Step 3: 检查**

Run: `npm run check:docs && git diff --check`，并对每个改动文件 `npx oxfmt --check <file>`。
Expected: 全部通过。

- [ ] **Step 4: Commit** — `docs(bot): ADR-214 客户端 Bot 标签 v2`
