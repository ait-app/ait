# Ait 0.1.12 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.11 的 BUILD-INFO.json 与 GitHub digest 已核对，实际来源为
`681133e146e2ea99c2063d0a25c997d95d781519`。本次固定 main 基线
`aa79a9997c5489654d7459daf418d35a3724bbbc`，纳入 PR #401、#402、#403、#404。
最终标签 v0.1.12 的成品来源以 BUILD-INFO.json 核对。

本次范围包含桌面客户端、两个独立 daemon、COS、官网和服务端下载入口、iOS TestFlight，
以及主 release 完成后的 Android 本地 AAB 构建与 Google Play 封闭测试。
按用户要求，GitHub 主 Release 完成后暂停，确认后继续 COS、下载入口和移动发布。
不提交 App Store 正式审核，不变更 Play 测试者或地区。

## 更新内容

- xterm 对 React Native 缺失的 navigator 浏览器字段作兼容处理，避免首屏前崩溃。
- 在其他应用模块加载前安装 navigator、crypto 和 screen.orientation polyfill。
- 通过 Expo 配置插件采用 iOS Scene 生命周期，兼容 Xcode 27 与 iOS 27；保留链接、前后台事件和粘贴初始化。
- 移除独立 APK GitHub Actions 工作流，保留商店 AAB 和 iOS 的独立发布入口。
- 修正已与 xterm 补丁冲突的旧测试：直接断言 navigator 初始化契约，再加载真实依赖；不再要求已修复的依赖抛错。

## 发布准备与验证

测量提交为 `c939e377d9b618bd067b32743e48cb23b91c7340`。
15 个 Cargo 包、7 个 npm 包、锁文件及 PKGBUILD 同步为 0.1.12；第三方依赖未变化。

- `npm ci`、`npm run verify:release -- v0.1.12`、`npm run test:release`（60 项）通过。
- 移动发布契约测试 40 项通过，包含 Scene 插件 6 项及 EAS 身份、版本和提交流程检查。
- 两个启动测试文件共 12 项通过；修正前 11 项通过、1 项因过时断言失败。
  临时将 navigator polyfill 改为空实现后，显式字段断言失败；恢复原实现后通过。
- `npm run build:desktop-main`、桌面与共享界面类型检查、改动文件格式和 UI lint 通过。
- 不含 bins/crates Rust 实现改动，按 AGENTS.md 跳过本地 Rust 测试与覆盖率。

aa79a999 的独立原生验证已在 iOS 26.5 与 27.0 的 iPad mini 模拟器各完成三次冷启动，
运行中链接打开设置、退后台返回均通过，未发现匹配的新崩溃报告。
该结果验证修复基线，不替代 0.1.12 新商店二进制或真机验收；登录和推送尚未验收。

正式公开前还须等待准备 PR 与 main 的适用 CI、Linux/macOS 构建、签名公证和成品启动门禁，
下载核对全部附件、SHA256SUMS、GitHub digest、更新 SHA-512 与 BUILD-INFO。
本报告记录准备证据，实际在线状态由 Release Notes 和跨仓库交付记录补充。

## Test coverage

本次实际测量 Scene 配置插件 `apps/mobile/plugins/with-ios-scene-lifecycle.js`：
行覆盖率 **86.60%（84/97 行）**，分支 93.33%，函数 75.00%。未覆盖 81–93 行的 Expo 插件包装入口。
[共享 JSON 证据](release-0.1.12-coverage.json)包含精确命令、源码 SHA-256、测试结果和限制。
命令在仓库根执行：

```sh
node --experimental-test-coverage \
  --test-coverage-include=apps/mobile/plugins/with-ios-scene-lifecycle.js \
  --test apps/mobile/plugins/with-ios-scene-lifecycle.test.cjs \
  apps/mobile/scripts/app-identity.test.cjs apps/mobile/scripts/eas-submit-tracks.test.cjs \
  apps/mobile/scripts/ios-testflight-workflow.test.cjs scripts/ios-testflight-release.test.mjs \
  scripts/android-release.test.mjs scripts/android-play-release.test.mjs
```

这是 JavaScript 配置插件的定向覆盖率，不代表 TypeScript、Swift、Rust 或整个产品。
TypeScript 启动路径以隔离运行时的真实依赖加载与终端解析测试验证，未测量行覆盖率。
没有同范围可比基线，不计算历史变化。若后续仅增加报告，以证据中的源码指纹核对等价关系。
