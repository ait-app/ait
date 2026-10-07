# ADR-089：内置 Provider 装配归 provider crate

- 状态：Accepted。
- 日期：2026-10-06。
- 范围：原生 adapter 注册、启动配置与元数据生成装配。
- 关系：细化 [ADR-031](adr-031-daemon-provider.md) 的能力边界与
  [ADR-058](adr-058-daemon-metadata-generation.md) 的辅助生成职责。

生成接口归属及配置注入现由 [ADR-100](adr-100-provider-summary-generator.md) 修订；原有配置键和生成行为保留。

## 背景

daemon 的 `compose_provider` 直接列举各个原生客户端，读取启动路径、DSH 传输选择并配置
图片目录。增加一个 provider 就必须修改 daemon。`compose_metadata` 又单独构造 Codex 和
Claude，并通过 `native_clients` 命名暗示只有两者属于原生客户端。

实际上所有内置 adapter 都对接对应的原生程序；Codex 与 Claude 目前额外实现了
`AgentClient::generate_metadata`，可执行不进入前台 Agent registry 的结构化辅助请求。

## 决策

1. `provider::Providers` 是宿主的内置 adapter 装配入口。宿主只传入持久数据目录，获取
   `MetadataGenerator` port，并把整组 adapter 注册到 `AgentManager`。
2. 内置列表、可执行文件默认值与覆盖变量、安装发现、DSH 传输选择、图片存储路径，以及
   可用于元数据生成的 adapter 集合均由 provider crate 内部维护。`local` adapter 模块不再
   作为 crate 的公共 API。
3. 同一次装配配置前台执行与辅助生成。只有实现结构化辅助生成的客户端进入生成器；该集合
   是具体能力选择，不是独立的「native client」类别。
4. 装配模块协调具体 adapter 与服务；应用服务继续依赖 `AgentClient` port。
   `AgentManager::register_client` 保留，供其他宿主与测试注入实现。
5. daemon 继续拥有存储根、进程 lifetime、跨能力服务连接和 execution worker 的启动。
   构造与注册 adapter 不启动原生进程、不创建存储；能力发现仍遵循既有有界调度。

## 后果与限制

新增内置 provider 不再要求 daemon 了解其客户端类型或启动配置。原有环境变量、原生认证、
AGY 官方与 Homebrew 安装发现、DSH native/ACP 行为和公共 provider catalog 协议保持兼容。
OpenCode 的自定义预算 builder 仅用于内部测试注入；生产 adapter 仍使用原有默认预算。

确定性测试覆盖完整注册、重复身份、无启动副作用、显式路径不回退、DSH 传输能力，以及
Codex/Claude 的结构化辅助生成。原生程序的真实能力仍以各 adapter 的协议与验证范围为准。
