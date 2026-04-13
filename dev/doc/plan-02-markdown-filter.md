# Plan 02: Markdown 过滤器 (StreamingMarkdownFilter)

## 目标

实现 CJK 感知的 Markdown 过滤器，用于清洗出站（Bot→用户）消息中的 Markdown 标记，使文本在微信中可读性更好。

## 背景

TS 版本在 `src/messaging/markdown-filter.ts` 中实现了 `StreamingMarkdownFilter`，这是一个字符级状态机，逐字符处理 Markdown 文本，按以下规则过滤：

| Markdown 语法 | 处理方式 |
|---------------|---------|
| 代码围栏 ` ```...``` ` | **保留**（原样输出） |
| 行内代码 `` `...` `` | **保留** |
| 表格 `\|...\|` | **保留** |
| 粗体 `**...**` | **保留** |
| 水平线 `---` | **保留** |
| H5/H6 标题 `#####`, `######` | **去除 `#` 标记**，保留标题文本 |
| 图片 `![alt](url)` | **去除整行** |
| CJK 斜体/粗体 `*CJK*`, `**CJK**` | **去除 `*` 标记**，保留 CJK 文本内容 |
| 普通斜体 `*text*` | **保留**（仅对 CJK 内容去标记） |

### CJK 检测逻辑

当 `*` 或 `**` 包裹的内容包含中日韩字符时，去除标记符号但保留内容，因为微信不渲染 Markdown 而 `*` 符号会干扰阅读。

## 实现方式

### 新建文件

`src/messaging/markdown_filter.rs`

### 核心结构

```rust
pub struct StreamingMarkdownFilter {
    output: String,
    state: FilterState,
    line_buf: String,
    // ... 内部状态
}

enum FilterState {
    Sol,        // start-of-line
    Body,       // normal text
    Fence,      // inside code fence
    // ...
}

impl StreamingMarkdownFilter {
    pub fn new() -> Self;

    /// 逐块输入文本
    pub fn feed(&mut self, chunk: &str);

    /// 结束输入，获取最终结果
    pub fn finish(self) -> String;

    /// 便捷方法：一次性处理完整文本
    pub fn filter(text: &str) -> String;
}
```

### 状态机逻辑

1. **行首 (Sol)**：检测代码围栏 ` ``` `、标题 `#`、图片 `!`、表格 `|`
2. **行体 (Body)**：检测行内代码、`*` 标记
3. **围栏内 (Fence)**：原样输出直到闭合围栏
4. **星号累积**：累积 `*` 后跟的文本，判断是否为 CJK 内容决定是否去除标记

### CJK 字符检测

```rust
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   |  // CJK Unified Ideographs
        '\u{3400}'..='\u{4DBF}'   |  // Extension A
        '\u{3000}'..='\u{303F}'   |  // CJK Symbols and Punctuation
        '\u{3040}'..='\u{309F}'   |  // Hiragana
        '\u{30A0}'..='\u{30FF}'   |  // Katakana
        '\u{AC00}'..='\u{D7AF}'      // Hangul Syllables
    )
}
```

### 集成点

在 `WeixinClient::send_text` 中调用：

```rust
let filtered = StreamingMarkdownFilter::filter(text);
// 用 filtered 代替 text 发送
```

提供可选的 `filter_markdown` 配置项，允许用户关闭过滤：

```rust
pub struct WeixinConfigBuilder {
    // ...
    pub fn filter_markdown(mut self, enabled: bool) -> Self;
}
```

## 测试方式

1. **单元测试**：覆盖每种 Markdown 语法的过滤行为
   - 代码围栏保留测试
   - CJK 斜体/粗体去标记测试
   - 非 CJK 斜体保留测试
   - 标题去 `#` 测试
   - 图片行去除测试
   - 混合内容测试
2. **流式测试**：验证分块 `feed()` 与一次性 `filter()` 结果一致
3. **边界测试**：空字符串、纯代码块、纯 CJK 文本、嵌套标记
4. **集成测试**：通过 `send_text` 发送 Markdown 文本，验证实际发出的内容已过滤

## 风险

- 中等风险：状态机实现复杂度较高，需要充分的边界测试
- 可降级：提供配置项关闭过滤，出问题可快速回退
