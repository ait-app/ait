# iOS 0.1.12 Terminal 首字符崩溃与 Paseo 输入方案

日期：2026-10-20。故障基线：`39675e3b7da3eee568843858fa87de7e65d5d8d2`（Ait 0.1.12）。
本文只记录 Terminal；同一工作树的 Combobox 布局修复另行验证。

## 根因与复现

用户报告输入第一个字符后退出，并出现 Apple 的崩溃报告提交弹窗。本次未下载用户已提交的远端报告，
而是在 iPhone 17 Pro / iOS 26.5 模拟器上，用真实软键盘得到最小原生复现：

| 输入页组合                                          | 实际操作                             | 结果                                   |
| --------------------------------------------------- | ------------------------------------ | -------------------------------------- |
| 自定义 iOS 输入框，不含网络、网格、KeyboardProvider | 拼音 `a`、确认“啊”，再切英文输入 `a` | 页面收到 `Received: "啊a"`，进程存活   |
| 相同页面仅增加生产使用的 KeyboardProvider           | 聚焦正常，按软键盘 `a`               | 立即回到 SpringBoard，产生原生崩溃报告 |

本地产生的 `Ait-2026-10-20-135528.ips` 显示 `EXC_BAD_ACCESS / SIGSEGV / KERN_PROTECTION_FAILURE`，
错误地址落在 Stack Guard。关键栈已去除运行路径和设备信息：

```text
KCTextInputCompositeDelegate.textViewDelegate.getter
KCTextInputCompositeDelegate.activeDelegate.getter
KCTextInputCompositeDelegate.forwardingTarget(for:)
CoreFoundation forwarding
UIKitCore -[UITextView keyboardInputChangedSelection:]
UIKitCore -[UITextView keyboardInputChangedSelection:]
...（连续 6 帧）
UIKitCore -[UIKBInputDelegateManager changedSelection]
...软键盘事件...
```

[原实现](https://github.com/ait-app/ait/blob/39675e3b7da3eee568843858fa87de7e65d5d8d2/apps/mobile/modules/ait-terminal-input/ios/TerminalTextView.swift#L5-L17)
把 `UITextView` 自身设为 delegate。KeyboardController 代理保存这个原 delegate，并把不能处理的 selector
转发给它；原 delegate 同时继承了 UITextView 的内部键盘方法，于是键盘选区通知经代理再次进入同一输入框，
形成原生递归。输入框使用的 1×1 尺寸、网络发送和终端回显都不是这个最小复现的必要条件。
[KeyboardController 转发实现](https://github.com/kirillzyusko/react-native-keyboard-controller/blob/v1.21.12/ios/delegates/KCTextInputCompositeDelegate.swift)

本次也核对了 Expo/Fabric 的事件契约；无 Provider 的真实输入页已收到中英文字符，
不支持把故障归因于 `event.nativeEvent.data` 形状错误。

## Paseo 对比与最终选择

核对了近期移植的 Paseo `91154ba4e8ace3484595a05a4366279fb4818745`，以及当天查询到的上游
`main`：`3081b99b06aa29bf39b9a83aaed73cf854e4a89b`。最新 SHA 经 GitHub commits API 和
`git ls-remote` 双重确认；最新 `terminal-input.native.tsx` 与本地 `91154ba4` 逐字节一致。
初始导入版本与当前上游的区别见 [Paseo 来源](../../../paseo/README.md)。

| 对比项   | Paseo                                            | Ait 0.1.12 原实现                        |
| -------- | ------------------------------------------------ | ---------------------------------------- |
| 输入组件 | 隐藏的 EditingTextInput / React Native TextInput | 自定义 Swift UITextView / Expo 模块      |
| 输入处理 | onKeyPress、onChangeText 与 JS 差分状态          | UIKit markedTextRange 区分组合与确认文字 |
| delegate | RN 输入控件内部管理                              | 自定义输入框设置 delegate = self         |

[上游输入 JSX](https://github.com/getpaseo/paseo/blob/3081b99b06aa29bf39b9a83aaed73cf854e4a89b/packages/app/src/terminal/native-renderer/terminal-input.native.tsx#L308-L333)
及 [RN 包装器](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/components/ui/text-input/text-input.native.tsx#L177-L186)
显示它没有使用 Ait 的独立 iOS 输入桥。Paseo 同样使用
[KeyboardProvider](https://github.com/getpaseo/paseo/blob/91154ba4e8ace3484595a05a4366279fb4818745/packages/app/src/app/_layout.tsx#L961-L969)，
因此区别是输入控件的集成方式，而非单纯是否使用该键盘库。

Ait 在 2026-10-09 的 [PR #238](https://github.com/ait-app/ait/pull/238)
（提交 `522af1b43a1d86bd8ac37c266dfc66302f1eb554`）中，为修复中文组合输入新增了整个 Swift 模块。
本次先验证过独立 NSObject delegate 的局部修复，但用户随后明确选择参考 Paseo 的设计，
最终删除自定义模块，统一复用现有 React Native 输入。

## 最终改动

- 删除 iOS 专用入口、Expo 模块、Podspec、原生注册定义和相应专用测试。
- iOS、Android 的无后缀输入入口均解析到共享的
  [terminal-input.native.tsx](../../../apps/mobile/src/terminal/native-renderer/terminal-input.native.tsx)。
- 保留 Paseo 的字符去重、终端按键、中文/日文/韩文差分、粘贴和焦点处理。
- 根据真实 iOS 事件补两处共享状态修正：忽略与已预测文字完全对应的拼音音节分隔符，重新清空回车后的延迟换行，避免吞候选或下一行首字。
- [ADR-173](../../decisions/clients/adr-173-shared-native-terminal-input.md) 取代
  [ADR-114](../../decisions/clients/adr-114-ios-terminal-ime.md)，记录组合文字可能先进入 PTY 的行为。

## 验证

平台回归先改成要求 iOS 复用共享输入，原实现出现 1 失败、1 通过：iOS 仍解析到专用入口。
删除专用入口后，连同已有输入与控件回归，共 6 个文件、52 项通过：

```sh
cd apps/mobile
npm exec -- vitest run \
  src/terminal/native-renderer/terminal-input-platform.test.ts \
  src/terminal/native-renderer/terminal-input.native.test.ts \
  src/terminal/native-renderer/terminal-input-resize-policy.test.ts \
  src/terminal/runtime/terminal-key-dispatch.test.ts \
  src/terminal/runtime/terminal-virtual-keyboard.test.ts \
  src/components/ui/text-input/text-input.native.test.tsx
```

TypeScript 检查通过。Expo iOS autolinking 解析出 36 个模块，确认不再包含 `ait-terminal-input`
或 `AitTerminalInput`。删除源码后的空目录也已清除，避免本地 Metro 把空模块目录当成仍存在的包。

原来的 14 项 UIKit 输入回归，以及独立 delegate 方案的 15 项回归，属于调查过程的历史记录，
不计作最终共享输入方案的测试覆盖；相关 Swift 组件已移除。iOS 27 的旧 UIKit 测试宿主未采用 Scene
生命周期，未执行到测试，亦不计作本次输入结果。

### 真实软键盘最终结果

在同一 iPhone 17 Pro / iOS 26.5、同一 KeyboardProvider 输入页上复验，无额外调试副本或日志钩子：

- 原先会闪退的首个英文 `a` 正常到达，App 持续存活。
- 连续两行拼音 `nihao` 都能选中“你好”，每次收到 5 个回删控制字符后跟确认的汉字；回车后的下一行保留首个 `n`，完整候选仍是“你好”。
- 中文结束后切英文，收到单次 `a`；再次聚焦后输入 `b`、退格、回车，事件不重复，App 保持正常。

这轮实测也发现了直接沿用上游状态机的两个缺口，并分别用失败回归锁定后修复：

1. iOS 的真实事件为 `key h` 后接 `text ni\u2006h`，后续为 `ni\u2006ha`、`ni\u2006hao`。
   原状态机会把 U+2006 音节分隔当成普通替换，进入 desync 保护并吞掉“你好”。现在仅在移除音节间分隔符后
   恰好等于已预测文字时忽略该系统变更；保留普通粘贴中的 U+2006，且不解除已有自动纠错保护。
2. 回车已发送后，UIKit 仍可能送来带尾随换行的旧文本。只忽略该事件会让原生缓冲区留下换行，
   下一行首字也被后续清空。现在要求再次清理原生缓冲区；修复后的连续两行候选验证通过。

TypeScript、修改源码 ESLint/Oxfmt 及修改文档的本地链接检查通过。模拟器输入页验收不能代替真机与完整 Host
终端验收；后续原生安装包需重新 prebuild/autolink 以清理旧模块生成注册。本次未运行 Rust 测试，也未打包发布新版。

## 本地复现记录

临时入口为 `apps/mobile/.tmp/debug-terminal-input/index.tsx`，位于 ignored debug 目录。
原始故障日志为 `/private/tmp/ait-terminal-input-probe-provider-console.log`；原生报告只保留在本机
DiagnosticReports。共享输入打包日志为 `/private/tmp/ait-terminal-shared-probe-bundle.log`，
autolinking 输出为 `/private/tmp/ait-terminal-shared-autolinking.json`。运行产物不纳入版本控制。
