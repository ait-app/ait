# ADR-116：库 crate 只公开被其他 crate 使用的项

- 状态：接受
- 日期：2026-10-09
- 关联：[ADR-107](adr-107-direct-imports-from-owning-crates.md)（直接从所属模块导入）、
  [ADR-112](adr-112-persistence-crate.md) 与
  [ADR-113](../workspace/adr-113-filesystem-capability-groups.md)。

## 背景

各库 crate 中约 3,870 个项、字段和模块声明为 `pub`，其中大部分只在本 crate 内使用。
`pub` 让编译器无法报告未使用代码，并把实现细节暴露给 daemon、API 和其他能力 crate。
filesystem、metadata 和 provider 的 service 模块还用 `pub use` 转发 ports 中的类型，
使类型看起来同时属于两个模块。

## 决策

1. 库 crate 默认私有。只有其他 workspace crate（最终是 `daemon` 二进制及其集成测试）
   实际使用的项保留 `pub`；crate 内共享使用 `pub(crate)`，模块内使用保持私有。
   `pub use` 只用于 crate 根的组装入口和重命名，不再转发其他模块的类型。
2. 根 `Cargo.toml` 启用 `unreachable_pub`，CI 的 `-D warnings` 拒绝不可从外部访问的 `pub`。
   收缩后由 `dead_code` 暴露的未使用代码直接删除：`persistence::watch` 文件观察、
   `File::path`、`Api::browser`、`Hello::negotiate`、`PaseoConfigRaw::into_value`、
   metadata 的 `PairingOffer`/`ProjectConfig*Success`/`WorkspaceScriptHealth`、
   未构造的 Forge/Git 枚举变体、provider 的 `Row::value` 和 `CreateRequest::subscribe`。
3. 只被本 crate 测试使用的便捷方法加 `#[cfg(test)]`，不随生产代码编译，例如
   `FileRegistry::{writer, set_writer}`、`PushTokens::active` 和 `AgentManager` 的测试入口。
4. 为兼容 `deny_unknown_fields` 而接收但不读取的 wire 字段保留，并用带原因的
   `#[expect(dead_code)]` 标注，避免移除后让已有客户端请求解析失败。
5. `len` 需公开的连接类型同时公开 `is_empty`，满足 `clippy::len_without_is_empty`。
   因可见性收缩而生效的 `clippy::struct_field_names` 对 wire schema 字段名用 `#[expect]` 豁免。

## 后果与验证

外部可见面收缩到约 1,560 个 `pub` 项，其余为 `pub(crate)` 或私有。协议字段、持久格式和
运行行为不变。新增 `pub` 时编译器会检查它是否可达，新代码无人使用时会立即报告。

验证覆盖全 workspace 的 `cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings`
和 `cargo test --workspace`。`process` 集成测试中
`websocket_directory_streams_keep_sequences_ownership_and_reconnect_checkpoints` 在基线提交
同样偶发超时（各 12 次运行中基线失败 3 次、本变更失败 2 次），与本决策无关。
