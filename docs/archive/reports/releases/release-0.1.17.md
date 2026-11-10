# Ait 0.1.17 发布说明与验证

## 发布范围与源码

上一正式版 v0.1.16 附件 BUILD-INFO.json 的实际来源为 `efa428c27791587502b3d06e609344597c590cfe`。
本次 main 基线 `0c1a9524bc5f43d99020a338f16e9f1723b54359`，纳入 #471、#473、#474、#476、#477、#434 等已合并功能与修复。
准备期间 main 新增 `c2962a9518e7cf8c6e30e684f10be7b7eaa49731`（#478），本次一并纳入；覆盖率目标文件保持相同指纹。
发布 Linux、macOS、Windows 桌面、Linux/macOS 独立 daemon，并由当前桌面工作流发布网页客户端镜像。
GitHub 正式 Release 完成并验证后按用户要求暂停；确认后继续 COS、官网及服务端下载入口。
本次不执行 iOS 或 Android 构建、测试分发与商店提交；已移除的 GitHub iOS 入口不是本次发布目标。

## 更新内容

- 自动合并连续 Thinking 块，减少消息中的重复分组。
- 工作区创建幂等键绑定实际提交意图，重试保持稳定，避免不同请求混用。
- 模型选择器单独管理 Provider 加载状态，避免其他请求干扰。
- Oh My Pi 按模型发现能力，修复控件图标、标签和本地化。
- Codex Provider 请求超时延长到 20 秒。
- 移除 Pi 无实际作用的合成权限模式，兼容已保存的 default 输入。
- 网页客户端改为根路径独立静态镜像，移除管理 tab。镜像由 ait-server 后续部署；发布镜像不表示生产上线。

## 发布准备与验证

15 个 Cargo 包、7 个活跃 npm 包、锁文件、PKGBUILD 和 CHANGELOG 同步为 0.1.17。
版本准备不修改第三方依赖或 Rust 实现。

- `npm ci`、`npm run verify:release -- v0.1.17`、发布工具 67 项测试通过。
- 版本元数据相关 35 项测试、桌面主进程构建、桌面与共享 UI 类型检查通过。
- iOS Info.plist 为 0.1.17 / 1017999；使用生产身份运行 Expo prebuild，锁定 CocoaPods 1.16.2 生成 ExpoTwoWayAudio 0.1.17 的 Podfile.lock，并通过 `pod install --deployment`。
- 原生同步仅保留 plist 两个版本字段及本地 Pod 版本和 checksum；预生成产生的无关图标及格式变化已清理。Xcode Cloud 源码测试 3 项通过，未执行移动端二进制构建。
- 本地 Rust 测试与覆盖率按 AGENTS.md 跳过；[基线 main CI](https://github.com/ait-app/ait/actions/runs/40097620696)通过。
- 公开前继续检查准备 PR、最终 main CI、三平台发布构建、签名公证、成品启动及网页镜像构建和路由门禁，核对全部附件的 SHA256SUMS、GitHub digest、更新 SHA-512 与两个 BUILD-INFO 来源。

网页镜像目标为 `ghcr.io/ait-app/ait-web-client:v0.1.17`，源码标签与桌面一致。镜像发布和生产部署分别记录。
Windows 需要远程 Host，无内置 daemon，安装包未签名。未验证真实外部 Provider、生产浏览器客户端部署或移动端真机安装。

## Test coverage

本次重新测量三个 JavaScript 发布工具，覆盖 **285/312 行（91.35%）**、分支 **91/96（94.79%）**、函数 **22/23（95.65%）**。
测量基线 `0c1a9524bc5f43d99020a338f16e9f1723b54359`，准备工作区仅包含版本与文档修改；最终合并提交通过源码指纹核对等价关系。
范围为 release-assets.mjs、release-layout.mjs、release-version.mjs，不代表 Rust、TypeScript 或整个产品覆盖率。

```sh
node --experimental-test-coverage \
  --test-coverage-include=scripts/release-version.mjs \
  --test-coverage-include=scripts/release-assets.mjs \
  --test-coverage-include=scripts/release-layout.mjs \
  --test scripts/nightly-workflow.test.mjs scripts/release-assets.test.mjs \
  scripts/release-layout.test.mjs scripts/release-workflow.test.mjs \
  scripts/prepare-desktop-daemon.test.mjs scripts/verify-packaged-resources.test.cjs \
  scripts/edit-windows-executable.test.cjs
```

三个文件行覆盖率依次为 93.43%、67.86%、100%。未覆盖资源脚本 43、129–135、228–237 行、布局脚本 20–28 行，主要为 CLI 和失败路径。
与 0.1.16 同范围比较，目标文件指纹完全一致，覆盖行、总行和百分比变化均为零；数据为本次实际重新测量。
[JSON 证据](release-0.1.17-coverage.json)记录命令、计数、测量时间和源码指纹。
