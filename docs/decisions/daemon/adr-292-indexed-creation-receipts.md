---
id: ADR-292
status: accepted
date: 2026-11-19
amends:
  - providers/adr-039-agent-timeline-provider-creation.md
  - daemon/adr-105-file-tools-and-startup-config.md
  - daemon/adr-106-concrete-file-persistence.md
  - daemon/adr-112-persistence-crate.md
paths:
  - crates/domain/src/creation
  - crates/domain/src/creation.rs
  - crates/model/src/creation
  - crates/model/src/creation.rs
  - crates/persistence/src/storage/creation
  - crates/persistence/src/storage/creation.rs
  - bins/daemon
---

# ADR-292：永久保留创建回执，按键读写

## 背景

Workspace 和 Agent 的创建回执用于重试、断线重连及资源 ID 认领。旧 JSON 数组永久保存原请求与最近一次结果，图片和附件会随请求再次存储。一次创建可能提交多个阶段，每次都要重写全部历史。历史文件超过通用 Registry 的 32 MiB 读取上限时，daemon 启动失败。按时间删除旧键会使旧客户端重试时再次执行创建。

## 决策

1. 修订 ADR-039 的回执格式和 ADR-105 第 4 条。每个创建键保留一行，包含原有的按类型区分的键标识、32 字节的请求指纹、资源认领、版本、初始 Agent 重试标记和无损 zlib 压缩的最新快照。终态快照保留客户端重放所需的完整返回值。指纹版本随数据库格式固定；对 JSON 值按类型与长度编码，对象按键排序后计算 SHA-256。
2. 修订 ADR-106 第 3、5、7 条和 ADR-112 第 1、5 条。协调器通过 `model::creation::ReceiptStore` 按键读写，`persistence::storage::creation` 通过 `rusqlite` 维护主键与两类资源认领索引。首次写入原子地检查键及资源冲突，更新只替换预期版本。提交成功后协调器才发布进度。启动读取库头和表结构，客户端查询只解压目标快照。
3. 首次启动时持有数据目录实例锁，在原目录逐条将旧 JSON 导入临时数据库。内存开销取决于单条记录大小。校验完整输入、提交事务并同步文件后，原子替换 `creations/receipts.json`，在 Unix 上同步父目录。沿用旧文件名使旧版在读取新库时显式报错。迁移中断后，下次启动从原件重做或打开已完成的新库。旧数据损坏时保留原件，报告路径、类别与位置并省略请求正文。

## 后果

回执总量随不同的幂等键及其压缩结果增长。每个键只存最新快照，后续重试和状态读取按索引访问。SQLite 空间不足时回滚本次写入，返回 `resource_exhausted`，已提交的资源认领继续有效。已执行的外部操作沿用原有的中断语义。

升级时需要可容纳临时库和事务日志的磁盘空间。旧版无法读取新格式；如需降级，应提前备份停机时的数据目录。恢复备份也会回退备份之后新增的资源和幂等保证。
