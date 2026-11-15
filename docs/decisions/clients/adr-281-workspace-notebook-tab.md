---
id: ADR-281
status: accepted
date: 2026-11-16
paths:
  - apps/mobile
  - docs/architecture/workspace.md
---

# ADR-281：可分享的 Workspace 记事本标签页

## 背景

Workspace 已经支持按 Workspace 分享和文件实时订阅。需要一个能记录协作笔记与 TODO 的标签页，且不为这项能力增加新的 daemon 协议或独立存储服务。

## 决策

- 客户端新增 `notebook` Workspace tab target。它是每个 Workspace 唯一的主面板标签页，入口由现有 `experimentalFeatures` 测试开关保护，默认不显示。
- 记事本内容保存为 Workspace 根目录的 `.ait-notebook.md`。面板使用现有文件读取、订阅、创建和带 `modifiedAt`/`revision` 条件的写入接口，因此 Workspace 分享后协作者读取同一份内容。
- 记事本采用 Markdown 文本，可同时记录普通笔记和 `TODO` 清单；不引入新的内容格式或服务端模型。
- 共享 Workspace 的访客沿用现有文件权限。写入被拒绝时面板转为只读提示；并发写入冲突保留本地编辑状态并提示用户重新确认磁盘版本。

## 结果

记事本 tab 的布局仍属于每个客户端，分享 Workspace 共享的是 `.ait-notebook.md` 内容。关闭测试开关后，已恢复的 tab 不会丢失，面板显示功能未启用提示。
