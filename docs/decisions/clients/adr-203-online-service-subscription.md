# ADR-203：Online Service 订阅与账号认证分离

状态：已接受。日期：2026-10-30。

## 背景

App Store 与 Google Play 的 `monthly` 自动续订商品只购买 Online Service 在线连接权益。账号管理、登录、续费与恢复购买在未订阅或订阅过期时仍须可用。复用账号有效期会让用户无法登录续费；仅由 App 判断购买成功也无法阻止旧客户端继续连接。

## 决定

iOS 本地 Expo 模块封装 StoreKit 2；Android 本地模块封装 Google Play Billing。共享客户端向账号中心提交 Apple 交易编号/环境或 Play purchase token。中心独立验证商店交易，拥有会员账本和在线连接准入决定；daemon 不持有商店密钥或订阅期限。Play 在绑定账号并保存凭据后由服务端 acknowledge，失败保留待重试凭据且不增加时长。

账号中心生成稳定的 `appAccountToken`，购买时绑定当前 Ait 账号。恢复购买必须回到原账号，订阅链不在账号之间转移。Play 的 obfuscatedAccountId 使用同一稳定 UUID。App 仅在中心确认保存交易后调用 `finish`；待批准和未完成交易通过单一全局监听器及前台恢复补验。

订阅控制 Hub access 和旧 Relay 的业务连接，覆盖票据签发、兑换/恢复和现有连接的周期复核。Host serve、控制链路、在线发现、认证与账单 HTTP 接口保持可用。本地、TCP、SSH 连接不受该权益限制。

## 取舍与后果

两家商店可同时订阅。每笔已核实成功月付从 `max(当前累计到期时间, 付款时间)` 增加一个自然月；按 UTC 计算，月末夹取到目标月最后一天。交易按付款时间、商店、环境、交易 ID 确定排序；Apple transaction ID / Play order ID 去重，恢复和重复通知不重复加时。退款或撤销排除对应交易并重放账本，保留其他付款；取消续费、暂停或商店订阅到期不清空已购累计时长。两家同时付款一次累计两个月，每月两家均续费再累计两个月。

Apple 查询签名完整交易历史；Play 根据最新成功订单号查询各期订单，不能用不会随续费改变的 purchase token 作为付款 ID。Play token 只在中心加密保存。生产和测试交易分别计算余额，测试策略变更不能把测试时长混入生产余额。商店明确的扣款宽限期可暂时授予访问，但不计入付款或后续加月基数。

上线开关默认关闭，需先配置中心的商店密钥、Apple 通知 V2、Google Pub/Sub RTDN 和真机验收。生产默认拒绝沙盒权益；App Review 账号可以在中心单独加入沙盒账号名单。商品价格由当前商店返回，本地化显示，不在 App 中写死美元价格。

部署与验证步骤见 [Online Service 订阅](../../operations/online-service-subscription.md)。
