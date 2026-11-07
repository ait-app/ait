# ADR-223：Claude 的 advisor 角色与统一模型预设的展开规则

状态：接受（阶段 A、B）。日期：2026-11-07。扩展 [ADR-220](adr-220-omp-model-presets.md)。

## 背景

用户希望"一个会话里同时有不同用途的模型"能像 OMP 的模型预设一样被统一配置。Claude Code
确实提供第二个模型槽位，但它只有**一个**额外角色，且完全没有原生预设目录：

- `claude --advisor <model>`（2.1.299 中为隐藏选项）或 settings 键 `advisorModel`；
- advisor 只在 first-party 部署、且 `CLAUDE_CODE_DISABLE_ADVISOR_TOOL` 未设置、
  `CLAUDE_CODE_ENABLE_EXPERIMENTAL_ADVISOR_TOOL` 或远端开关允许时启用；
- 主模型必须支持 advisor，advisor 的能力档位必须不低于主模型，否则 CLI 只打 warning；
- 同一会话内更换 advisor 要等 `/clear` 或 `/compact` 才完全生效。

因此 Claude 能贡献的是"角色层"，预设列表必须由 Ait 自己持有。

## 决策

### 1. 角色命名与 OMP 对齐

Claude 的 advisor 暴露为既有形状的 select feature `modelRole.advisor`，取值
`native` / `unset` / 模型别名或标识符。不新造 `claudeAdvisor` 之类的 provider 前缀，
这样前端既有的 `isModelConfigurationFeature`、`mergeRoleChoices`、`withModelPresetSelection`
以及 OMP 的 `modelRole.*` 无需任何分支即可复用，将来统一预设也能把该键原样塞进
`featureValues`。

`native` 表示沿用 Claude 自己的用户/项目/本地设置（Ait 已传
`--setting-sources=user,project,local`）；`unset` 表示本次会话明确不要 advisor。

### 2. 展开位置

`native` 不产生参数；具名值产生 `--advisor=<model>`；`unset` 通过进程环境
`CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1` 关闭。不能改用 `--settings={"advisorModel":null}`：
Claude 的 settings schema 只接受字符串，null 会让**整个** settings 文件被丢弃；
空字符串 `--advisor=` 会被当作未提供，清除不了用户级 `advisorModel`。

### 3. 能力门控

Ait 侧持有 2.1.299 模型目录的 advisor 档位（haiku-4-5=1 … fable-5-1=9），
校验"档位 ≥ 2 且不低于主模型"。已知组合不满足时在写入前 `Rejected`，因为静默接受会让
客户端显示一个永远不生效的选择。未知名字（别名、自定义 gateway）沿用 CLI 判定，
以 warning 降级，Ait 不因此让整轮失败。

feature 的选项带 `metadata.advisorRank`，并对当前主模型不可用的选项标注说明，
让预设作者在保存前看到限制。

### 4. 生效时机

advisor 是进程启动参数，因此沿用既有"配置变更 → 下一轮关旧 writer 并 `--resume` 重开"
路径（`claude/session.rs`）。文案必须说"下一轮生效；Claude 需新对话或 `/compact`
才完全切换 advisor"，不得承诺立即替换。不新增中途杀进程的路径。

## 阶段 B：统一预设的管理面与存储归属

### 1. 一套 UI，两种后端

用户在设置里看到的是**同一个**「模型预设」分区（列表、新建、编辑、删除），OMP 不再有
第二套入口；差异由 daemon 返回的 `PresetDescriptor` 表达：

```
{ backend: "native" | "ait",
  canCreate, canEdit, canDelete, canRename, reason? }
```

前端只读这四个能力位决定按钮是否可用，**不含任何 `provider === "omp"` 分支**；改名等
原生缺失的操作如实置灰并展示 `reason`，不做假按钮。

- **native（OMP）**：`list` 复用 `config list --json` 的 `modelPresets`（global/project
  覆盖语义不变），`save`/`delete` 转发既有 `/modelpreset save|delete` 并保留写后回读校验。
  能力位：`canCreate=true`、`canEdit=true`（同名 save 覆盖）、`canDelete=true`、
  `canRename=false`（CLI 无 rename；改名需 save 新名 + delete 旧名，非原子）。旧版 OMP
  缺 `modelpreset` 命令时 descriptor 全 false 并给出原因串。
- **ait（Claude、Codex、OpenCode、MiniMax、Pi、Antigravity、DSH）**：读写 daemon config
  `providers.<id>.modelPresets[]`，四项能力位全 true（改名是一次整体重写）。

### 2. 存储与校验

记录形状与客户端 `AgentConfigApply` 对齐：`{name, model, thinkingOptionId?,
featureValues?}`。`domain::preset::ProviderPreset` 拥有唯一的校验规则（name/model
非空且 ≤256、无控制字符、thinking 同规则、feature key 匹配
`^[A-Za-z][A-Za-z0-9._-]{0,64}$`、feature 不得为 null、每 provider ≤64 条、每 presets
序列化 ≤64 KiB、名称唯一）。daemon 配置存储层与 `invalid_provider_patch` 都调用同一函数，
因此落盘值与编辑器认知不会分叉。`providers` 走既有 `deep_merge`，单 provider 更新不影响别家。

### 3. 合成规则（关键）

`ports::preset::synthesize` 只有一条优先级：

1. provider 已发出原生 `modelPreset` → **原样返回**，绝不叠加 Ait 条目（OMP 行为零变化，
   两套存储不会打架）；
2. 无原生 feature 且 Ait 侧有条目 → 合成
   `{type:"select", id:"modelPreset", value, options:[…, __custom__]}`，每个具名选项带
   `metadata.configuration = {modelId, thinkingOptionId?, featureValues?}`，`source: "ait"`；
3. 两者都没有 → 不产生 feature，使用处保持"整块不渲染"，由空态引导去设置创建。

当前值判定与客户端 `withModelPresetSelection` 同义：模型、思考强度一致且所有已存 feature
都在生效才选中该预设，否则回落 `__custom__`。

### 4. 传输与前端

新增 `provider.presets.describe|list|save|delete.request`，在 provider dispatch、operation
与 daemon-client 中登记；写失败作为 `error` 字段随响应返回，编辑器可直接展示原因。
`ProviderPresetsSection` 挂在 `ProviderDiagnosticSheet` 顶部，空态提供「去设置创建预设」，
与既有 `ProviderSettingsAction` / `buildSettingsHostSectionRoute(serverId, "providers")`
一致。5 处 `provider === "omp"` 门控改为「features 里是否有 modelPreset」，使新 provider
的手动改模型同样回落 `__custom__`。

## 后果

- 阶段 A 不引入任何新存储：Claude 的角色值仍存在会话的 `config.featureValues`，
  Agent Profile 与表单偏好原样可存。
- 阶段 B 的 Ait 预设只落在 daemon config，**不为 OMP 写入任何副本**；OMP 的读写链路保持
  现状（`omp/roles/presets.rs` 与 `management.rs` 的解析/写后校验未改）。
- Claude adapter 仍**不发** `modelPreset`：预设由统一层合成，避免与原生语义重复。

## 验证

阶段 A 定向测试覆盖：`validate_shape` 对关键字/别名/标识符与非法值的接受与拒绝；
`features` 的选项顺序、默认值、档位标注、且不含 `modelPreset`；档位不足与低于主模型时
`Rejected`；`configuration::claude` 三态的 argv；进程级测试验证 `--advisor=opus` 到达
原生 CLI、`unset` 不产生参数且环境开关生效。

阶段 B 定向测试覆盖：daemon 配置的 preset round-trip、非法条目拒绝、单 provider 更新
不影响别家；`PresetService` 的 Ait CRUD 与跨重启持久化、无效预设不落盘；合成规则三条
分支与 `__custom__` 判定；descriptor 能力位；OMP 原生 descriptor 的 `canRename=false`、
`native_presets` 读取与 save/delete 转发；前端分区的列表/新建/删除/空态、按钮随
descriptor 变化，以及"同一能力位 + 同一列表下 native 与 ait 渲染完全一致"的 provider
无关断言。

## Test coverage

**not measured**：未请求覆盖率；如需测量，限定 `crates/provider/src/local/claude` 与
`crates/provider/src/local/configuration` 的定向测试范围。
