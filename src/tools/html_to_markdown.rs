//! HTML-to-Markdown conversion used after Readability main-content extraction.

use html2md;

/// Converts extracted article HTML into Markdown via the `html2md` parser.
pub fn convert_html_to_markdown(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }

    // Use html2md to parse and convert
    html2md::parse_html(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_html() {
        assert_eq!(convert_html_to_markdown(""), "");
    }

    #[test]
    fn test_simple_heading() {
        let html = "<h1>Test Title</h1>";
        let md = convert_html_to_markdown(html);
        // html2md may format headings differently, just check the text is present
        assert!(
            md.contains("Test Title"),
            "Markdown should contain the title text"
        );
    }

    #[test]
    fn test_paragraph() {
        let html = "<p>This is a paragraph.</p>";
        let md = convert_html_to_markdown(html);
        assert!(md.contains("This is a paragraph"));
    }

    #[test]
    fn test_link() {
        let html = r#"<a href="https://example.com">Example</a>"#;
        let md = convert_html_to_markdown(html);
        assert!(md.contains("[Example]"));
        assert!(md.contains("https://example.com"));
    }

    #[test]
    fn test_list() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let md = convert_html_to_markdown(html);
        assert!(md.contains("Item 1"));
        assert!(md.contains("Item 2"));
    }

    #[test]
    fn test_code_block() {
        let html = "<pre><code>let x = 1;</code></pre>";
        let md = convert_html_to_markdown(html);
        assert!(md.contains("let x = 1"));
    }

    #[test]
    fn test_table() {
        let html = "<table><tr><th>Header</th></tr><tr><td>Data</td></tr></table>";
        let md = convert_html_to_markdown(html);
        assert!(md.contains("Header"));
        assert!(md.contains("Data"));
    }

    #[test]
    fn test_complex_html() {
        let html = r#"
            <article>
                <h1>Main Title</h1>
                <p>First paragraph with <strong>bold</strong> and <em>italic</em>.</p>
                <ul>
                    <li>List item 1</li>
                    <li>List item 2</li>
                </ul>
            </article>
        "#;
        let md = convert_html_to_markdown(html);
        assert!(md.contains("Main Title"));
        assert!(md.contains("First paragraph"));
        assert!(md.contains("List item 1"));
    }
}
