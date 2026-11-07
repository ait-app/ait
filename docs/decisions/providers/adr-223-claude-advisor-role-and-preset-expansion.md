# ADR-223：Claude 的 advisor 角色与统一模型预设的展开规则

状态：接受（阶段 A）。日期：2026-11-07。扩展 [ADR-220](adr-220-omp-model-presets.md)。

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

## 后果

- 阶段 A 不引入任何新存储：Claude 的角色值仍存在会话的 `config.featureValues`，
  Agent Profile 与表单偏好原样可存。
- 统一预设列表留给阶段 B：daemon config `providers.<id>.modelPresets`，由前端合成为
  `modelPreset` select。Claude adapter **不发** `modelPreset`，避免与合成逻辑重复。

## 验证

定向测试覆盖：`validate_shape` 对关键字/别名/标识符与非法值的接受与拒绝；
`features` 的选项顺序、默认值、档位标注、且不含 `modelPreset`；档位不足与低于主模型时
`Rejected`；`configuration::claude` 三态的 argv；进程级测试验证 `--advisor=opus` 到达
原生 CLI、`unset` 不产生参数且环境开关生效。

## Test coverage

**not measured**：未请求覆盖率；如需测量，限定 `crates/provider/src/local/claude` 与
`crates/provider/src/local/configuration` 的定向测试范围。
