# Online Service 双平台订阅

日期：2026-10-30。商品为 `monthly`（6822790958），订阅组 `22477177`，周期一个月，美国区 $4.99；仅解锁在线连接。

## 代码与职责

- `apps/mobile/modules/ait-play-billing`：Google Play Billing 9.1，月付基础方案、本地化价格、购买及恢复；服务器确认付款。
- `apps/mobile/modules/ait-storekit`：StoreKit 2 商品、购买、交易更新、恢复与完成交易；iOS 16+。
- `apps/mobile/src/billing`：账号绑定的购买交付协调、全局前台补验与会员卡。设置、欢迎页和添加在线连接共用账号面板。
- `packages/client/src/account-session.ts`：使用当前账号凭据读取会员、提交交易编号，拒绝切换账号后的迟到结果。
- 本地 `ait-server`：独立付款账本、Apple API/JWS 与 Google Developer API 验证、通知去重、周期对账与 Hub/Relay 准入。

App Store 生产 bundle 为 `com.necokeine.ait`（与 Xcode Cloud 发布包一致），App ID 为 `6817043477`；中心的 `APP_STORE_BUNDLE_ID` 必须相同。默认 development build 使用 `dev.ait.mobile.debug`，不能获取生产 App 下的真实商品；沙盒或 TestFlight 测试必须使用生产 bundle。Xcode 本地 StoreKit 配置不是 Apple 服务端可查询的交易，端到端验收需真实 Sandbox/TestFlight。

## 部署次序

1. 在 `ait-server` 配置 In-App Purchase key 的 issuer ID、key ID 和 PEM 私钥；仅放在服务端环境/密钥管理中。具体变量、迁移和通知配置见服务端 `docs/online-service-subscriptions.md`。
2. 先部署迁移和代码，保持 `ONLINE_SERVICE_MEMBERSHIP_REQUIRED=false`。准备好新 App 与审核方案后再开启；旧客户端同样会被中心的连接校验拦截。关闭期间，只要中心已配置对应商店的验单，App 仍显示购买入口，用于内测、审核和提前订阅，已购时长照常累计。
3. 独立测试中心可启用 `APP_STORE_ALLOW_SANDBOX=true`。生产应保持 false，只通过 `APP_STORE_SANDBOX_ACCOUNT_IDS` 允许专用 App Review 账号。不要让普通生产用户使用沙盒权益。
4. App Store Connect 设置生产/沙盒通知为 V2；向对应环境发送 Test Notification 并检查 Apple 的通知历史。第一份订阅随包含购买界面的新版本提交。
5. App 默认仅在 `https://aitapp.cn/api` 提供购买。使用独立测试中心时，构建时设置 `EXPO_PUBLIC_BILLING_CENTER`（兼容旧 `EXPO_PUBLIC_APP_STORE_CENTER`），并在登录时选择同一中心。购买页隐私政策为 <https://ait-app.com/privacy/>，iOS 条款使用 Apple Standard EULA，Android 使用 Google Play 条款。

## 真机验收

使用专用 Ait 账号和 Apple Sandbox tester，在生产 bundle 的设备构建/TestFlight 上依次检查：

- 未订阅可登录、查看在线 Host、看到本地化月价格；Hub/Relay 业务连接被拒绝；本地连接可用。
- 购买、取消、待批准、购买后立即断网/杀进程；重新打开后补验并获得在线连接，未保存前不结束交易。
- 删除/重装 App 后恢复；切换另一 Ait 账号恢复应提示原账号，不能转移购买。
- 加速续费、取消续费后剩余周期、到期、扣款宽限期/重试、退款；核对中心会员期限及已有连接的关闭。
- 查看、取消订阅后返回 App 更新状态；删除 Ait 账号前能看到商店订阅需单独取消的提示。

## 验证范围

单次验证证据写在 PR 描述中。证书测试使用隔离的测试 CA，生产只固定 Apple Root CA G3；未配置真实商店凭据前，自动化测试不代表 Apple Sandbox 或 Play 已联通。

## Google Play 配置

生产包名 `dev.ait.mobile`，商品 ID `monthly`，自动续订基础方案 ID `monthly`，周期 P1M，美国区 $4.99。客户端默认使用该组合，可通过 `EXPO_PUBLIC_GOOGLE_PLAY_PRODUCT_ID` / `EXPO_PUBLIC_GOOGLE_PLAY_BASE_PLAN_ID` 覆盖，必须与中心一致。客户端只购买无优惠、非免费、单阶段无限月续费方案；价格从商店返回。

当前 Play Console 已启用 `monthly / monthly`，美国区 $4.99/月，174 个当前国家/地区可用，不自动加入未来新增地区；中英文权益均只解锁 Online Service。基础方案为自动月续费、无优惠，宽限期 7 天、自动冻结期 53 天；关闭商店外重新订阅，要求回 App 登录后绑定账号购买。

仅上传草稿时，订阅页只显示“上传新 APK”；须先把含 Billing 权限的版本发布到[内部测试轨道](google-play-internal-testing.md)才能创建订阅。服务端需要独立的 Developer API 账单权限与 Pub/Sub RTDN 配置；CI 测试轨道上传权限不能代替账单权限。密钥仅存中心，见服务端部署说明。

Google Play 真机验证使用生产包名、从 Play 安装的测试版本和 license tester。检查待付款不授予权益、后台补验、服务端 acknowledge 失败重试、恢复、续费、退款与 RTDN。原账号已订阅 Apple 时仍能在 Android 购买，反之亦然。

## 累计规则

每笔已验证付款增加一个 UTC 自然月，从付款时间和累计到期时间较晚者起算，月末夹取。两家同时付款累计两个月，双方每月续费再增加两个月。恢复和通知幂等；全额退款撤销该笔时长，部分退款按累计已成功退款金额/原实付金额缩减对应自然月跨度，重放时保留其他付款，精确到微秒。取消续费、暂停和商店到期均不清空已付余额。商店宽限期是临时访问，不作为付费加月基数。购买页分别显示商店订阅状态与累计期限，管理/取消仍需分别到各商店。
