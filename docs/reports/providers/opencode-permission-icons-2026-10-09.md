# OpenCode 原生权限图标：PR 验证

日期：2026-10-09。基线：`ec5e48aa868e44389e20017af07d2c1643888c18`（原生权限 PR #241）。
验证源码：`bd84f845c388380fa32646cd7cfd3f36bb269ad8`；平台：macOS。
对应反馈：OC-003，Allow / Ask / Deny 需要不同图标。

共享 UI 按原生 permission 值解析当前按钮及菜单选项的图标，桌面工具栏与移动端选择器保持一致。

| 原生值 | Lucide 图标 |
| --- | --- |
| allow | ShieldCheck（对勾盾牌） |
| ask | ShieldQuestionMark（问号盾牌） |
| deny | ShieldOff（关闭盾牌） |
| null（原生继承） | Shield（中性盾牌） |

OpenCode v1 / v2 通过同一个 feature select 展示这些原生值，本次仅修改 UI 呈现。

## Test coverage

- Rust：not applicable — no Rust behavior changed；按 AGENTS.md 跳过 Rust 测试与覆盖率测量。
- TypeScript 行覆盖率：not measured；本次为图标呈现修正，未新增覆盖率测量或比较基线。
  后续新增交互行为时再按对应范围测量；本报告不将通过的测试数量作为覆盖率。
- 已有相关测试：5 个文件、27 项通过（图标解析、控制按钮、模式、布局与特性辅助函数）。
- `tsgo --noEmit`、改动文件的 oxlint 与 oxfmt 检查通过；oxlint 为 0 errors / 0 warnings。
- `npm run check:docs`、`git diff --check` 通过。
- 本次未运行桌面安装包或移动端真机视觉复测，后续在实际会话中确认三种选项的显示。

验证命令：

```sh
npm run test --workspace=@ait/mobile -- src/agent-controls/icons.test.ts src/composer/agent-controls/control.test.tsx src/composer/agent-controls/mode.test.ts src/composer/agent-controls/layout.test.ts src/composer/agent-controls/utils.test.ts
npm run typecheck --workspace=@ait/mobile
npx oxlint apps/mobile/src/agent-controls/icons.ts apps/mobile/src/composer/agent-controls/index.tsx
npx oxfmt --check apps/mobile/src/agent-controls/icons.ts apps/mobile/src/composer/agent-controls/index.tsx
npm run check:docs
git diff --check
```
