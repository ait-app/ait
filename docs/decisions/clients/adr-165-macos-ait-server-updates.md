# ADR-165：macOS 通过 AIT Server 检查与下载更新

- 状态：Accepted
- 日期：2026-10-19
- 关联：[ADR-053](adr-053-paseo-desktop-release.md)、[ADR-158](adr-158-standalone-daemon-release.md)

## 背景

macOS 安装包原先内嵌 GitHub updater provider，检查更新和下载均依赖 GitHub。
AIT Server 已提供兼容当前 electron-updater generic provider 的公开清单及版本化资源路由，
可将国内及未知地区的下载发送到 COS，其他已知地区发送到 GitHub。

## 决策

macOS 的 `mac.publish` 使用 generic provider，feed 固定为
`https://aitapp.cn/api/v1/updates/macos/`。稳定版和 beta 分别请求 `latest-mac.yml`、
`beta-mac.yml`；继续由现有客户端控制灰度、自动下载与安装前复核。
清单及下载无需登录，不随账号服务地址或登录状态变化。

AIT Server 返回原生 UpdateInfo，保留校验和、文件大小、发布日期、发布说明和
`rolloutHours`。资源地址为 `files/v{version}/{filename}`，服务端以 `307` 选择镜像；
客户端继续验证原始安装包摘要。COS 只支持单段 Range，显式设置
`useMultipleRangeRequest: false`，差分下载通过多次单段请求完成。
新旧 ZIP 和 blockmap 的版本路径均须保持可读。

根级 GitHub 发布配置继续用于 Linux。macOS 的平台配置只决定安装包内嵌的更新源及
builder 清单；现有 GitHub Release 工作流仍通过 `gh release upload/create` 上传两平台
产物。COS 镜像与 `latest-macos.json` 由 ait-realease 发布编排负责，在校验安装包上传后
更新通道指针，客户端不维护另一份清单转换逻辑。

## 后果

需要发布包含此配置的新客户端；旧版本继续通过内嵌的 GitHub provider 获取首次升级。
未发布通道返回 404，沿用现有“无可用更新”处理；网络或 503 错误继续显示为检查失败。
本次接入限服务端已提供接口的 macOS；Linux 沿用 GitHub 更新源。

验证覆盖 builder 实际解析出的应用更新配置、stable/beta 通道切换、JSON 清单解析、
版本化 ZIP 和新旧 blockmap 地址、单段 Range 配置，以及 404/503 的错误区分。
