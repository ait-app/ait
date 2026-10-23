# ADR-070：Ait iOS TestFlight 手动发布工作流

状态：Superseded（2026-10-23），由 [ADR-183](adr-183-ios-xcode-cloud-release.md) 取代。
以下保留原 EAS 方案的历史决策，不再作为当前发布操作依据。

## 背景

`apps/mobile` 此前只有两条 iOS 发布路径，均来自 Paseo 遗留的 `.eas/workflows/*`：
`release-mobile.yml` 在 `submit_ios_for_review` job 里直接调用
`bundle exec fastlane ios submit_review` 把构建提交 App Store 正式审核；
`release-ios-beta.yml` 用 `profile: production` 构建，并通过
`type: testflight` 的 `submit_beta_review: true` 把 TestFlight 分发同时提交外部测试组
审核；`resubmit-ios-review.yml` 同样调用 fastlane 重新提交审核。三者都没有显式的人工审核
门禁，都会在构建完成后自动触发某种形式的 App Store 审核提交，且都使用 `production`
profile，而不是本仓库实际维护的 Ait 应用身份。

与此同时，`apps/mobile` 的 Android 发布已经有一条独立、范围明确的 Internal Testing 路径
（见[发布操作指南](../../operations/releasing.md#google-play-android-internal-testing)）：
固定 `ait` profile、固定 `internal` 轨道、不触达正式商店。iOS 侧缺少对应的、范围同样克制
的内部测试发布方式：团队需要把真机构建发到 TestFlight 做验证，但不希望任何一次操作顺手
触发 App Store 正式审核，也不希望在本仓库引入新的长期 Apple 签名凭据。

`apps/mobile/eas.json` 已经有 `build.ait`、`submit.ait.android` 和 `submit.ait.ios`，其中
`submit.ait.ios` 固定了 Ait 的 App Store Connect app。仍需要给出一条独立于 Paseo 遗留工作流、
只做 TestFlight 分发、不触达正式审核的发布入口。

## 决策

- 新增 GitHub Actions 工作流 `.github/workflows/release-ios-testflight.yml`，只声明
  `workflow_dispatch` 触发，不声明 `push` 触发器，并用 `if: github.ref ==
'refs/heads/main'` 把执行限制在 `main` 分支。发布顺序固定为：先完成桌面
  [创建 Release](../../operations/releasing.md#创建-release)打出稳定 `vX.Y.Z` 标签并发布桌面
  GitHub Release，再手动触发本工作流，输入同一个已存在的标签。工作流不创建、不移动标签，
  也不在打标签或合并 `main` 时自动运行。
- 工作流绑定 GitHub Environment `ios-testflight`；在该 environment 配置 required
  reviewers 后，手动触发之外还需要审核人批准才会继续执行，构成第二道人工确认。
- 工作流只声明一个 GitHub Secret：`EXPO_TOKEN`，用于 `eas` CLI 的非交互身份验证。不新增
  任何 Apple ID、App Store Connect API key 或签名材料作为 Secret；iOS 签名证书、
  provisioning profile 和 ASC 凭据完全交给 EAS 托管，构建步骤显式传入
  `--freeze-credentials` 锁定复用已保存的签名配置，不触发新凭据生成。
- 构建与提交统一使用 `apps/mobile/eas.json` 已有的 `ait` profile：`build.ait` 延续已有的
  `com.necokeine.ait` 应用身份；`submit.ait.ios` 固定 `ascAppId`、`appleTeamId`、
  `bundleIdentifier` 三项，`appleTeamId` 与 `build.ait.env.APPLE_TEAM_ID` 保持一致。
- 工作流先用 `scripts/verify-release-version.mjs`（与桌面发布共用的同一脚本）校验输入标签
  是稳定 `vX.Y.Z` 形式且与仓库版本一致，再用 `gh release view` 确认该标签已有非 draft、
  非 prerelease 的正式 GitHub Release；任一校验失败即终止，不发起构建。
- 标签必须含有 `ait` profile；在读取 EAS 项目之前，工作流从标签源码的 `build.ait.env`
  加载公开项目标识供查询、构建和提交命令使用。既有 `v0.0.11` 标签不含该 profile，
  不可用于新工作流；首次自动化运行需要一个包含此配置的新稳定标签。
- 新增 `scripts/ios-testflight-release.mjs` 承载发布计划与重复构建判定的纯函数逻辑
  （`planRelease`、`decideBuild`），工作流通过子命令调用它，不在 workflow YAML 里内联等价
  判断逻辑。
- 重复构建判定：没有显式 `build_id` 输入时，先用 `eas build:list` 按 `ait` profile、
  应用身份和目标 iOS build number 查询现有构建。只要存在状态为 `finished`、`in-queue`、
  `in-progress`、`new` 或 `pending-cancel` 的匹配构建，立即判定为不安全重复并快速失败，
  报错带上已存在构建的 build ID；只有匹配构建全部是 `errored` 或 `canceled` 才允许新建。
- 显式提供 `build_id` 输入时跳过新建构建，改为校验该 build 的 profile、应用版本、iOS build
  number 与 `finished` 状态，校验通过才直接提交；用于构建已成功但提交步骤失败时的重试，
  不强制重跑整个构建。
- 工作流只调用 `eas submit --platform ios --profile ait --id <build-id>` 把构建送入
  TestFlight，不调用任何 App Store 正式审核提交命令，不安装、不调用 fastlane。运行结束
  在 `GITHUB_STEP_SUMMARY` 写明本次构建号与 build ID，并明确标注 App Store 审核未提交，
  需要人工在 App Store Connect 里另行操作。

## 后果

- 团队获得一条独立于 Paseo 遗留工作流、范围明确的 iOS TestFlight 内部测试发布入口，
  与 Android Internal Testing 在设计模式上对齐：固定 profile、固定人工触发、不触达正式
  商店审核。
- `apps/mobile/.eas/workflows/release-mobile.yml` 和 `release-ios-beta.yml` 不再响应标签推送，
  但与 `resubmit-ios-review.yml` 一样仍可手动触发。前者含 Fastlane 正式审核步骤，
  beta 工作流会请求外部测试审核；三者均不属于 Ait TestFlight 路径，不能替代本工作流。
- 仓库侧新增的唯一长期凭据是 `EXPO_TOKEN`；Apple 签名材料、ASC API key 均不进入仓库或
  GitHub Secrets，责任边界落在 EAS 账号和 Expo 项目 credentials 上。
- `ios-testflight` environment 的具体审核人名单由仓库 GitHub 设置维护，不记录在文档里；
  增删审核人不需要改动工作流本身。
- 本 ADR 只记录工作流设计本身，不代表该工作流已经在生产环境完整跑过一次成功发布；
  首次实际运行的记录应作为独立报告补充，不属于本次文档改动范围。

## 被否决的方案

- **复用 `release-mobile.yml` 并去掉 `submit_ios_for_review` job**：否决。该工作流使用
  `profile: production`，对应的是 Paseo 的应用身份和签名配置，而不是 Ait 当前维护的
  `com.necokeine.ait`；直接复用会让两套发布身份混在同一个 workflow 文件里，增加误触风险。
- **在 EAS workflow（`.eas/workflows/*.yml`）里新增一个 Ait 专用 TestFlight workflow**：
  否决。EAS workflow 的人工审核门禁能力弱于 GitHub Environment 的 required reviewers，
  且现有三个 `.eas/workflows` 文件已经证明这类 workflow 容易在后续修改中被误接上自动提交
  审核的 job；改用 GitHub Actions 工作流可以复用仓库已有的 Environment 审核机制，并与桌面
  `release.yml`、`verify-release-version.mjs` 等现有发布基础设施共享版本门禁逻辑。
- **继续依赖 fastlane 做 TestFlight 分发**：否决。EAS Submit 已经原生支持 TestFlight
  上传，不需要额外维护 Fastfile、Apple ID 密码或 fastlane session 这类仓库里原本就没有的
  凭据形态；新增 fastlane 依赖只会扩大凭据面，与“不新增 Apple 签名 Secret”的目标相悖。
- **构建完成后自动触发 TestFlight 外部测试组审核**（即保留 `submit_beta_review: true`
  这一类行为）：否决。本工作流的范围明确限定为“产物送达 TestFlight 供内部测试”，不应
  自动触发任何审核流程；是否、何时提交外部测试组或正式商店审核留给人工在 App Store
  Connect 里判断，保持与 Android Internal Testing 一致的“只做内部分发”语义。
