# ADR-158：Release 附带独立 daemon 归档

状态：已接受

[ADR-053](adr-053-paseo-desktop-release.md) 只发布桌面安装包，daemon 仅位于安装包内部。[无人值守 daemon](../../operations/headless-daemon.md) 面向没有桌面环境的 Linux/macOS 主机，此前只能从源码构建。

桌面 Release 工作流在每个平台复用同一次构建的 Rust `daemon`，额外发布 `Ait-VERSION-daemon-linux-x64.tar.gz` 与 `Ait-VERSION-daemon-macos-arm64.tar.gz`。归档内的可执行文件命名为 `ait-daemon`，避免放入 `PATH` 时与系统同名程序冲突；同时附带 `LICENSE`。打包前核对 `--version` 与 Release 版本一致。macOS 归档内的可执行文件使用与桌面相同的 Developer ID 证书和 entitlements 单独签名并公证。

正式版与 beta 的 Release 校验必须包含两个归档，并纳入 `SHA256SUMS`；nightly 不发布独立 daemon。Android 补发流程只复核已有附件，允许其中存在 daemon 归档。国内 COS 镜像由 ait-realease 的发布编排在 GitHub Release 发布后，与 DMG 一起按 `SHA256SUMS` 校验上传。

不新增 runner，只覆盖桌面已发布的 Linux x86_64 与 macOS Apple Silicon。Linux 归档在 `ubuntu-24.04` 构建，依赖该版本及以上的 glibc 与 libstdc++；需要支持更旧发行版时，另行决定构建基线。
