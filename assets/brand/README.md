# AIT 飞鸟生产资产

采用 [ADR-051](../../docs/decisions/branding/adr-051-ait-brand-identity.md) 中的 04 飞鸟。

唯一可编辑源是 [`ait-mark.json`](ait-mark.json)：
包含两个折面、合并轮廓、小尺寸光学校正轮廓、路径化 AIT 字标、深浅色令牌及容器位置。
应用内 `AitLogo` 与导出脚本直接读取同一份数据。这里的 SVG、Logo PNG 和各端 PNG、
ICO、ICNS 都是生成产物，请修改源后统一生成，不单独修改某一端文件。

```sh
npm ci
npm run generate:icons
npm run check:icons
```

生成依赖固定版本的 `sharp`，无需字体、图像生成服务或额外图标转换程序。
`check:icons` 在相同依赖环境下重新生成并逐字节检查全部产物，缺失或过期时返回失败。

| 资产                              | 用途                                                          |
| --------------------------------- | ------------------------------------------------------------- |
| `ait-mark.svg`                    | 双色独立飞鸟，透明底                                          |
| `ait-mark-mono.svg`               | 深炭色单色轮廓                                                |
| `ait-mark-small.svg`              | 小尺寸单色轮廓，16–32 px favicon                              |
| `ait-lockup-horizontal.svg`       | 横向图形与路径化 AIT 字标                                     |
| `ait-lockup-stacked.svg`          | 纵向组合，欢迎与启动场景                                      |
| `logo.svg` / `logo.png`           | 浅色圆角图标与 README                                         |
| `logo-dark.svg` / `logo-dark.png` | 深色圆角图标与 README                                         |
| `apps/desktop/assets/`            | Electron 开发/发行 PNG、Windows ICO、macOS ICNS、Linux 尺寸集 |
| `apps/mobile/assets/images/`      | Expo 图标、自适应前景、通知、启动及 favicon 状态资源          |
| `apps/mobile/public/`             | Apple touch 与 PWA 安装图标                                   |

iOS 图标和 PWA 图标是不透明方图；系统负责遮罩。Android 前景和通知图标保留透明背景，
通知图标为纯白轮廓。PWA 与 Android 的飞鸟比例分别服从各自安全圆；桌面圆角不用于移动端母图。
所有品牌 SVG 都有对应的 `-dark.svg`，保持相同轮廓和留白。暗色主翼使用 `#FF704D`，
前向折面使用 `#FFA184`，字标与单色轮廓使用 `#F4F4F5`；圆角容器使用 `#18181B` 底色。
应用内 `AitLogo` 跟随应用主题（包括手动选择），显式 `color` / `wordmarkColor` 仍优先。
README 通过 `<picture>` 按浏览器主题选择 Logo；favicon 跟随浏览器系统主题，原生启动图使用系统主题。
根目录不再保留 Logo 副本。各端打包资源保留在各自的 assets、public 或原生资源目录。
运行蓝点、提醒绿点属于应用状态，不属于品牌配色。

干净的 Expo prebuild 会读取新资源。已有本地 iOS prebuild 可显式同步：

```sh
npm run generate:icons -- --sync-native
npm run check:icons -- --sync-native
```

这会刷新现有 `Images.xcassets` 内的应用图标、启动图及启动底色，并保留已有的启动图文件名。
仓库已跟踪的原生资源应一起提交；不额外提交本地构建产物。
Android 使用项目已有的 clean prebuild 构建入口。更换已安装应用的系统图标需要重新构建和安装；
Electron 分支图标缓存版本已经更新。Web 构建使用带内容哈希的 favicon。

实际产物和页面截图见 [落地报告](../../docs/reports/clients/ait-brand-rollout.md)。
