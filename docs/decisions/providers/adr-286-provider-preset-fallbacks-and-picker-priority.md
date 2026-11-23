---
id: ADR-286
status: accepted
date: 2026-11-18
amends:
  - providers/adr-279-provider-model-presets.md
paths:
  - crates/domain/src/preset.rs
  - crates/provider/core/src/ports/agent_session.rs
  - crates/provider/runtime/src/service/presets.rs
  - crates/provider/omp
  - apps/mobile/src/agent-controls
  - apps/mobile/src/components
  - apps/mobile/src/composer
  - apps/mobile/src/create-agent-preferences
  - apps/mobile/src/data
---

# ADR-286：模型预设的备选链、供应商展示与选择器优先级

**后续修订：** [ADR-301](adr-301-preset-selection-identity.md)

## 背景

ADR-279 建立了 Provider 模型预设的统一端口，但在实际使用中暴露出四处边界缺陷：

1. 模型卡片把预设放在整份模型目录之后，并要求先发现 `modelRole.*` 才展示预设：OMP 目录常含数百个多供应商模型，预设被埋在列表底部，而没有独立角色项的 Ait 托管预设在设置中保存后无法在模型卡片中选择。
2. OMP 支持按角色配置有序备选模型链（`retry.fallbackChains`），并允许对推理模型指定 `:auto` 与 `:off` 等级，但预设契约与私有 `--config` overlay 只投影了 `modelRole.<role>`，导致真实 OMP 预设在应用时因 `:auto` 校验失败而无法覆盖已选默认模型，也无法编辑备选链。
3. `PresetService` 在每次 `list`、`save`、`delete` 前重复执行完整的原生 `describe` 子进程探测，客户端又在读取与保存前串行做预检查询，并在每次输入渲染时重复解析完整模型目录。
4. 多供应商路由（如 OMP 的 `[provider, modelId]`）在预设编辑器与列表中隐去了上游供应商，同名或相近模型无法区分来源。

## 决策

本决策修订 ADR-279 的第 1、2、4、5 条：

1. **备选模型链契约**：`domain::preset` 与会话配置在 `modelRole.<role>` 之外接受 `modelFallback.<role>`（含 `default`），取值为 `"native"`（继承）或至多 32 项有序非空选择器数组。`provider-omp` 将其双向投影到原生预设的 `fallbackChains`，并在会话私有 `--config` overlay 中生成 `{"retry": {"fallbackChains": …}}`，不修改用户的全局或项目 `config.yml`。
2. **OMP 角色与推理等级对齐**：`provider-omp` 的角色目录合并 OMP 内置角色集、用户自定义标签与预设/备选链引用的角色，推理模型补齐 `auto` 与 `off` 等级；原生 `model_changed` 同步会话已选模型快照，应用预设时将 `modelRole.default` 归一为继承并替换表单偏好中的旧角色与备选链，确保切换预设始终覆盖当前默认模型。
3. **无 I/O 存储归属分派与并发读取**：`AgentClient::preset_backend` 以纯内存声明存储归属，`PresetService::list/save/delete` 直接按归属分派，不再在每次读写前重跑原生 `describe`；OMP 预设列表与保存复用按工作目录缓存的角色目录并与配置层并发读取，客户端并发请求 `describe` 与 `list` 且不在写前重复拉取列表。
4. **选择器优先级与供应商标识**：Provider 设置页与模型选择卡片均在模型列表上方展示“模型预设”，只要该 Provider 存有预设且存储未报错即展示（不再以 `modelRole.*` 作为可见性前提）。预设编辑器与列表统一使用一次性索引展示“上游供应商 / 模型名”，单供应商模型不重复前缀；composer 将 `modelRole.*` 收纳至特性面板并隐藏与主选择器重复的 `modelRole.default`。

## 后果

用户在模型卡片顶部即可一键切换任意 Provider 的模型预设并立即覆盖默认模型；OMP 预设可配置主模型与各角色的有序备选链并在下一轮通过私有 overlay 生效。预设打开与保存消除了多余的 CLI 子进程与前端主线程解析开销，上游供应商在模型、角色与备选选择器中清晰可见。
