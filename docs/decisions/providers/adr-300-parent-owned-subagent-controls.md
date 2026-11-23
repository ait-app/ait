---
id: ADR-300
status: proposed
date: 2026-11-23
amends:
  - clients/adr-044-paseo-client-rust-transport.md
  - providers/adr-209-observed-subagent-normalization.md
paths:
  - crates/provider/core
  - crates/provider/omp
  - crates/provider/runtime
  - crates/api
  - packages/protocol
  - packages/client
  - apps/mobile/src
---

# ADR-300：由父会话控制原生子 Agent

## 背景

原生子 Agent 以只读 observed 身份登记。用户需要引导或停止仍在运行的子任务，但不能
因此接管子会话、创建第二个 writer，或向其他父会话发送控制。

## 决策

- 新增类型化子 Agent 引导和停止接口；客户端提交父、子 Ait 身份及可选观测版本，原生
  身份由宿主从可信观测记录取得。不得提供通用原生命令网关。
- 执行前校验父会话由本进程管理且有 live writer、子项属于该父会话且观测状态为当前。
  通过父会话的单一 writer 请求控制；不恢复父会话，也不启动子 writer。普通 observed
  Agent 的写操作仍被拒绝。
- 控制返回关联收据，由原生回执异步结算；等待期间同一父会话必须继续处理权限问题。
  只在可证明时报告接受或子项不再运行，结果不确定时不重试或伪造成功。
- 新客户端按宿主方法与父会话能力显示操作；旧宿主和未实现端口的 Provider 不提供控制。
  访客的控制请求仍受父 Agent 的既有驱动权限限制。
- 分通道 SDK 只在主通道协商和调用 `connection.ping`；各通道仍协商自己的订阅释放方法。
  这修订 ADR-044 对每个通道均协商 ping 的要求，为新增方法保留能力槽位。

## 后果

该控制是 ADR-209 只读观测边界中受父会话约束的窄例外，不改变子 Agent 自身的写权限。
原生停止是否已经完成仍由后续观测确认；连接丢失不能作为安全重放控制命令的理由。
