---
id: ADR-303
status: accepted
date: 2026-11-24
amends:
  - providers/adr-248-omp-paseo-native-rpc.md
paths:
  - crates/provider/native
  - crates/provider/codex
  - crates/provider/omp
  - apps/mobile/src/composer
  - apps/mobile/src/panels
---

# ADR-303：原生 Goal 状态投影到共享胶囊轨道

修订说明（2026-11-26）：OMP 的原生 Goal RPC 可以不出现在补全目录；以活动进程的结构化
Goal 状态确认支持，并在完整目录无名称声明时提供宿主入口，仍保留冲突与扩展归属保护。
详见 [Provider 规格](../../architecture/provider.md#原生-goal)。

## 背景

原生 Goal 命令和通知已有适配，但当前目标没有独立状态投影，客户端无法在变更统计旁展示
可靠的 Goal 胶囊。时间线里的说明文字不能作为当前目标或操作权限的依据。

## 决策

- 修订 ADR-248 的带外命令范围：活动会话确认原生内置归属后，Goal 操作也走原生 RPC，
  不转为推理输入；会话启动的私有配置同时启用 RPC Goal 驱动。
- adapter 将原生读取结果、操作确认和通知投影到现有 `runtimeInfo.extra.goal`，通过现有
  `RuntimeInfo` 事件发布和持久化，不新增 Goal 数据库或从文字推断目标。缺省不承诺支持，
  `null` 表示确认清除；有效对象含 `objective`、`status`、`actions` 及原生提供的可选用量。
- `actions` 只声明 adapter 能执行的 `edit`、`pause`、`resume`、`clear`。状态归一为
  active、paused、completed、budgetLimited、blocked、usageLimited、failed、unknown。非法和不完整的通知不清除旧状态。
- 客户端不按 Provider 名称分支：有效目标在 composer track 的变更统计旁显示胶囊，点开
  同一 popover/sheet 卡片；操作复用已受原生归属裁决的 `/goal` 带外命令，保留输入草稿。
  状态只随原生确认更新，失败不乐观清除或改写目标。
- Codex 支持读取、原地编辑及生命周期操作，显式宿主操作携带 user provenance。
  OMP 的 RPC 当前提供 get/create/pause/resume/drop，不支持原地编辑，卡片不提供编辑按钮，
  不用 drop/create 冒充编辑。会话私有 `--config` 将 continuationModes 设为 rpc，使原生
  Goal 驱动可续跑，不改全局配置或强制开启被用户禁用的 goal.enabled。

## 后果

支持原生 Goal 的新 adapter 只需提供相同投影和带外操作，同一胶囊即可显示。
旧宿主不发送快照时客户端不显示胶囊；读取失败保留已知状态，恢复会话时重新读取原生目标。
