use super::render;

#[test]
fn renders_supported_markdown_and_escapes_raw_html() {
    let markdown =
        "# Result\n\n**bold** and `code`\n\n- one\n- <two>\n\n```rust\nlet x = 1 < 2;\n```";
    assert_eq!(
        render(markdown),
        "<b>Result</b>\n<b>bold</b> and <code>code</code>\n• one\n• &lt;two&gt;\n\n<pre><code>let x = 1 &lt; 2;\n</code></pre>"
    );
}

#[test]
fn escapes_link_attributes() {
    assert_eq!(
        render("[docs](https://example.com/?a=1&b=\"two\")"),
        "<a href=\"https://example.com/?a=1&amp;b=&quot;two&quot;\">docs</a>"
    );
}
