# ADR-100：功能 crate 作为完整服务安装

- 状态：Accepted。
- 日期：2026-10-07。
- 范围：功能 crate 对外服务、daemon 组装、API 安装与方法声明。
- 修订：[ADR-035](adr-035-daemon-capability-groups.md) 的细分服务安装组合与 [ADR-095](adr-095-component-method-declarations.md) 的安装筛选粒度。

## 背景

filesystem 的文件、Git、Forge 等对象多数并列，通过共享注册表和端口协作；metadata
围绕 Workspace 数据组合目录、标签、命名与自动化；provider 的执行对象直接持有管理器
和运行目录，而预设目录保持独立存储。内部职责、锁和后台任务的拆分有价值，但生产 daemon
整体组装这些功能，API 将每个对象作为独立安装开关增加了无实际生产需求的组合。

## 决策

七个功能 crate 均提供根级 `Service` 入口。filesystem、metadata、provider 使用完整组合
对象和无可选功能字段的 `Dependencies`；browser、schedule、terminal、voice 复用现有单一
对象作为 `Service`。构造参数是组装输入，内部功能对象不再是 API 的独立安装单位。

`api::Services` 仅包含七个可选的 crate 级服务。各能力模块继续提供两种方法元数据接口：
`implemented_methods()` 返回本 crate 的全部声明；`installed_methods(bool)` 在已安装时
返回全部声明，未安装时返回空迭代器。移除 `InstalledServices` 以及内部对象级筛选。
CLI 安装、认证、模型配置与后端健康状态继续通过业务结果表达，不改变方法安装集合。

API 自行声明始终安装的九个连接方法：server info、ping、状态订阅、订阅释放、桌面 editor
迁移响应、会话心跳、会话事件订阅和创建记录订阅。metadata 不再将这些方法计入自身声明；
其会话、创建记录和连接分发实现继续作为 API 可复用的基础设施。精简 host 可以没有业务
服务，仍使用这些连接操作与三个 relay 方法。方法名称、消息方向及 wire 字段保持一致。

provider 服务必含预设目录、原生执行及辅助生成。运行目录由执行对象持有，API 不再支持
将独立运行目录作为另一种 Provider 安装组合。内部对象与端口仍可用于独立测试和跨 crate
协调；构造服务不会合并存储、扩大锁的范围或创建额外执行队列。

API 的私有 transport composition 将完整服务转为独立同步的请求句柄。该转换不参与能力
筛选，也不接受公共的细分安装开关。现有命名取消、Git 观察、setup 事件、worktree 清理和
Provider/终端停机顺序保持原来的共享实例及生命周期。依赖方向不变，metadata 不反向依赖
filesystem 或 provider；跨能力协调仍由 daemon 与 API 连接端口完成。

## 后果与验证

服务完整性由构造参数保证，API 按 crate 整体安装；测试不再枚举不存在的内部安装组合。
组件测试验证全量或空方法集合，API 测试验证基础方法归属、唯一性及精简 host 协议行为，
生产 daemon 测试验证全部 179 个实现方法和 180 个可协商能力。另行验证事件、目录推送、
原生执行、worktree 资源清理与停机行为。

新增功能应纳入所属 crate 的完整服务和方法声明。内部依赖与资源仍按职责拆分，不引入
一个组件共用的全局互斥锁。
