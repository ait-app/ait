# ADR-173：移动端终端输入复用 React Native

- 状态：Accepted，取代 [ADR-114](adr-114-ios-terminal-ime.md)
- 日期：2026-10-20

iOS 0.1.12 的自定义 `UITextView` 将自己设为 delegate，和全局 `KeyboardProvider`
的代理转发组合后，首字符触发 UIKit 内部选区回调递归，造成原生栈溢出。
独立 delegate 可以局部修复，但仍需单独维护 iOS 输入桥。按用户选择，改为参考
[Paseo 的共享输入实现](https://github.com/getpaseo/paseo/blob/3081b99b06aa29bf39b9a83aaed73cf854e4a89b/packages/app/src/terminal/native-renderer/terminal-input.native.tsx)。

iOS 和 Android 均由 `terminal-input.native.tsx` 使用隐藏的 `EditingTextInput`，通过
React Native `TextInput` 的 `onKeyPress`、`onChangeText` 处理文字、终端控制键和焦点。
删除 iOS 专用入口及 `ait-terminal-input` Expo 模块，不再由自有 Swift View 管理输入 delegate。
既有 native-grid、WebView、终端传输与 Rust daemon 边界不变。

中文、日文、韩文沿用上游 JS 差分与回删替换策略，组合中的文字可能先进入 PTY，
不再承诺 ADR-114 中“只发送确认文字”的行为。直接处理按键的全屏终端程序仍需按实际输入法
验证；多行粘贴继续使用显式终端粘贴入口。

保留两处由真实 iOS 事件验证的共享状态机修正：音节间自动插入的 `U+2006` 在去掉分隔符后
恰好等于已预测文字时不视作替换，避免吞掉后续汉字候选；回车后的延迟换行事件重新清空原生
缓冲区，避免吞掉下一行首字。普通粘贴空格和既有自动纠错保护不变。

回归使用实际 Metro 配置验证 iOS、Android 都解析到共享输入组件，并检查已有输入状态、
键盘控制和焦点行为。原生验收重跑带 `KeyboardProvider` 的同一首字符复现，检查文字到达且
进程存活。后续原生构建重新执行 Expo autolinking，移除旧模块的生成注册。
