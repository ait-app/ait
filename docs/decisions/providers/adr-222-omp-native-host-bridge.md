# ADR-222：OMP 私有原生宿主桥接运行时

状态：接受。日期：2026-11-07。修订 [ADR-218](adr-218-native-ui-and-actions.md) 中
「未导出的任意 `custom()`、组件工厂、header/footer 和主题编辑不引入私有运行时补丁」的
适用范围，并扩展 [ADR-180](adr-180-omp-native-rpc-provider.md)、
[ADR-190](adr-190-omp-acp-settlement.md) 与 [ADR-221](adr-221-omp-headless-commands-and-side-questions.md)。

## 背景

OMP 的 Vibe 模式（多 fast/good 原生 worker 的持久编排）只存在于官方 TUI 与官方工具的
原生注册表中，RPC 模式不导出进入、退出、worker 生命周期控制，也不导出 worker 观测。
`/vibe` 在纯 RPC 下只能作为被屏蔽的 TUI-only 命令返回错误卡片。

ADR-218 排除的是**替换或伪造官方界面**的私有补丁。本决策只针对一类不同的需求：
官方 SDK 的 RPC 路径缺少一个能力入口，而该入口所需的全部原语（`vibe_*` 工具、
`VibeSessionRegistry`、`AgentRegistry`、session manager）都已在官方包内公开导出。
因此补丁的边界是「在 RPC 入口挂接官方已存在的原语」，不是重写 OMP。

同时必须解决一个真实风险：如果私有运行时使用自己的数据目录，`/vibe` 会变成一个影子
系统——Ait 会话跑 18.14 官方二进制，Vibe 跑另一套存储。因此本决策的核心约束不是补丁
本身，而是**单 writer / 同目录归属**。

## 决策

### 1. 只在 RPC 入口挂接，不复制业务逻辑

私有运行时由 `runtimes/omp-native-host/` 构建：

- `vibe-controller.ts` / `vibe-observer.ts` 是薄胶水，只调用官方 `vibe_*` 工具、
  `VibeSessionRegistry` 与只读 `AgentRegistry`；不含 provider、权限、持久化或执行逻辑。
- `rpc-patch.mjs` 对已安装的官方 SDK 做**锚定、哈希校验、幂等**的源码补丁：
  补齐 hello 帧模块、在 `--mode rpc-ui` 的 `ready` 帧之前输出
  `ait_native_host_hello`、把 controller 接入命令目录、prompt 输入、会话变更拦截与关闭。
- 补丁前校验 `rpc-mode.ts` 与 `main.ts` 的 SHA-256 必须等于已发布 18.17.0 tarball；
  任一不匹配即报错中止，绝不静默跳过或部分应用。重复执行为 no-op。
- `install.mjs` 校验 lockfile 完整性、SDK/Bun 版本、Bun 二进制签名，并只通过一次
  rename 发布，失败回滚。

### 2. 能力握手

hello 帧是唯一的启用信号：

```json
{
  "type": "ait_native_host_hello",
  "bridgeVersion": 1,
  "sdkVersion": "18.17.0",
  "capabilities": { "vibe": true, "vibeWorkerObservation": true, "vibeWorkerControl": true }
}
```

Rust 适配器只在 `bridgeVersion == 1` 且 `capabilities.vibe == true` 时认为 bridge
可用。即便如此，`/vibe` 仍需原生命令目录里存在 `source == "builtin"` 的 `vibe` 条目
才放行——hello 只证明运行时有补丁，builtin 目录条目才证明命令表真的挂上了（普通
`--mode rpc` 启动不广播该条目）。任一条件不满足时 `/vibe` 保持阻塞并返回说明缺少
bridge 的独立卡片（不送模型），绝不当成 prompt 发给模型。
真实扩展/技能/文件命令同名时仍归其原生 owner。hello 是连接内协商，不写历史；
metadata 探针也会收到 hello，但探针不构造 Session 级 Vibe 状态。

### 3. 命令表挂接

私有运行时在 `--mode rpc-ui` 下构造一个 `AitNativeHostBridge`：

- `ready` 之后调用 `initialize()`（rehydrate 持久 scope 与 worker 名册）；
- 命令目录合并 `/vibe` 条目后，同一份合并目录才是输入派发查找所用，
  否则 `/vibe` 永远匹配不到 builtin；
- prompt 派发前调用 `controller.input(text, commands)`：返回字符串表示把该字符串作为
  inline directive 继续走官方 prompt 路径（形成真实 turn），返回 `null` 表示仅切换模式；
- `handleCommand` 先过 `assertRequest`，Vibe 模式下的会话变更命令被拒绝；
- `ait_vibe_control`（list/spawn/send/wait/kill/toggle）走同一条命令通道，
  worker 引用由 observer 从 Ait subagent ID 解析为 native id；
- 进程关闭时 `close()` 停止观察定时器。
  仅 `rpc-ui` 且带 hello 时才构造桥；普通 `--mode rpc` 完全没有 Vibe 状态。

### 4. 单 writer 与同目录归属

- 同一 OMP 会话永远只有一个写入进程。要么官方二进制，要么私有运行时，不并存。
- 私有运行时**不改变**数据目录：沿用同一 `HOME`、`PI_CONFIG_DIR`/XDG 解析结果
  （`sandbox::OMP_STATE`）与 `OmpClient` 的 environment 注入，因此
  `~/.omp/agent`（`agent.db`、`sessions/`、`config.yml`、认证）与官方进程完全相同。
- 私有运行时必须通过 manifest 校验才被采用（见下）；未通过则回退官方二进制，
  并声明 bridge 能力不可用。不存在“装了但用不了还硬启动”的状态。
- 运行时的启动形态只是替换可执行文件：`bun <sdk>/src/cli.ts --mode rpc-ui --cwd …`，
  其余 argv、环境变量、cwd、进程组与沙箱策略不变。

### 5. 版本边界与共存

实测事实（2026-11-07，官方 18.17.0 tarball）：

- 既有 165 个 native journal 的 header 记录 `runtime.version` 分别为 18.12.6（75）、18.14.0（75）
  与 18.17.0（15）；官方 18.17 能解析全部旧 journal（列表与恢复都成功）。
- 但**恢复旧会话会追加写入**：`--resume <旧 journal>` 立即追加 `session_exit` 与相关记录。
  也就是说，任何 18.17 进程（官方或私有运行时）一旦恢复 18.14 会话，该文件就归 18.17 写入。
  这不是补丁引入的问题，而是官方“恢复即接管”的语义。
- 因此版本边界只能靠**单 writer**保证：同一 journal 在任一时刻只由一版进程写入。
  Ait 侧现有的 `--resume` 已满足该约束（会话由当前 adapter 独占）；不得并行启动第二版进程读同一 journal。
- 回退可用性已实测（2026-11-07）：取 18.12.6 时期的 journal，追加一条
  `customType: "session_exit"` / `runtime.version: "18.17.0"` 记录后，用**官方 18.14.0**
  （`@oh-my-pi/pi-coding-agent@18.14.0` 源码 + bun 启动）执行
  `--mode rpc-ui --resume <journal>`：
  - 进程正常启动并输出合法 `ready` 帧，stderr 为空；
  - `get_state` 返回 `success:true` 且带恢复后的 model/sessionId；
  - `get_entries`（完整历史读取）返回 `success:true`。
    即 18.14 **可以**读取被 18.17 追加过的 journal，降级回退不会导致会话不可读。
    唯一保留：18.14 不认识 18.17 新增记录的语义（它把 `session_exit` 当普通 custom 记录），
    因此回退后这些条目不参与行为，只作为历史存在。
- 降级前置条件（2026-11-07 实测，对 18.14 与 18.17 一致）：`--resume` 要求该 journal
  引用的模型仍在配置中。缺少对应的 `models.yml`/`config.yml`（例如隔离环境或用户已删除
  该 provider）时，恢复直接失败于
  `SessionModelRestoreError: Could not restore model <provider>/<model>`，**与版本无关**。
  回退二进制时若遇到该错误，应先恢复模型定义，而不是判定为 journal 格式不兼容。

- 私有运行时锁定**一个**官方 SDK 版本（当前 18.17.0）与一个 Bun 版本（1.4.4）。
  `manifest.json` 记录 `schemaVersion`、`adapter`、`adapterVersion`、`ompVersion`、
  `bunVersion`、`bunPath`、`entrypoint`；适配器逐字段校验后才启动。
- 磁盘上的 journal 格式由官方 SDK 决定。18.17 私有运行时读写的仍是同一
  `sessions/` 目录与同一 `agent.db`；Ait 不建立第二套存储、不复制、不双写。
- 版本升级只通过替换整个安装目录（新 staging → rename）完成；旧目录由
  `publishInstallation` 备份后清理。Ait 不就地改动已安装运行时。
- 若未来官方 SDK 变更使既有 journal 不兼容，处置顺序为：先拒绝采用该运行时
  （manifest 版本不匹配 → 回退官方二进制），再给出迁移方案；**不得**让两个版本
  同时对同一目录写入。

### 6. 未安装时的降级

未安装、manifest 损坏、哈希不符、SDK 未打补丁、路径逃逸或体积超限时：

- 适配器继续使用官方二进制，OMP 其余功能不受影响；
- 不声明 `vibe` 能力，`providerUi.vibe` 不出现，`/vibe` 返回明确错误卡片；
- 记录一次 warn 级诊断（不含凭据），不做静默降级以外的任何隐式操作。

### 7. 许可证与升级策略

- 私有运行时随附官方 MIT 许可证、Bun 许可证与依赖清单（`install.mjs` 的 notices
  阶段生成 `NOTICE.txt`、`dependencies.json`、`licenses/`）。
- 补丁只修改已安装 SDK 的源码副本，且每次安装可复现；不重新分发修改后的 SDK 包。
- 升级官方 SDK 版本需要同步更新 `EXPECTED_SHA256`、`COPIES` 与锚定编辑，并重新跑
  `node --test runtimes/omp-native-host/install.test.mjs` 与真实 rpc-ui 冒烟。

## 后果

- `/vibe` 从「可宣布但不可执行」变为可执行，且 worker 观测与控制复用官方工具权限。
- Ait 只新增一个能力面（hello → `providerUi.vibe` → 子 Agent 事件），protocol 侧
  `vibe` 字段可选，旧客户端与旧快照保持兼容。
- worker 只以可发布状态进入共享子 Agent 目录：共享 wire 状态枚举没有 `unknown`，
  未定状态的 worker 仍显示在 `providerUi.vibe.workers`，但不作为子 Agent 发布，
  也不得让父会话的 turn 因校验失败而中断。
- 代价是引入了对官方 SDK 源码的构建期补丁，必须随 SDK 版本维护；这是 ADR-218 的
  明确修订例外，范围仅限 RPC 入口挂接官方已有原语，不扩展到 UI 替换。
- 探针回归已实测（2026-11-07）：把打补丁的运行时包装成单文件启动器后作为
  `AIT_TEST_OMP_BIN`，跑 `local::omp::tests::installed` 全套（14 项，`--ignored`）：
  10 passed / 4 failed。同样的 4 项在**未打补丁的系统 omp 18.17** 上同样失败
  （completion 2 项需要 `AIT_TEST_OMP_COMPLETION_BIN` 指向带该能力的构建；
  settlement 的 proxy 注入与 tiers 的特性计数在 18.17 上已漂移），
  因此这 4 项是既有基线问题，不是“所有 OMP 进程改走 host”引入的回归。
  角色、preset、目录发现、插件命令、动作、交互、结构化提问全部通过，
  且启动器日志证明 metadata 探针（`config list --json`、`models --kind all --json`、
  `--mode rpc-ui --no-session`）确实经由 host 启动。

- 安装与打包落点（已实现）：
  - 桌面：`apps/desktop/scripts/before-pack.js` 在打包前调用
    `installRuntime({ destination: release-resources/provider-runtimes/omp-native-host/1 })`，
    `electron-builder.yml` 的 `extraResources` 把 `provider-runtimes` 复制进应用资源；
    macOS 额外把 bun 二进制登记进 `binaries` 以便签名。
  - 校验：`verify-packaged-resources.cjs` 校验 manifest 字段、bun 可执行、notice 文件，
    以及 **`ait-native-host-hello.ts` 与 `ait-native-host-bridge.ts` 必须存在**——
    少了补丁标记的包会在打包阶段直接失败，而不是发出一个 `/vibe` 永远不可用的产物。
  - Headless：daemon 启动时 `host::native_host::ensure_runtime(data_dir)` 先探测
    （顺序：`AIT_OMP_NATIVE_HOST` → `<data_dir>/providers/omp-native-host/1` →
    `AIT_SERVER_OMP_NATIVE_HOST_BUNDLE` → 可执行文件旁的 `provider-runtimes/...`），
    只有都没有时才用 `AIT_SERVER_NODE_BIN` 跑一次
    `runtimes/omp-native-host/install.mjs --data-dir <data_dir>`；任何失败只 warn。
  - 打包形态**不复制**运行时到用户数据目录：应用旁的 bundle 被原地使用，
    现场实测 daemon 启动日志为
    `Using the OMP native host runtime manifest=<resources>/provider-runtimes/...`，
    且 `<data-dir>/providers` 未被创建。
  - 独立 daemon 发布包：`scripts/release-assets.mjs daemon ... --runtime <dir>` 把
    `provider-runtimes` 一起打进 tarball，并由 CI 传入打包产物的实际路径。
