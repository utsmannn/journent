//! Markdown → HTML. Server-side render so post body + comments are curl-loadable.
//!
//! Diagrams: fenced ` ```mermaid ` blocks are rewritten to
//! `<div class="mermaid">…escaped source…</div>` so the client-side mermaid.js
//! can pick them up and render them as SVG. With JS disabled, the source still
//! shows as readable text inside the div — graceful degradation, curl-friendly.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html};

pub fn render(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(md, opts);
    let events = rewrite_mermaid_blocks(parser);
    let mut out = String::with_capacity(md.len() * 2);
    html::push_html(&mut out, events.into_iter());
    out
}

/// True if the CodeBlockKind is a fenced block whose tag is exactly `mermaid`
/// (case-insensitive, trimmed).
fn is_mermaid(kind: &CodeBlockKind) -> bool {
    match kind {
        CodeBlockKind::Fenced(lang) => lang.trim().eq_ignore_ascii_case("mermaid"),
        _ => false,
    }
}

/// Intercept ` ```mermaid ` fenced blocks and emit a `<div class="mermaid">…</div>`
/// wrapper around their text content. `push_html` HTML-escapes `Event::Text`
/// for us, so the mermaid source is safely escaped while still being decoded
/// back to source by mermaid.js (which reads `textContent` on the div). Outside
/// mermaid blocks, events pass through unchanged.
fn rewrite_mermaid_blocks<'a>(parser: Parser<'a>) -> Vec<Event<'a>> {
    let mut out = Vec::new();
    let mut in_mermaid = false;
    for ev in parser {
        match ev {
            Event::Start(Tag::CodeBlock(ref k)) if is_mermaid(k) => {
                in_mermaid = true;
                out.push(Event::Html(CowStr::Borrowed("<div class=\"mermaid\">")));
            }
            Event::End(TagEnd::CodeBlock) if in_mermaid => {
                in_mermaid = false;
                out.push(Event::Html(CowStr::Borrowed("</div>")));
            }
            // text inside a mermaid block is forwarded as Text; push_html escapes it
            other @ Event::Text(_) if in_mermaid => out.push(other),
            other => out.push(other),
        }
    }
    out
}

/// Strip a leading markdown H1 (e.g. `# Title`) from the body. Convention: the title is
/// stored separately, so a body that re-states its own `# Title` would render a duplicate
/// heading. Remove only the FIRST line if it is an H1.
pub fn strip_leading_h1(md: &str) -> String {
    let mut lines = md.lines();
    let first = match lines.next() {
        Some(s) => s.trim_start_matches([' ', '\t']),
        None => return md.to_string(),
    };
    if !(first.starts_with("# ") || first == "#") {
        return md.to_string();
    }
    let rest: Vec<&str> = lines.collect();
    rest.join("\n")
}

/// Short excerpt for feed card.
pub fn excerpt(md: &str, max: usize) -> String {
    // strip markdown roughly, ambil plain text-ish
    let p = Parser::new_ext(md, Options::empty());
    let mut text = String::new();
    for event in p {
        if let pulldown_cmark::Event::Text(t) = event {
            text.push_str(&t);
        }
        if text.len() > max * 2 {
            break;
        }
    }
    if text.len() > max {
        let mut end = max;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_block_renders_as_mermaid_div() {
        let md = "```mermaid\ngraph TD\n  A --> B\n```\n";
        let html = render(md);
        assert!(html.contains("<div class=\"mermaid\">"));
        assert!(html.contains("</div>"));
        // source text is preserved (escaped inside) so mermaid.js can read it
        assert!(html.contains("A --&gt; B"));
    }

    #[test]
    fn non_mermaid_code_block_is_left_alone() {
        let md = "```rust\nlet x = 1;\n```\n";
        let html = render(md);
        assert!(html.contains("language-rust"));
        assert!(!html.contains("mermaid"));
    }
}
