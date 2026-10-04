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

服务端从环境读取 `AIT_SERVER_TOKEN`。浏览器先用 Bearer 换取 30 秒有效的一次性连接票据；
每次重连都会重新换票。页面来源必须匹配 `--web-origin`，原生客户端不需要这个参数。
配置也可写入 server 的非秘密 TOML：`web_origins = ["http://localhost:8081"]`。

## 原生与桌面

`npm run ios --workspace=@ait/mobile` / `npm run android --workspace=@ait/mobile` 构建
共享依赖后启动对应原生工程。后端仍需单独启动。iOS simulator 可直接访问宿主 loopback；
Android emulator/device 可先运行 `adb reverse tcp:7316 tcp:7316`，再连接 `127.0.0.1:7316`。
Android 支持账户登录与中继：首次启动点击欢迎页的 **Online Service（在线服务）**；
也可打开 **Settings → App → Online Service（应用 → 在线服务）**，或在 **Add Host → Online Service** 二级页面登录。
使用与电脑桌面应用相同的账户登录，再选择在线电脑。工作区、Agent、终端和文件
复用 Rust 单连接协议；下载写入手机缓存文件，完成后打开系统分享面板。
登录凭据保存在 Android 安全存储中，退到后台暂停连接，回到前台重新验证账户节点。
手机不运行 daemon，也不作为工作主机出现在列表里。iOS 和浏览器仍使用直接连接。
要让电脑上线，在其 **Host → Connections → Sync with online service（主机 → 连接 → 与在线服务同步）** 点击连接。
停止同步只影响当前主机。客户端退出账户只释放自身节点与绑定 daemon 的租约，其他 daemon 保留原账户授权并继续续租，退出登录后仍可在对应主机中主动停止。
应用运行期间负责这些主机的续租；关闭应用不主动撤销远程租约，但进程停止或授权失效后无法续租。重新启动应用后需再次启用同步；旧 daemon 需更新后才能使用此入口。
已在实体 Android 设备验证欢迎页账户入口与邮箱/密码登录表单；真实账户到远程电脑的完整
中继流程尚未完成真机验收。

新增原生依赖或更新账户 WebSocket 插件后，需要重新生成原生工程再构建 APK：

```sh
cd apps/mobile
APP_VARIANT=production npx expo prebuild --platform android --no-install
```

Electron 使用 `npm run dev:desktop`，由主进程负责 Rust 服务启动及 Bearer 注入。

## 验证与构建

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

Android 账户边界见 [ADR-076](../../docs/decisions/clients/adr-076-android-account-relay.md)。
直接连接边界见 [ADR-049](../../docs/decisions/clients/adr-049-app-rust-browser-transport.md) 和
[实施报告](../../docs/reports/clients/app-rust-daemon.md)。
