# ADR-129：Agent Skills 能力组归 provider

- 状态：接受
- 日期：2026-10-11
- 修订：[ADR-043](../workspace/adr-043-daemon-skills.md) 的 Skills 能力归属、
  [ADR-113](../workspace/adr-113-filesystem-capability-groups.md) 的 filesystem 能力组集合，
  以及 [ADR-100](../daemon/adr-100-crate-level-service-installation.md) 的服务组成。

## 背景

`agent.*` 中只有五个 `agent.skills.*` 请求属于 filesystem，其余属于 provider。
Skills 子树独立拥有安装协议、业务协调、存储端口、本地文件适配器及事务恢复，
不依赖 filesystem 的其他能力组。将其整组归入 provider 可以统一 Agent 能力归属，
同时保留原有分层和安装边界，无需引入功能 crate 之间的依赖。

## 决策

1. `crates/filesystem/src/skills.rs` 与 `skills/` 子树整体迁到 `provider::skills`。
   保留 `protocol`、`ports`、`service`、`local` 分层和所有测试，不保留旧路径转发。
   filesystem 只保留 files、forge、git、worktrees 四组，依赖守卫继续禁止
   provider 与 filesystem 互相依赖。
2. 五个方法的声明和 dispatch 归 provider。`provider::Dependencies` 必含 `Skills`，
   filesystem 的完整服务不再包含它；API 从 provider 服务取出独立的
   `Arc<Mutex<Skills>>`，通过公共 Runtime 的阻塞任务预算执行，
   不并入原生 Agent 执行器或会话锁。
3. 仍按 crate 整体安装：provider 已安装时声明 Skills 方法，未安装时不声明。
   正常 daemon 同时安装 provider 和 filesystem，方法集合不变；仅安装 filesystem
   的精简 host 不再提供 Skills，安装 provider 的 host 获得这五个方法。
   原生 CLI 安装、登录或会话状态不作为 Skills 请求的额外条件。
4. 方法名、方向、参数、响应和错误码保持不变。Skills 仍是主机级安装能力，
   不接受 `agentId`，不与 `agent.commands.list.request` 的原生命令发现合并。
   daemon 继续使用原有环境变量、bundle 路径、三个安装目标和 `skills-state` 目录。
   selection、所有权标记、暂存路径、事务日志格式、每次请求前恢复及删除确认均保持原样。
5. 方法归属与调度分类分开：单连接先验证方法来自 provider 声明，再将
   `agent.skills.*` 继续送入原来的文件操作队列；其他 provider 方法仍进入原生请求队列。
   未声明的 Skills 名称不因前缀匹配而获得该队列。队列容量与保序规则保持
   [ADR-120](../daemon/adr-120-single-connection-startup-admission.md) 的行为。

## 后果与验证

provider 承担主机技能安装，但无需新依赖或数据迁移；所有 `agent.*` 方法只由 provider 声明。
外部 Rust 构造路径和完整服务参数发生变化，daemon、API 和测试夹具同步调整。

验证覆盖迁入的 Skills 单元与事务恢复测试、方法唯一性和安装集合、独立于 filesystem
及原生执行器的五个方法分发、单连接调度、文件操作夹具和能力边界守卫。
真实 daemon 进程测试在普通与单连接模式下验证安装、删除确认、重启后的选择恢复和卸载。
