---
id: ADR-283
status: accepted
date: 2026-11-17
amends:
  - workspace/adr-136-single-area-layer-flattening.md
paths:
  - crates/model
  - crates/metadata
  - crates/filesystem
---

# ADR-283：RPC JSON 编解码统一归 model

## 背景

`model::rpc` 已提供 RPC envelope 使用的 JSON 编解码，但 metadata 与 filesystem 仍各自
保留一份相同实现。两份副本只为携带各 crate 自己的错误码，导致同一 crate 内出现两套
调用路径。

## 决策

`model::rpc::{decode, encode}` 是所有依赖 `model` 的 RPC JSON 编解码入口。`encode` 接受
调用方提供的错误值并在序列化失败时原样返回，使 metadata 与 filesystem 可以继续保留
各自的错误码类型；`decode` 的 `model::ErrorCode` 通过已有转换进入调用方错误类型。

删除 metadata 与 filesystem 的 `support::rpc` 模块及其调用点。`support` 仍只承载各 crate
自己的错误码和其他内部辅助逻辑，不新增对 capability group 的依赖。

## 后果

RPC 编解码行为只有一个实现，错误映射调整只需修改 `model::rpc`。保留 crate 本地错误码
避免改变 capability RPC 的错误边界；新增 RPC 代码应直接从 `model::rpc` 导入编解码函数。
