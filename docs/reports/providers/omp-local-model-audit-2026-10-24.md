# OMP 本机六模型实测与兼容修补

日期：2026-10-24，Linux x86_64，OMP **18.12.6**。初始检查使用已安装的 Ait daemon
**0.1.15**；现已通过远程 AUR `ait-git` 升级至 **0.1.16.r715.gc2181d6-1** 并重启验证。
系统 daemon 由 pacman 管理，路径为 `/usr/lib/ait/resources/bin/daemon`。

源码修补位于独立工作区 `../omp-provider-audit`、分支 `fix/omp-provider-audit`，基线为
远程 main `c2181d69`。修补已构建成隔离测试 daemon，尚未进入远程 AUR 安装包。

## 逐模型结果

全部六个模型先经过真实 `omp --print --mode json` 调用，再经过 Ait 的 `omp` provider
创建会话并请求原生 `read` 工具读取测试文件。没有使用模型 mock 或其他 provider 代答。

| OMP 模型                           | 原生 CLI                                 | Ait 实际工具调用                               | Ait 生效推理等级 | 结论                      |
| ---------------------------------- | ---------------------------------------- | ---------------------------------------------- | ---------------- | ------------------------- |
| `futureppo/deepseek-v4-flash-0731` | 两次分别等候 60、90 秒，均无输出         | 会话成功创建；120 秒无输出后结束测试           | `max`            | 上游直接请求返回 HTTP 502 |
| `futureppo/deepseek-v4.1-flash`    | 两次分别等候 60、90 秒，均无输出         | 会话成功创建；120 秒无输出后结束测试           | `max`            | 上游直接请求返回 HTTP 502 |
| `futureppo/grok-4.7`               | 成功，约 8.9 秒                          | `read` 完成，答案正确，回到 `idle`，约 21.4 秒 | `xhigh`          | 可用                      |
| `futureppo/minimax-m3.1-flash`     | 成功，约 2.3 秒；另测真实读档成功        | `read` 完成，答案正确，回到 `idle`，约 5.3 秒  | `xhigh`          | 可用                      |
| `futureppo/z-ai/glm-5.3`           | 成功，约 6.8 秒                          | `read` 完成，答案正确，回到 `idle`，约 6.0 秒  | `max`            | 可用                      |
| `tai/deepseek-v4.1-flash`          | 有工具时成功；无工具请求经下述修补后成功 | `read` 完成，答案正确，回到 `idle`，约 5.2 秒  | `max`            | 可用                      |

Ait 四个成功会话均读到 `AIT_OMP_READ_OK_742196`，timeline 中存在 `read` 的
`completed` 事件，无工具错误；最终 `activeTurn=null`、`lastError=null`。
这里的耗时包括启动和检查，是单次观测，不是性能基准。

## 已应用的本机修补

### tai 的空工具请求返回 HTTP 400

原生 `--no-tools` 请求携带 `tools: []`，InferKnock 明确返回：
`tools array cannot be empty; omit the field entirely to disable tool use`。
启用 `read` 后，同模型立即成功，证明不能把这个 400 当作上游模型不可用。

已在 `~/.omp/agent/extensions/inferknock-empty-tools.js` 安装原生扩充：

```js
// InferKnock rejects tools: [] on Responses requests; omission disables tools.
export default function inferknockEmptyTools(api) {
  api.on("before_provider_request", (event, context) => {
    if (context.model?.provider !== "tai" || context.model.api !== "openai-responses") {
      return;
    }
    const original = event.payload;
    if (!original || typeof original !== "object" || Array.isArray(original)) {
      return;
    }
    if (!Array.isArray(original.tools) || original.tools.length !== 0) {
      return;
    }
    const payload = { ...original };
    delete payload.tools;
    return payload;
  });
}
```

修补仅在 tai 的 Responses 请求上省略空工具数组；非空工具保留。
无工具真实请求已返回 `OMP_EMPTY_TOOLS_FIXED`，Ait 的 tai 真实 `read` 也通过。
这是本机 OMP 扩充兼容修补，未修改安装包，也未将其描述为上游代码已修复。
新建原生进程会自动加载；显式 `--no-extensions` 会禁用此修补。

### tai 工具回传时的加密 reasoning 状态失效

修补后的隔离 daemon 做读档回归时，tai 的第二次推理请求返回 HTTP 400：
`responses_reasoning_invalid`，提示状态无效、过期或属于其他 key。
这发生在新会话的工具回传阶段，第一轮请求和 `read` 本身已完成。
此前相同调用也曾成功，故不能把一次成功当作这条代理线路稳定支持加密状态重放的证明。

已为 tai 增加原生支持的兼容配置：

```yaml
compat:
  supportsDeveloperRole: true
  includeEncryptedReasoning: false
  filterReasoningHistory: true
```

`includeEncryptedReasoning=false` 停止请求加密状态，`filterReasoningHistory=true`
过滤历史中的原生 reasoning 项；推理等级仍为 Max，用户消息、工具调用与结果继续传递。
OMP 原始码已确认这两个字段分别控制加密请求和重放过滤；这是线路兼容修补，
未声称已经修复代理服务内部的 key 或状态管理。
配置修改前另存 `backups/audit-20261024/models-before-tai-reasoning.yml`，
解析比较确认其他字段和凭据未变。

修改后原生 CLI 的 Max 真实读档和无工具请求均通过。
已安装远程 AUR daemon 与修补后的隔离 daemon 也分别完成 Max 读档，
随后在同一会话、不再次读档的情况下正确回忆测试标记，均回到 `idle`。

### 模型推理配置使用无效字段

`~/.omp/agent/models.yml` 六个模型的 `thinkingLevelMap` 不属于 18.12.6 配置 schema，
原始码也没有读取这个字段。已替换成原生支持的 `thinking.mode: effort` 和 `thinking.efforts`：

- DeepSeek、GLM：`[low, high, max]`。
- Grok、MiniMax：`[minimal, low, medium, high, xhigh]`。

等级取自修补前的原生模型目录，修补后重新执行 `omp models --json` 确认一致。
没有把不受支持的 `xhigh` 或 `max` 额外宣告为可选等级。
配置格式依据 [18.12.6 模型 schema](https://github.com/can1357/oh-my-pi/blob/v18.12.6/packages/coding-agent/src/config/models-config-schema-bundle.ts)，
请求扩充依据 [18.12.6 扩充事件](https://github.com/can1357/oh-my-pi/blob/v18.12.6/docs/extensions.md)。

### 缺少 smol 角色

原生日志反复出现技能描述压缩失败：`No smol or tiny model available`。
已在 `~/.omp/agent/config.yml` 增加
`modelRoles.smol: futureppo/minimax-m3.1-flash:low`，该模型以 `low` 实际读档通过。
后续探测进程的日志没有再出现上述缺少模型的告警。
原有 `modelRoles.default: tai/deepseek-v4.1-flash:max` 保留。

## Futureppo 上游异常证据

独立于 OMP 和 Ait，直接对原配置的 `/v1/chat/completions` 提交最小文本请求，
使用同一凭据，`stream=false`、`max_tokens=256`、`reasoning_effort=low`：

- `deepseek-v4-flash-0731`：约 45.6 秒后 HTTP 502，正文 `error code: 502`。
- `deepseek-v4.1-flash`：约 55.7 秒后 HTTP 502，同样正文。

`GET /v1/models` 虽列出这些模型，但不能证明推理端点正常。
因此这两项记为本次测试期间上游不可用；没有删除模型或偷偷改用另一条供应商线路。
上游恢复后，仍需重新执行 CLI 和 Ait 工具测试。
补充探测中，tai 对 Python 默认客户端返回过 HTTP 403；其原生 OMP 请求成功，
故未将该探测结果用作 tai 不可用的证据。

旧诊断中存在 OMP `create: Rejected`。最初使用目录返回的完整模型 ID 和 `full` 模式，
六个会话均成功创建，但这没有覆盖界面提交的所有特性组合。后续截图和补充复现确认了下述
Ait 源码问题，不能用基础读档测试通过推导 Ait 没有源码错误。

## Ait 界面错误的补充复现

用户提供的界面截图包含 `Thinking=Max`、`Fast service tier=开启`，创建请求返回
`agent.create.request / invalid_message`。使用相同 tai 模型，提交完整四个特性开关：

- `autoCompaction=true`、`autoRetry=true`、`fastMode=false`、`slowMode=false`：
  创建成功，正常回到 `idle`，输出 `AIT_UI_CONFIG_OK`。
- 仅将 `fastMode` 改成 `true`：创建失败，返回 `Invalid message or parameters`。

Ait 的 OMP adapter 无条件展示 Fast/Slow service tier；OMP 的 `set_fast_mode` 则会拒绝
没有 service-tier family 的模型，包括这里的 DeepSeek。这是已经确认的 Ait 能力展示问题。
本机 OMP 配置修补不能解决它。截图中聊天历史附件的组合尚未独立复现，不能声称整个界面
错误已完全定位或修复。

另一个已核实的问题是 Ait 只给默认模型返回推理选项。原生 RPC 的六个模型都返回
`thinking.efforts`，Ait 的五个非默认模型却返回空 `thinkingOptions`。
已在独立源码工作区修复这两项：

- 新建草稿、默认目录、运行中会话均按原生能力过滤 Fast/Slow。
  `get_state.slowModeSupported` 提供 Slow 能力；Fast 没有只读能力字段，故只在
  `--no-session` 的临时进程中尝试 `set_fast_mode=true`，随后关闭。
  不调用可能修改全局设置的 `set_slow_mode`。原生拒绝 Fast 时隐藏开关，其他传输错误继续上报。
- 草稿通过 CLI 的 `--provider`、`--model` 选择完整路由，再核对实际模型，避免模糊匹配和
  同名模型混淆；不向探测进程提交用户输入或写入原生会话。
- 非默认模型从原生 `thinking.efforts` 恢复推理选项，加入原生 RPC 接受的 `off`；
  不凭模型名字猜测等级，拒绝不合法或重复的等级。
- 运行中切换模型后重新查询能力；待应用模型与当前模型不同时，不复用旧模型的档位能力。
  未连接原生进程时，仅展示已知通用开关。

修补增加一次临时原生进程启动来确认运行中会话的能力，没有新增模型推理请求。
这些修改均在 OMP adapter 内，未变更端口、领域边界、依赖或协议。
前端已有逻辑只提交当前可用特性，因此旧的 `fastMode=true` 偏好会被过滤。
截图中历史附件组合未独立重放，仍不把该组合描述为已完全验证。

## 远程 AUR 更新与重启（已完成）

用户指定使用远程 `ait-git`。已从 `https://aur.archlinux.org/ait-git.git` 重新克隆，
AUR 配方提交为 `3b1b749359d6bc55de1be30deb2a3abef21f658a`，应用源码直接拉取远程 main
`c2181d69`，没有叠加本地源码补丁。生成软件包：
`ait-git-0.1.16.r715.gc2181d6-1-x86_64.pkg.tar.zst`。

构建最初放在仅 3.9 GiB 的 `/tmp`，配额中断留下零字节 favicon 缓存。已将这份远程构建
移到 Home 磁盘，删除该生成缓存并完成打包。打包时包内 daemon 与远程源码构建产物的 SHA-256 一致：
`476ea5e0d6eabe7b5b610f2ca9f97e8a03610c18da8962e5fa6445cf180411fa`。

对新 daemon 使用隔离数据目录与 loopback 端口实测：版本 `0.1.16`、生命周期 `ready`，
发现六个 OMP 模型；完整配置关闭 Fast 时，tai `read` 正确完成并回到 `idle`。
开启 Fast 仍返回同样创建错误。因此不能把远程升级描述为已经修复该源码问题。

用户在本机完成 sudo 认证后，已安装上述远程软件包，并由独立 systemd 单元自动重启。
实际结果：

- `pacman -Q ait-git`：`0.1.16.r715.gc2181d6-1`。
- `ait-daemon.service`：`active/running`，PID 从 `772` 变为 `3555198`，
  启动时间为 2026-10-24 13:03:27 CST。
- daemon 报告 `0.1.16`、`ready`；server ID 保持一致，instance ID 已改变。
- 重启后目录仍为六个模型；tai 的 Max 真实读档返回 `AIT_OMP_READ_OK_742196`，
  状态为 `idle`，无错误。

软件包位于 `~/.cache/paru/clone/ait-git-remote-20261024/`。
安装和外部重启脚本、来源清单、实际重启结果位于
`~/.local/state/ait-maintenance/20261024-remote-aur/`：
`install-and-restart.sh`、`build-manifest.json`、`restart-result.json`。
系统继续使用这份远程软件包，没有用本地修补二进制覆盖安装文件。

## 复现、证据与还原

原生基线对每个完整模型选择器依次执行：

```sh
omp --print --mode json --model futureppo/grok-4.7 \
  --no-session --no-title --no-lsp --no-tools --no-extensions \
  --no-skills --no-rules --max-time 60s \
  'Reply with exactly OMP_AUDIT_OK. Do not use tools.'
```

tai 修补后的无工具回归省略 `--no-extensions`，预期文字为 `OMP_EMPTY_TOOLS_FIXED`。
两项 DeepSeek 重测使用 `--tools read --thinking low --max-time 90s`。
Ait 测试经已运行 daemon 的 `/mcp/agents`，逐项调用
`create_agent`、`wait_for_agent`、`get_agent_status`、`get_agent_activity`、`kill_agent`。
例如模型参数使用 `omp/["tai","deepseek-v4.1-flash"]`，`modeId=full`；
只在测试会话关闭 `autoRetry`，防止重试掩盖首次错误。

本机原始证据和驱动脚本位于 `/tmp/ait-omp-audit-20261024/`，
其中 `verified-summary.json` 汇集六项 Ait 结果，`direct-deepseek.log` 保存两个 502。
配置备份位于 `~/.omp/agent/backups/audit-20261024/`。
还原时复制其中的 `models.yml`、`config.yml` 回 `~/.omp/agent/`，
并移除新增的 `extensions/inferknock-empty-tools.js`。

所有六个测试会话均已结束并归档，对应六个临时 Workspace 也已归档，原生历史保留用于核对。
凭据与供应商路由逐项比较后确认未改变，模型数量仍为六个。
配置、扩充和备份均为本机文件；仓库只记录源码、测试、本报告与索引，不包含 token、数据库或原始运行产物。

## 客户端图标与 i18n 补充

截图还暴露了客户端展示问题，与模型推理能否完成分开处理：

- 四个 OMP 特性只提供 ID 和英文标签，客户端缺少映射，统一退回 `Settings2`。
  已为 `autoCompaction`、`autoRetry`、`fastMode`、`slowMode` 分别映射
  Lucide `Minimize2`、`RotateCcw`、`Zap`、`Turtle`，同时保留服务端明确声明的已知图标。
  工具栏、手机设置列表、选项图标与命令中心共用同一规则。
- 服务端未给模式传图标时，`buildProviderDefinitions` 一律填 `ShieldCheck`，
  造成 Full Access 也显示盾牌勾选。现在先按 provider 和 mode ID 查找内置定义；
  OMP 的 full 使用 `ShieldOff`，write 使用 `ShieldAlert`，ask 使用 `ShieldCheck`。
  原生明确指定的图标与风险色阶仍优先，未增加服务端没有宣告的模式。
- 四个特性标签直接显示英文，中文资源中的 Thinking、Mode 等条目也仍是英文。
  现已统一走 i18n，补齐九种已支持语言的四个特性标签和两个 OMP 模式标签。
  常见推理等级及权限模式复用既有翻译，中文显示“推理强度／关闭／最高”、
  “模式／完全访问”、“自动压缩上下文”、“自动重试”等。
- 增加语言变化依赖，避免选项在 `useMemo` 中保留旧语言。
  手机设置面板、桌面控件与配置文件编辑中的特性名称都经过展示层翻译，
  协议 ID、布尔值、模型路由和自定义扩展标签不随翻译改变。

相关实现位于 `apps/mobile/src/agent-controls/`、`composer/agent-controls/`、
`utils/provider-definitions.ts` 和九份语言资源。本轮没有再修改 Rust，
依仓库规则没有重复执行 Rust 测试。此前的 OMP Rust 修补仍保留在同一独立工作区。

渲染回归测试保留真实的 DraftAgentControls、权限模式及特性控件，
隔离模型浏览器、原生快捷键和服务连接，检查中文标签、对应图标及切换英语后的原地更新。
这属于 JSDOM 控件测试，未声称在真实 iPhone 上完成新客户端验收。
本次客户端源码尚未发布；重启 daemon 不能更新已经安装的手机客户端。

## Test coverage

**not measured**：本次未请求覆盖率，不提供行覆盖率百分比。若后续需要测量，
应使用 `cargo llvm-cov` 限定 `provider` 的 OMP 专项测试，不扩大为整个 workspace。
验证基线为 `c2181d69572db746dc6526a5de7457d724cf9153`。
Rust 源码对应提交 `6667ebc9934e0552dbae8ca16ffeddc7ffe074b5`，
客户端源码对应提交 `ce800b2f8c8dc579955685ce22edc0ac5188da11`；
所列测试已在提交前针对相同源码执行，后续提交仅补齐本报告。

执行结果与覆盖率分开记录：

本轮客户端验证：

- 在旧实现上新增回归复现了中文仍显示 `Full access`、`Max` 以及特性图标回退。
- `apps/mobile` 下执行 `vitest run --project unit`，指定以下十个测试文件：
  `src/agent-controls/labels.test.ts`、`src/agent-controls/icons.test.ts`、
  `src/i18n/resources.test.ts`、`src/composer/agent-controls/utils.test.ts`、
  `src/composer/agent-controls/speed-toggle.test.tsx`、
  `src/composer/agent-controls/localization.test.tsx`、
  `src/utils/provider-definitions.test.ts`、
  `src/provider-selection/resolve-agent-form.test.ts`、
  `src/agent-profiles/internal/profile-form-model.test.ts`、
  `src/command-center/agent-control-contributions.test.ts`：**173 项通过**。
  覆盖九语言键同步和插值、标签映射、图标映射、模式回退、关联表单以及实际控件的语言切换。
- `tsgo --noEmit -p apps/mobile/tsconfig.json`、`node scripts/check-ui.mjs`、
  改动文件的 `oxfmt --check`、`git diff --check` 均通过。
- 未运行整套客户端测试或覆盖率。渲染测试原始 JSON 位于
  `/tmp/ait-omp-audit-20261024/ui-render-tests.json`，属于本机测试证据。

此前的 Rust 与原生 OMP 验证：

- 三个新增回归测试在原生产代码上全部失败，确认非默认推理选项与档位展示问题。
- `cargo test --locked -p provider local::omp:: --lib`：29 通过、3 忽略；
  忽略项为两个需本机 OMP 的测试和一个需要额外凭据的完整交互测试。
- `AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -p provider local::omp::tests::installed:: --lib -- --ignored --nocapture`：
  两个真实原生 RPC 测试通过，未发起推理，确认 GPT 档位保留、DeepSeek 档位隐藏、
  两模型 Max 可选、`config.yml` 与 `models.yml` 字节不变且无探测会话文件。
- `cargo clippy --locked -p provider --lib --tests -- -D warnings` 被其他模块已有的
  14 处 `clippy::assert_is_empty` 阻止；这些诊断来自当前 Rust 1.99。
  仅为这些既有断言放行该 lint 后，
  `cargo clippy --locked -p provider --lib --tests -- -D warnings -A clippy::assert_is_empty` 通过。
- `cargo clippy --locked -p provider --lib -- -D warnings`：生产库严格检查通过。
- `cargo fmt --all --check`、`git diff --check` 通过。
- `RUSTFLAGS='-C opt-level=3 -C target-cpu=native' cargo build --locked --release -p daemon`：
  构建成功。`filesystem::workspace_runtime` 已有的 `Atomic::fetch_update` 弃用告警仍存在，
  本次未扩大到无关源码修改。

修补后的隔离 daemon 经真实 `/mcp/agents` 验证：

- 六个模型的 `list_models` 均保留原生推理等级；`inspect_provider` 均只返回实际支持的
  `autoCompaction`、`autoRetry`，即使草稿携带旧的 Fast/Slow 值也不宣告其可用。
- Grok、MiniMax、GLM、tai 四个实际 provider 回合分别以 `xhigh`、`xhigh`、`max`、`max`
  完成原生 `read`，检查 timeline 中 `read/completed`，答案为 `AIT_OMP_READ_OK_742196`。
  tai 首次失败的 reasoning 状态及后续兼容修补按上一节保留记录，没有隐藏失败。
- tai 修补后继续第二轮对话成功；能力查询及读档前后配置哈希一致。
- 使用移动端现有 `pruneFeatureValues`、`resolveFeatureValues` 的真实 TypeScript 函数，
  经 Node 类型擦除执行断言，确认旧的 `fastMode=true`、`slowMode=true` 不再进入提交值。
  该阶段尚未更改移动端源码；后续客户端改动及渲染测试见上节，未执行真实手机 UI 自动化。

隔离验证原始结果位于
`~/.local/state/ait-maintenance/20261024-remote-aur/`：
`patched-preflight-before-tai-fix.json`（前三个模型通过及 tai 首次失败阶段）、
`patched-preflight-result.json`（兼容修补后六模型能力、tai 两轮通过）、
`stock-tai-compat-result.json`（系统远程 AUR daemon 两轮通过）。
本地修补测试 daemon 已停止，测试 Agent 与 Workspace 已结束、归档；系统 daemon 继续运行。

原始六项 CLI、六项 Ait provider 测试及两项独立上游探测见前文。
四个可用模型读档通过；两个 Futureppo DeepSeek 在本次上游 HTTP 502，未伪报通过。
本机扩充通过 `node --check`、`oxlint`，本机 YAML 经解析、目录加载与 `oxfmt` 检查。
未运行整个 Rust workspace 测试或覆盖率；未验证图像、压缩和长期运行。
远程包升级、源码修补和源码部署是不同结果；源码补丁尚未进入已安装 AUR 包。
