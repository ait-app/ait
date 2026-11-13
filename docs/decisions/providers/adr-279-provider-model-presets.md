---
id: ADR-279
status: accepted
date: 2026-11-13
paths:
  - crates/domain/src/preset.rs
  - crates/persistence/src/storage/daemon_config.rs
  - crates/provider/core/src/ports/preset.rs
  - crates/provider/core/src/ports/agent_session.rs
  - crates/provider/runtime/src/service/presets.rs
  - crates/provider/omp
  - packages/protocol/src
  - packages/client/src
  - apps/mobile/src/components
  - apps/mobile/src/data
  - apps/mobile/src/composer
---

# ADR-279：Provider 模型预设的存储归属与整套角色应用

## 背景

Provider 设置需要管理具名的模型与角色分配，模型切换卡片需要读取同一份预设。
AgentProfile 是独立的启动配置包，不能把它的列表改名后充当 Provider 的预设库。
OMP 已有原生模型角色、预设和固定的 RPC 接入；新增管理界面需要尊重这些数据的原生归属。

## 决策

1. `domain::preset::ProviderPreset` 定义预设内容：名字、默认模型、可选思考等级及
   `featureValues`。领域层负责结构与容量约束；`provider-core::ports::preset` 定义存储能力、
   条目来源和错误。运行时的 `PresetService` 协调存储，daemon 注入配置端口。
2. 原生预设库归 adapter。OMP 通过公开配置命令读写原生 `modelPresets`，项目覆盖条目只读；
   原生库不可用时报告原因，不另存一份同名 Ait 配置。没有原生库的 Provider 使用 daemon
   配置 `providers.<id>.modelPresets`；同一 daemon 中的读改写共享串行锁。
3. `provider.presets.describe/list/save/delete.request` 是客户端管理预设的统一入口。
   SDK 将四个方法全部纳入能力协商，只有 Host 声明完整方法集时才暴露 `providerPresets`。
   列表条目的来源与可编辑性随同一次读取返回，未知归属按只读处理。
4. 设置页与模型卡片共享按 Host、Provider、工作目录分隔的查询；保存或删除后失效相关缓存。
   卡片按“默认模型、模型预设”排序，仅对发现了角色配置能力且有预设的 Provider 显示预设组。
   AgentProfile 保持独立入口与语义。
5. 应用预设只产生一次配置提交，替换默认模型、思考等级与整套角色分配；未指定的角色恢复
   当前目录中的继承值，显式清空保持清空。权限、模式、工具等非模型设置不因预设切换而改变。
   设置编辑器保留无法识别的原有角色值，目录读取失败时不以空目录覆盖已保存的分配。
6. OMP 继续使用现有 `rpc-ui` 会话与 `--config` overlay 路径。预设管理不引入 ACP 桥接、
   第二套会话传输或为了读取配置而启动 RPC 探针。

## 后果

新建预设立即成为模型卡片的可选项，跨设备与重启后的读取仍由同一存储负责。
客户端根据能力与条目归属决定可用操作，Provider 差异留在 adapter；对旧 Host 隐藏新增管理功能。
新增一种原生预设库需实现端口，不需要为设置页新增 Provider 分支。
