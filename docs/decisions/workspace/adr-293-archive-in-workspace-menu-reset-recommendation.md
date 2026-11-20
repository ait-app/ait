---
id: ADR-293
status: accepted
date: 2026-11-20
amends:
  - workspace/adr-083-reset-workspace-to-origin-default.md
  - workspace/adr-084-workspace-primary-git-action.md
paths:
  - apps/mobile
---

# ADR-293：归档移入 Workspace 菜单，交付完成后主按钮推荐重置

- 关联：ADR-083、ADR-084

## 背景

ADR-084 让右上角的 Git 主按钮在工作交付后推荐“归档工作区”，并在其下拉菜单中保留手动归档。归档处理的是
Workspace 本身，与提交、推送和 PR 无关；放在 Git 按钮里，非 Git 目录没有这个入口，交付后的默认动作也是
丢弃 Workspace，而托管 worktree 更常见的下一步是回到最新默认分支继续使用。

## 决策

1. Git 操作模型不再包含归档。右上角的主按钮及其下拉菜单、变更面板的 Git 菜单都不提供“归档工作区”。
2. 归档入口为 Workspace 自身的菜单：左侧栏的 Workspace 菜单，以及标题左侧的 Workspace 操作菜单（宽屏与紧凑布局
   各一个，归档是最后一项）。命令面板和快捷键保留。标题菜单与命令面板共用同一个入口实现，托管 worktree 的风险
   确认沿用 ADR-084 第 7 条：优先使用实时 checkout 状态，缺失时使用目录中的 Git 摘要，两者都未知时入口禁用。
   入口不再依赖 Git 状态，非 Git Workspace 同样可以归档。
3. ADR-084 第 1、5、6 条判断“工作已交付”的条件不变，满足时主按钮推荐“重置工作区”，流程变为
   “提交工作区”→“创建 PR”→“推送”→“查看 PR”→“重置工作区”。这修订 ADR-083 第 2 条中“重置不作为默认主按钮”
   的结论；重置前的确认提示、服务端校验和适用范围不变。
4. 只有重置可以执行时才推荐：托管 worktree、已知创建时分支名、存在 origin。其余 checkout 在同一状态下没有
   主按钮，右上角显示菜单入口；空闲或仍有活动 Agent 的 Workspace 同样不推荐重置。

## 后果与验证

交付后的 Workspace 不再被默认引导去归档，归档需要从 Workspace 菜单手动执行；主机设置中的合并后自动归档不受影响。
客户端测试覆盖推荐重置的条件、不可重置时不推荐、Git 操作中不再出现归档、标题菜单的归档项及其禁用与进行中
状态，以及命令面板的归档入口。
