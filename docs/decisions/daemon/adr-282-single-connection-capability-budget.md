---
id: ADR-282
status: accepted
date: 2026-11-17
amends:
  - clients/adr-074-account-host-relay.md
paths:
  - crates/model
  - crates/api
  - docs/architecture/connection.md
---

# ADR-282：单连接能力提议预算随方法目录扩容

- 修订：[ADR-074](../clients/adr-074-account-host-relay.md)“单连接调度”一节中“能力提议最多包含
  256 个不重复条目”的数量。

## 背景

单连接客户端在 hello 中提交整份能力目录：`@ait/client` 的 `ALL_CAPABILITIES` 是四个分组通道
方法名的并集，其它客户端同样回显 `info.capabilities`。daemon 用
`model::server::single::MAX_CAPABILITIES` 校验 `capabilities`、`required_capabilities` 各自长度
以及二者并集的去重计数。

方法目录随功能演进单调增长。到 0.2.14 时 `info.capabilities` 恰好为 256 条，与预算相等，
没有任何余量；新增任何方法都会让整份目录回显的 hello 超过预算，被以 `invalid_message`
拒绝并在握手阶段关闭连接。ADR-074 固定写下的 256 因此不是保护性上限，而是随目录增长必然
触发的握手失败点。

## 决策

- `MAX_CAPABILITIES` 提高到 512。它仍是有界预算：hello 是单条 1 MiB 帧，能力条目受
  `valid_id`（非空、≤128 字节、无控制字符）约束，因此 512 条仍是可校验的有限声明。
- 预算校验语义不变：`capabilities` 与 `required_capabilities` 各自不超过预算，二者并集去重后
  不超过预算；单连接模式不再使用普通连接的 64 条上限。
- 方法目录数量不复制进规格文档，避免文档与实际目录漂移；以 `info.capabilities` 和
  `crates/model` 的常量为准。
- 预算与目录的关系由测试固定：`crates/api` 的测试断言当前目录仍留有至少一个条目的余量，
  下次目录增长到贴着预算时会先失败在测试里，而不是在握手阶段。

## 结果

单连接客户端回显整份目录的 hello 重新可通过。提高预算只放宽拒绝阈值，不改变协商结果、
订阅预算、每组的四个 worker、入站队列容量或出站字节预算。普通四通道连接的 64 条上限不受影响。
