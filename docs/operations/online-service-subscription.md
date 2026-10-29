# Online Service App Store 订阅

日期：2026-10-30。商品为 `monthly`（6822790958），订阅组 `22477177`，周期一个月，美国区 $4.99；仅解锁在线连接。

## 代码与职责

- `apps/mobile/modules/ait-storekit`：StoreKit 2 商品、购买、交易更新、恢复与完成交易；iOS 16+。
- `apps/mobile/src/billing`：账号绑定的购买交付协调、全局前台补验与会员卡。设置、欢迎页和添加在线连接共用账号面板。
- `packages/client/src/account-session.ts`：使用当前账号凭据读取会员、提交交易编号，拒绝切换账号后的迟到结果。
- 本地 `ait-server`：独立会员表、Apple API/JWS 验证、通知去重、周期对账与 Hub/Relay 准入。

App Store 生产 bundle 为 `dev.ait.mobile`，App ID 为 `6817043477`。默认 development build 使用 `dev.ait.mobile.debug`，不能获取生产 App 下的真实商品；沙盒或 TestFlight 测试必须使用生产 bundle。Xcode 本地 StoreKit 配置不是 Apple 服务端可查询的交易，端到端验收需真实 Sandbox/TestFlight。

## 部署次序

1. 在 `ait-server` 配置 In-App Purchase key 的 issuer ID、key ID 和 PEM 私钥；仅放在服务端环境/密钥管理中。具体变量、迁移和通知配置见服务端 `docs/online-service-subscriptions.md`。
2. 先部署迁移和代码，保持 `ONLINE_SERVICE_MEMBERSHIP_REQUIRED=false`。准备好新 App 与审核方案后再开启；旧客户端同样会被中心的连接校验拦截。
3. 独立测试中心可启用 `APP_STORE_ALLOW_SANDBOX=true`。生产应保持 false，只通过 `APP_STORE_SANDBOX_ACCOUNT_IDS` 允许专用 App Review 账号。不要让普通生产用户使用沙盒权益。
4. App Store Connect 设置生产/沙盒通知为 V2；向对应环境发送 Test Notification 并检查 Apple 的通知历史。第一份订阅随包含购买界面的新版本提交。
5. App 默认仅在 `https://aitapp.cn/api` 提供购买。使用独立测试中心时，构建时设置 `EXPO_PUBLIC_APP_STORE_CENTER`，并在登录时选择同一中心。购买页隐私政策为 <https://ait-app.com/privacy/>，条款使用 Apple Standard EULA。

## 真机验收

使用专用 Ait 账号和 Apple Sandbox tester，在生产 bundle 的设备构建/TestFlight 上依次检查：

- 未订阅可登录、查看在线 Host、看到本地化月价格；Hub/Relay 业务连接被拒绝；本地连接可用。
- 购买、取消、待批准、购买后立即断网/杀进程；重新打开后补验并获得在线连接，未保存前不结束交易。
- 删除/重装 App 后恢复；切换另一 Ait 账号恢复应提示原账号，不能转移购买。
- 加速续费、取消续费后剩余周期、到期、扣款宽限期/重试、退款；核对中心会员期限及已有连接的关闭。
- 查看、取消订阅后返回 App 更新状态；删除 Ait 账号前能看到 Apple 订阅需单独取消的提示。

## 验证范围

实现包含购买协调器、会员卡、共享客户端、数据库权益/归属/时序与真实 HTTP/Hub 退款断链测试。证书测试使用隔离的测试 CA，生产只固定 Apple Root CA G3。未配置真实 Apple 私钥之前，测试不代表真实 Apple Sandbox 已联通。

本次未修改 `bins/` 或 `crates/` 中的 Rust，按仓库规则不运行主仓库 Rust 测试；只运行独立服务端的相关测试。未测覆盖率：本次未请求覆盖率统计。

本次验证记录：移动端 28 项、共享账号客户端 93 项、桌面账号 IPC 13 项、服务端会员 9 项及 HTTP/真实 Hub/Relay 3 项专项测试通过。iOS StoreKit Pod 的 arm64 模拟器编译、移动端/桌面端类型检查、变更文件 oxlint/oxfmt、服务端 rustfmt 和 Clippy（含测试编译）通过。

全仓库文档链接检查会扫描已安装的 Pods，发现 libwebp 自带 README 的四处缺失文件；变更文档已单独核对链接。这些供应商文档未修改。
