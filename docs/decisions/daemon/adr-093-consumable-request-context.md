# ADR-093：以可消费 Context 逐级处理请求

- 状态：Accepted。
- 日期：2026-10-07。
- 背景：API 先从全局路由表选出 crate 和能力组，再调用同一处维护的执行分支，重复表达了
  请求归属。用户要求使用 `Option<Context>` 逐级处理，处理后直接结束流程。
- 范围：修订 ADR-036、ADR-037 的请求分发方式；不改变 crate 依赖方向及传输协议。

## 决策

API 校验名称、消息方向、已协商 capability 和能力安装情况后，将请求放入
`Some(Context)`。工作区创建、归档、relay 和各能力 crate 的入口依次接收
`&mut Option<Context>`，在自己的入口判断是否处理。未匹配时原样保留请求；匹配后取走
Context，完成执行和响应。调用方只根据返回值结束或继续：成功直接返回，只有
`DispatchError::NotImplemented` 才进入下一层，其他错误立即传播。

各能力 crate 内部也逐个调用处理分支，在实际处理入口直接匹配方法并取走 Context。
删除跨 crate 和 crate 内部的 Group、`IMPLEMENTED_GROUPS` 与 `Context::take_matching`，
执行路径不查询 capability 目录。各实际实现组件在自己的 RPC、service 或 connection 模块
声明 `METHODS`；crate 的能力发现只组合这些方法与安装条件，API 再组合各 crate 的迭代器。
protocol 中重复的 capability 数组移除，协议类型与规范目录不决定 daemon 已实现什么。
消息方向和兼容名称继续由协议校验，传输队列归属查询实现组件组合后的方法目录。
未实现方法返回 `DispatchError::NotImplemented` 并保留 Context。每个继续分发的分支调用
`Context::assert_unhandled`，断言请求仍为 `Some`；若 Context 已消费，先记录带 handler 名称的
error，再触发断言，阻止违反契约的处理链继续执行。此断言在 debug 和 release 均生效。
删除把未实现错误转成成功默认值的 `DispatchError::or_next`，也不在成功后用 Context 的
Option 状态再次决定流程。已消费请求的队列或编码失败返回 `DispatchError::Delivery` 并立即传播。
空 Context 入口仍然无操作成功返回。

方法目录只保留消息方向与协商 capability，继续区分未知方法、错误方向、未协商能力与未安装
实现。`server.status.unsubscribe` 继续使用 `server.status.subscribe` 的协商能力。

匹配后的业务错误也属于已处理请求，通过原请求 ID 发送一次错误响应，不交给下一层重试。
队列或编码错误直接传播，取走的请求不恢复到 Option。全部入口均未匹配时，API 发送
`not_implemented`；未知名称在进入处理链前继续返回 `method_not_found`。

metadata 和 provider 仍以明确的数据返回跨能力收尾工作：释放连接订阅、获取 daemon 的
Provider 可用性快照，以及关闭 Agent 后关闭关联 Terminal。API 必须完成收尾并发送响应后，
才从成功分支返回。已接纳的后台等待继续由 Runtime 追踪，Option 为 `None`
表示请求所有权已移交，不要求长任务此时已经结束。

单连接模式保留四个有界 worker 及其连接状态所有权。入口按所属 crate 的方法声明选择
worker 队列，仅用于并发和订阅隔离，不选择业务 handler 或传递能力组；worker 内仍使用
相同的 Context 处理链。事件、客户端响应和二进制帧沿用原连接入口。

## 后果与验证

新增能力实现只需要所属 crate 的声明与处理分支。API 仍显式装配能力入口和跨能力协调，
不引入动态 handler 注册、boxed future 或回调接口。匹配不分配内存，最坏逐个尝试
处理分支；请求载荷只移动一次。

回归测试覆盖未匹配错误与请求原样保留、违约消费的断言、空 Context 无操作、所有已声明请求的消费分支、
各 crate 消费一次、业务错误停止处理、发送失败传播、后段能力与最终兜底、metadata 收尾、校验错误优先级及 worker 归属。
既有 WebSocket 测试继续验证 capability 协商、响应顺序、订阅释放和连接隔离。
