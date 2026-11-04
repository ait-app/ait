# 工作区标题字重验证

日期：2026-11-04。验证基于 `4b6c90bb77e5518f84d79b0c4d326751bebe5dbc` 的工作区修改。
环境：Linux x86_64、Node.js 26.10.0、npm 12.2.0、Vitest 4.1.7。

## 变更范围

共享 `ScreenTitle` 仅在桌面布局（`md`，宽度 720px 起）使用
固定字重 `"500"`，替代原先的 300。
桌面工作区标题及使用该组件的菜单、返回导航标题同步调整。
窄屏继续使用原先的 400；独立的 compact header 继续使用原先的 500。

标题保持既有界面字体与字号。标签页保持常规字重，使工作区标题获得适度强调。

已验证源码的 SHA-256：

```text
5bc0202b4b64da4b627a06a5ebd7b17804677440bfbc008cbcabbb074000644b  apps/mobile/src/components/headers/screen-title.tsx
```

## 验证结果

| 命令                                                                                                                                                                                                             | 结果                                                  |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| `npm run typecheck --workspace=@ait/mobile`                                                                                                                                                                      | 通过，无诊断                                          |
| `npm run test --workspace=@ait/mobile -- --project=unit src/appearance/apply.test.ts src/styles/theme.test.ts src/styles/unistyles-module-scope.test.ts src/screens/workspace/workspace-source-of-truth.test.ts` | 4 个文件、33 项测试通过，0 失败                       |
| `node_modules/.bin/oxfmt --check apps/mobile/src/components/headers/screen-title.tsx docs/reports/README.md docs/reports/clients/workspace-title-weight-2026-11-04.md`                                           | 通过                                                  |
| `node_modules/.bin/oxlint --deny-warnings apps/mobile/src/components/headers/screen-title.tsx`                                                                                                                   | 通过，0 告警、0 错误                                  |
| 在 `apps/mobile` 执行 `../../node_modules/.bin/eslint src/components/headers/screen-title.tsx`                                                                                                                   | 通过，无诊断                                          |
| `npm run check:docs`                                                                                                                                                                                             | 检查器 1 项测试通过；514 篇 Markdown 本地链接检查通过 |
| `git diff --check`、`git diff --cached --check`                                                                                                                                                                  | 通过                                                  |

单元测试验证外观设置应用、主题 token、样式初始化约束及工作区标题数据来源。

## 验证范围

未执行浏览器或原生设备视觉验收；单元测试不验证实际字体栅格化和文字截断。
未测量测试覆盖率。
