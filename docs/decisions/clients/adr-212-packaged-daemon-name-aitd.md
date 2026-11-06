# ADR-212：打包的 daemon 可执行文件统一命名为 aitd

- 状态：Accepted
- 日期：2026-11-05
- 修订：[ADR-158](adr-158-standalone-daemon-release.md)、[ADR-163](adr-163-nightly-standalone-daemon.md)（归档内可执行文件名）

## 背景

同一个 Rust daemon 在发布产物中有两个名字：Linux/macOS 桌面安装包内为 `resources/bin/daemon`，
独立 daemon 归档内为 `ait-daemon`。两个名字都要在文档、签名和校验脚本中分别维护；
`daemon` 放进 `PATH` 也容易与系统程序冲突。

## 决策

- 桌面安装包与独立归档（正式版、beta、nightly）中的可执行文件统一命名为 `aitd`：
  桌面为 `resources/bin/aitd`（macOS 为 `Contents/Resources/bin/aitd`），归档内为 `aitd` 与 `LICENSE`。
- Cargo package 与 bin target 仍为 `daemon`，构建产物保持 `target/<profile>/daemon`。
  CLI 对外显示统一为 `aitd`：顶层与子命令的 `--help` 使用 `aitd`，`--version` 输出 `aitd VERSION`。
  桌面打包校验同步使用这一版本输出；独立归档工具仅校验版本号，仍可重建改名前的不可变标签。
- 归档文件名 `Ait-VERSION-daemon-<平台>.tar.gz` 不变，下载链接与 AUR、COS 同步流程不受影响。
- 协议标识不属于可执行文件名，保持不变：设备授权 `client_id` 与 MCP `serverInfo.name` 仍为 `ait-daemon`。

## 影响

- 从独立归档手动安装 daemon 的用户，升级到新版本后需把 systemd/launchd 等服务配置中的
  `ait-daemon` 改为 `aitd`。
- 桌面应用整包更新，新版本直接启动 `bin/aitd`，无需迁移。
- 打包校验只接受 `resources/bin/aitd`；残留的 `daemon` 或 `ait-daemon` 会使打包失败。
