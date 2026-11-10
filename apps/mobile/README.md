# Ait Mobile 与 Web

Expo / React Native 前端，直接 TCP 连接使用独立 Rust `daemon` 后端。浏览器、Electron 和
原生客户端共享 Rust 协议适配器，开发依赖由仓库根 npm workspace 管理。

## 本地 Web 开发

从仓库根目录运行：

```sh
npm ci
export AIT_SERVER_TOKEN="$(openssl rand -hex 32)"
npm run dev:mobile
```

打开 `http://localhost:8081`，添加直接连接：Host `127.0.0.1`，端口 `7316`，访问令牌填写
当前 `AIT_SERVER_TOKEN`。macOS 可在同一终端用 `printf %s "$AIT_SERVER_TOKEN" | pbcopy`
复制令牌。不要把令牌放进 URL 或 `EXPO_PUBLIC_*` 环境变量。

入口先构建共享包与 Rust binary，再启动 daemon 和 Expo；Ctrl+C 同时停止两者。
daemon 默认数据目录为 `.tmp/app/server`，可用 `AIT_SERVER_DATA_DIR` 覆盖。支持
`AIT_SERVER_BIN`（已有 binary）、`AIT_SERVER_LISTEN` 和 `EXPO_PORT`。若修改服务端口，
连接表单也须填写对应端口。`npm run web --workspace=@ait/mobile` 是同一入口。

## 分别启动

```sh
cargo run -p daemon --bin daemon -- \
  --data-dir .tmp/app/server --listen 127.0.0.1:7316 \
  --web-origin http://localhost:8081 --web-origin http://127.0.0.1:8081
```

另一个终端：

```sh
npm run build:ui-deps
npm run web:expo --workspace=@ait/mobile -- --localhost --port 8081
```

服务端优先读取 `AIT_SERVER_TOKEN`；未设置时复用数据目录内自动生成的 `local-api.token`。浏览器先用 Bearer 换取 30 秒有效的一次性连接票据；
每次重连都会重新换票。页面来源必须匹配 `--web-origin`，原生客户端不需要这个参数。
配置也可写入 server 的非秘密 TOML：`web_origins = ["http://localhost:8081"]`。

## 原生与桌面

`npm run ios --workspace=@ait/mobile` / `npm run android --workspace=@ait/mobile` 构建
共享依赖后启动对应原生工程。后端仍需单独启动。iOS simulator 可直接访问宿主 loopback；
Android emulator/device 可先运行 `adb reverse tcp:7316 tcp:7316`，再连接 `127.0.0.1:7316`。
Android 和 iOS 只作为客户端，登录同一账号后连接在线工作主机；在线业务使用 Hub，文件下载走独立 Relay。
登录入口、服务地址、凭据持久化和主机授权统一见[账号登录与主机上线](../../docs/operations/authing-client-login.md)。
daemon 自行维护机器授权与租约，退出移动客户端不会撤销主机授权。

Android 与 iOS 切后台后，账号连接和下载最多保留 2 分钟，随后暂停账户活动；
回前台重新验证账户节点并恢复连接。仅进入 `inactive` 不主动断开。系统可提前挂起进程或断网，
这个窗口不保证后台执行；后台冷启动直接暂停账户活动。下载写入手机缓存，完成后打开系统分享面板。
独立 Web 开发可直接连接本地 daemon；生产网页仅提供在线服务连接，见下文。

Android 和 iOS 的消息输入框附件菜单支持 **拍照（Take photo）**：授权相机后使用系统相机拍摄，
确认照片后加入待发送图片附件；取消拍摄不修改草稿。相机权限被拒绝后，可在系统设置中重新开启。
相机权限说明随原生构建生效，需要重新构建应用。
已在 Pixel 7 Pro 真机验证授权、拒绝后重试、拍摄确认、图片预览、取消与移除附件。

新增原生依赖或更新 Android 账户 WebSocket 插件后，需要重新生成原生工程再构建 APK：

```sh
cd apps/mobile
APP_VARIANT=production npx expo prebuild --platform android --no-install
```

Electron 使用 `npm run dev:desktop`，由主进程负责 Rust 服务启动及 Bearer 注入。

## 验证与构建

通知在前台使用本地提醒；后台由桌面内置 daemon 默认交给 Hub 投递至 Apple APNs／Google FCM，独立 daemon 可显式启用 Hub 或使用本机凭据直发。App 通知设置支持系统授权和本地测试；凭据配置、iOS Bundle ID 白名单及真机验收见[移动端通知](../../docs/operations/mobile-notifications.md)。更新此能力需要重新构建安装包并重启对应运行时。

iPhone 构建从仓库根运行 `npm run build:ios`（未签名真机归档）、
`npm run build:ios:simulator`（Release 模拟器 App）或 `npm run build:ios:ipa`（签名 IPA）。
证书、Bundle ID 和导出配置见 [Apple 构建说明](../../docs/operations/apple-builds.md)。

```sh
npm run typecheck --workspace=@ait/mobile
npm run test --workspace=@ait/mobile -- --project unit
npm run build:web --workspace=@ait/mobile
APP_BROWSER_UI=1 node scripts/validate-app-rust-browser.mjs
```

最后一条从仓库根运行，需要已编译的 `target/debug/daemon`、Web 导出和 Playwright Chromium；
可用 `AIT_SERVER_BIN` 指定 binary。测试使用临时数据目录，覆盖实际浏览器鉴权、SDK RPC、
重连、错误令牌、页面连接及刷新恢复，结束后清理。

原生账户边界见 [ADR-076](../../docs/decisions/clients/adr-076-android-account-relay.md) 和
[ADR-084](../../docs/decisions/clients/adr-084-ios-account-relay.md)。
直接连接边界见 [ADR-049](../../docs/decisions/clients/adr-049-app-rust-browser-transport.md) 和
[实施报告](../../docs/archive/reports/clients/app-rust-daemon.md)。

## 网页客户端部署

网页客户端以独立静态镜像部署在网站根路径 `/`，与 ait-server 同源：网关保留 `/api`、
管理控制台 `/dash` 及登录授权页，其余路径交给本镜像。客户端共享网站登录和同源 `/api`，
网页仅提供在线服务连接；未登录时跳转 `/login?client=1`，登录后回到 `/`。
刷新与浏览器重启保留有效登录，退出或会话失效时清理。管理功能只在 `/dash`，客户端不内嵌。

```sh
npm ci
npm run build:web-console --workspace=@ait/mobile   # 导出至 apps/mobile/web-console/dist/
docker build --tag ait-web-client apps/mobile/web-console
docker run --rm -d --name ait-web-client -p 127.0.0.1:8080:80 ait-web-client
bash apps/mobile/web-console/check-routes.sh http://127.0.0.1:8080
```

导出使用 `EXPO_PUBLIC_AIT_CONSOLE=1`、不设 `AIT_WEB_BASE_PATH`，并写入 `client-build.json`
记录版本与提交。镜像只托管静态文件：深链回退 `index.html`，`/_expo/static/` 等带哈希资源长期缓存，
`/healthz` 用于健康检查。发布标签会推送 `ghcr.io/ait-app/ait-web-client:<tag>` 和 `sha-<commit>`。
本地单独运行镜像时没有 `/api` 与登录页，只用于检查静态路由。
边界见 [ADR-132](../../docs/decisions/clients/adr-132-web-console-client.md) 和
[ADR-188](../../docs/decisions/clients/adr-188-standalone-web-client-image.md)。
