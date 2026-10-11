# ADR-137：Diff 的 GitHub 语言提示、配色与按文件降级

状态：Accepted（2026-10-11）

## 背景

ADR-071 在 Rust checkout adapter 中生成语法 token，但语言只按扩展名选择。
完整订阅超出输出预算时，RPC 清除所有文件的 token；纯文本仍超预算时清空所有文件。
因此大文件增加或减少时，旁边没有变化的小文件也可能失去高亮。
Diff 的颜色还跟随全局语法主题，默认设置会覆盖已有 GitHub 配色。

GitHub 的公开实现将 [Linguist 语言识别](https://github.com/github-linguist/linguist/blob/main/docs/how-linguist-works.md)
与 [grammar 高亮](https://github.com/github-linguist/linguist/blob/main/docs/troubleshooting.md#theres-a-problem-with-the-syntax-highlighting-of-a-file)
区分开，并支持 [linguist-language 和 modeline 覆盖](https://github.com/github-linguist/linguist/blob/main/docs/overrides.md)。
GitHub 本身也有[单文件与完整 Diff 的显示限制](https://docs.github.com/en/repositories/creating-and-managing-repositories/repository-limits#diff-limits)。

## 决策

- 保持 Rust `two-face / syntect` 解析、主题无关 token 和现有协议。语言检测顺序为显式
  `linguist-language`、文件前后五行的 Vim/Emacs modeline、完整文件名、首行 shebang、扩展名。
  支持 Dockerfile、Makefile 及其变体、CMakeLists.txt 和 grammar 中的已知 dotfile。
  无法识别的显式语言覆盖保留普通文本，不套用扩展名对应的其他语言。
- 新旧两侧分别识别语言。旧侧使用 rename 前路径和旧 revision 的 attributes；新侧使用
  工作树或目标 revision 的 attributes。`git check-attr --source` 读取对应树，路径分批查询、
  NUL 分隔；Git 无法提供 attributes 时继续使用其他语言提示。不可变快照缓存键包含语言
  覆盖，工作树内容继续按摘要和语法名称缓存。
- Diff 画布使用已有 GitHub 明暗语法配色，随应用明暗模式切换。其余代码视图继续使用
  全局语法主题，Diff 的布局、背景和增删色仍来自应用主题。
- RPC 保持现有输出预算和单文件检查。完整快照超预算时，先计算真实 JSON 字节数；
  若纯文本本身过大，按文件文本成本从大到小清除必要的 hunks，保留文件元数据和
  `too_large` 状态。随后只清除足够数量的高亮 token，优先处理 token 成本最高的文件。
  文件数组保持原顺序，不因预算排序改变 UI；JSON 大小使用计数 writer，不分配编码副本。
- 若文件元数据或其他响应字段仍无法满足预算，继续保留既有完整快照 `diffTooLarge`
  兜底。原始 Git Diff、单行和解析文本的限制不变；这不是无限大小或始终高亮的保证。

## 后果

大文件不再触发整个快照的无差别清除，小文件更容易保持高亮和内容。
用户可用与 GitHub 相同的语言提示修正无扩展名或自定义扩展名文件。
本实现没有引入 Ruby Linguist，也没有复制 GitHub 的全部 grammar、歧义语言启发式或
贝叶斯分类，因此语言覆盖、token 边界和具体限制不保证与 GitHub 完全一致。

本决策修订 ADR-071 的语言选择、客户端配色和完整快照预算策略；adapter、port、RPC
和客户端的职责不变。验证见[实现报告](../../reports/workspace/github-style-diff-highlighting.md)。
