# Ait 0.1.22 发布说明与验证

## 发布范围与来源

上一正式版 0.1.21 的最终构建来源为 `7113431099355c59170bef9b825c92bd9b5d3f23`。
本次功能基线为 main `5bfc9d69447f4556eeda7524a861cd803cdb3f96`，纳入 #515、#517、#518、#516、#520、#521、#523。
发布 Linux/macOS/Windows 桌面、两个独立 daemon 及现有工作流的网页客户端镜像。
GitHub Draft 附件校验完成后暂停，收到继续指令后再公开 Release、同步 COS、官网和服务端下载入口。
本次不触发 iOS/Android 工作流；原生版本与本地 Pod 锁文件作为统一源码元数据同步。

## 更新内容

- daemon 统一负责机器授权、节点注册与续租，Hub/Relay 独立重连；Desktop 退出不停止节点发布。
- 本地 API token 跨重启复用，显式环境变量优先；旧启动脚本需移除 `--headless`。
- 内置编排技能自动分发，简化客户端在线连接状态和断开操作。
- 修复 OMP 交互阻塞与多行输入、MiniMax 新会话模型选择，并加入浏览器账户删除确认。

## 发布前修复与验证

[功能基线 CI](https://github.com/ait-app/ait/actions/runs/40838957745) 有 3 个 Rust 测试及 7 个桌面断言失败。
本地定向复现后确认旧测试仍依赖客户端注入 Hub 票据、临时 API token 和旧能力数量，与 ADR-201 不一致。
修复提交 `0227c080` 更新这些期望；Hub 事件与目录测试通过隔离的 API/连接夹具验证真实服务组装、日志与 WSS，完整 daemon 的授权交接和拒绝外部票据仍由原有集成测试验证。未改动产品实现。

最终版本元数据为 0.1.22：15 个 Cargo 包、7 个 npm 包、锁文件、PKGBUILD 和 iOS 0.1.22 / 1022999。

- `npm ci`、`npm run verify:release`、75 项 `npm run test:release`、37 项 `npm run test:mobile-release`、文档检查、格式检查、桌面主进程构建通过。
- 桌面账号、daemon 环境和生命周期定向测试 17 项通过、8 项按现有环境条件跳过。
- 下列 Rust 定向测试在修复及 0.1.22 元数据上 34 项通过；筛选范围之外的 321 项未执行。
- `cargo build --locked --workspace`、`cargo clippy --locked -p api -p daemon --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。
- 生产身份 Expo prebuild 生成 iOS 0.1.22 / 1022999，CocoaPods 1.16.2 的 `pod install` 和 `pod install --deployment` 通过；仅保留版本与本地 Pod checksum 更新。
- 三平台安装包、签名、公证及 GitHub Draft 附件的最终检查由发布工作流和发布工作区记录，全部通过后才交付 Draft。

```sh
cargo nextest run --locked -p api -p daemon -E 'test(connection::validation::tests) | test(capabilities::tests) | test(host::tests) | test(host::hub::tests::wire) | test(tests::hub_control)'
npm test --workspace=@ait/desktop -- src/daemon/account-session.test.ts src/daemon/rust-daemon-environment.test.ts src/daemon/rust-daemon.test.ts
```

## Test coverage

未测量（not measured）：本次未请求覆盖率，修复只更新既有测试及发布元数据。测试通过数不代表覆盖率；如需测量，使用 `cargo llvm-cov` 对上述 API/daemon 定向范围生成可审阅报告。未执行全量 Rust workspace 测试，也未进行移动设备、真实模型或本机已安装 Electron 的升级验收。
