---
id: ADR-294
status: accepted
date: 2026-11-20
amends:
  - daemon/adr-106-concrete-file-persistence.md
  - daemon/adr-112-persistence-crate.md
paths:
  - crates/persistence
  - crates/model/src/creation.rs
  - bins/daemon
---

# ADR-294：创建回执按键存入 SQLite

## 背景

旧版将创建回执存为 `creations/receipts.json`。回执中的原始创建请求可以包含图片。
通用 JSON Registry 引入 32 MiB 上限后，升级可能因既有文件超限而无法启动。
每次进度更新重写整个历史文件也会放大写入成本。

## 决策

- 创建回执适配器仍位于 `persistence`，允许此 crate 使用 `rusqlite`。daemon 继续通过
  `ReceiptStore` 注入存储。通用 JSON Registry 延续现有大小限制。
- `creations/receipts.sqlite3` 按回执 ID 保存原始意图、最新进度和重试标记。
  进度更新只提交目标记录，启动恢复认领时只读取进度列。
- 首次初始化时逐条读取旧 JSON，在同一个 SQLite 事务中导入记录并提交数据库版本。
  导入失败时回滚，并允许下次启动重试。旧 JSON 保留原字节，作为升级前的备份。

## 后果

历史记录的总体积不再受通用 JSON Registry 上限约束。回执写入仍受磁盘容量和 SQLite
限制。降级后的旧版只读写 JSON 备份，升级后的 SQLite 不会合并降级期间产生的新回执。
