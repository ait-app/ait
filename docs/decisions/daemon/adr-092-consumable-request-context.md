# ADR-092：以可消费 Context 逐级处理请求

- 状态：Accepted。
- 日期：2026-10-07。
- 背景：API 先从全局路由表选出 crate 和能力组，再调用同一处维护的执行分支，重复表达了
  请求归属。用户要求使用 `Option<Context>` 逐级处理，处理后直接结束流程。
- 范围：修订 ADR-036、ADR-037 的请求分发方式；不改变 crate 依赖方向及传输协议。

## 决策

API 校验名称、消息方向、已协商 capability 和能力安装情况后，将请求放入
`Some(Context)`。工作区创建、归档、relay 和各能力 crate 的入口依次接收
`&mut Option<Context>`，在自己的入口判断是否处理。未匹配时原样保留请求；匹配后取走
Context，完成执行和响应。每层完成后，API 发现 Option 为 `None` 就正常返回。

公共 `Context::take_matching` 根据调用方自己的方法分组匹配并取走请求，不复制请求或载荷。
能力组只留在所属 crate 内部，API 删除跨 crate 的 Group 枚举、handler 路由表和组分发 match。
方法目录只保留消息方向与协商 capability，继续区分未知方法、错误方向、未协商能力与未安装
实现。`server.status.unsubscribe` 继续使用 `server.status.subscribe` 的协商能力。

匹配后的业务错误也属于已处理请求，通过原请求 ID 发送一次错误响应，不交给下一层重试。
队列或编码错误直接传播，取走的请求不恢复到 Option。全部入口均未匹配时，API 发送
`not_implemented`；未知名称在进入处理链前继续返回 `method_not_found`。

metadata 和 provider 仍以明确的数据返回跨能力收尾工作：释放连接订阅、获取 daemon 的
Provider 可用性快照，以及关闭 Agent 后关闭关联 Terminal。API 必须完成收尾并发送响应后，
才能根据已消费的 Context 返回。已接纳的后台等待继续由 Runtime 追踪，Option 为 `None`
表示请求所有权已移交，不要求长任务此时已经结束。

单连接模式保留四个有界 worker 及其连接状态所有权。入口按所属 crate 的方法声明选择
worker 队列，仅用于并发和订阅隔离，不选择业务 handler 或传递能力组；worker 内仍使用
相同的 Context 处理链。事件、客户端响应和二进制帧沿用原连接入口。

## 后果与验证

新增能力实现只需要所属 crate 的声明与处理分支。API 仍显式装配能力入口和跨能力协调，
不引入动态 handler 注册、boxed future 或回调接口。匹配不分配内存，最坏按静态方法声明
数量线性扫描；请求载荷只移动一次。

回归测试覆盖未匹配请求原样保留、空 Context 无操作、各 crate 消费一次、业务错误停止
处理、发送失败传播、后段能力与最终兜底、metadata 收尾、校验错误优先级及 worker 归属。
既有 WebSocket 测试继续验证 capability 协商、响应顺序、订阅释放和连接隔离。
