# ADR-163：nightly 附带独立 daemon 归档

- 状态：Accepted
- 日期：2026-10-19
- 修订：ADR-158（原规定 nightly 不发布独立 daemon）

## 背景

[ADR-158](adr-158-standalone-daemon-release.md) 只让正式版与 beta 附带独立 daemon 归档。
当时 nightly 的 Linux daemon 直接在 `ubuntu-24.04` runner 上编译，需要 glibc 2.39；
main CI 也没有 Developer ID 证书，macOS nightly 只能 ad-hoc 签名。无人值守主机若要验证
main 上的修复，只能从源码构建，或从桌面安装包中取出 `resources/bin/daemon`。

nightly 的 Linux daemon 现已与正式版一样在 `manylinux_2_28` 容器中编译，glibc 不再是障碍。

## 决策

- main CI 发布 nightly 时附带 `Ait-<构建标识>-daemon-linux-x64.tar.gz` 与
  `Ait-<构建标识>-daemon-macos-arm64.tar.gz`。归档与桌面包共用同一次构建的 daemon，
  内容与正式版相同（`ait-daemon` 和 `LICENSE`）；打包时仍核对 `--version` 等于 `packagedVersion`。
- nightly 的资产校验必须包含这两个归档并纳入 `SHA256SUMS`，不接受以版本号命名的 daemon 归档。
- macOS daemon 只保留链接器生成的 ad-hoc 签名，不使用 Developer ID 签名，也不公证。
  nightly Release 说明提示：浏览器下载后若被 Gatekeeper 拦截，在解压目录执行
  `xattr -d com.apple.quarantine ait-daemon`。
- 正式版与 beta 的签名、公证和校验规则不变。

## 影响

- 不需要向 main CI 开放 Apple 签名密钥，nightly 也不等待公证。
- macOS 用户使用 nightly daemon 时需要手动移除 quarantine 属性；用 `curl` 等命令行工具下载
  不会附加该属性。
- nightly 的 Linux 打包时间包含 manylinux 容器内的 Rust 安装与编译。
