use crate::pipelines::DomNode;
use std::fmt::Write;

/// Maximum output size in bytes. Output exceeding this is truncated.
const MAX_OUTPUT_SIZE: usize = 500 * 1024; // 500 KiB

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect all descendant text nodes into a single string.
fn collect_text(nodes: &[DomNode]) -> String {
    let mut buf = String::new();
    for node in nodes {
        match node {
            DomNode::Text(t) => buf.push_str(t),
            DomNode::Element { children, .. } => buf.push_str(&collect_text(children)),
            _ => {}
        }
    }
    buf
}

/// Get the value of an attribute by name.
fn get_attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// Extract the code language and content from a <pre><code> block.
fn extract_code_block(nodes: &[DomNode]) -> (String, String) {
    for node in nodes {
        if let DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } = node
            && tag == "code"
        {
            let language = get_attr(attrs, "class")
                .and_then(|c| c.strip_prefix("language-"))
                .unwrap_or_default()
                .to_string();
            return (language, collect_text(children));
        }
    }
    (String::new(), collect_text(nodes))
}

/// Collect rows from a <table> element.
fn collect_table_rows(nodes: &[DomNode]) -> Vec<Vec<DomNode>> {
    let mut rows = Vec::new();
    for node in nodes {
        if let DomNode::Element { tag, children, .. } = node {
            match tag.as_str() {
                "tr" => {
                    rows.push(children.clone());
                }
                "thead" | "tbody" | "tfoot" => {
                    rows.extend(collect_table_rows(children));
                }
                _ => {}
            }
        }
    }
    rows
}

/// Check if a table row looks like a header (contains <th> cells).
fn has_header_cells(cells: &[DomNode]) -> bool {
    for cell in cells {
        if let DomNode::Element { tag, .. } = cell
            && tag == "th"
        {
            return true;
        }
    }
    false
}

/// Collect individual cell nodes from a table row.
fn collect_cells(nodes: &[DomNode]) -> Vec<DomNode> {
    let mut cells = Vec::new();
    for node in nodes {
        if let DomNode::Element { tag, .. } = node
            && (tag == "td" || tag == "th")
        {
            cells.push(node.clone());
        }
    }
    cells
}

/// Check if a table element has colspan, rowspan, or block content in any cell.
/// Such tables cannot be represented as GFM pipe tables and must be emitted as raw HTML.
fn table_is_complex(node: &DomNode) -> bool {
    match node {
        DomNode::Element { tag, attrs, children, .. } => {
            if tag == "td" || tag == "th" {
                // Check for colspan/rowspan
                if let Some(v) = get_attr(attrs, "colspan") {
                    if v != "1" { return true; }
                }
                if let Some(v) = get_attr(attrs, "rowspan") {
                    if v != "1" { return true; }
                }
                // Check for block elements inside the cell
                for child in children {
                    if let DomNode::Element { tag: t, .. } = child {
                        match t.as_str() {
                            "ul" | "ol" | "pre" | "blockquote" | "table" => return true,
                            _ => {}
                        }
                    }
                }
            }
            // Recurse into children
            for child in children {
                if table_is_complex(child) {
                    return true;
                }
            }
        }
        _ => {}
    }
    false
}

/// Serialize a DomNode tree to an HTML string.
fn serialize_node_to_html(node: &DomNode) -> String {
    match node {
        DomNode::Text(text) => {
            // Escape HTML special characters
            let mut out = String::with_capacity(text.len());
            for ch in text.chars() {
                match ch {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '"' => out.push_str("&quot;"),
                    _ => out.push(ch),
                }
            }
            out
        }
        DomNode::Element { tag, attrs, children, .. } => {
            // Special case: convert <math> to inline/display LaTeX
            if tag == "math" {
                let alttext = get_attr(attrs, "alttext").unwrap_or("");
                let display = get_attr(attrs, "display").unwrap_or("inline");
                if !alttext.is_empty() {
                    if display == "block" || display == "display" {
                        return format!("$$\\displaystyle {}$$\n", alttext);
                    } else {
                        return format!("${}$", alttext);
                    }
                }
                // No alttext — render children as fallback
                return children.iter().map(|c| serialize_node_to_html(c)).collect::<String>();
            }
            let mut out = String::new();
            let _ = write!(out, "<{}", tag);
            for (k, v) in attrs {
                let _ = write!(out, " {}=\"{}\"", k, v.replace('"', "&quot;"));
            }
            if children.is_empty() {
                let _ = write!(out, " />");
            } else {
                out.push('>');
                for child in children {
                    out.push_str(&serialize_node_to_html(child));
                }
                let _ = write!(out, "</{}>", tag);
            }
            out
        }
        DomNode::Comment(comment) => {
            format!("<!--{}-->", comment)
        }
        DomNode::Doctype(dtd) => {
            format!("<!DOCTYPE {}>", dtd)
        }
    }
}

/// Resolve a potentially relative URL against a base URL.
fn resolve_url(url: &str, base_url: Option<&str>) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    if url.starts_with("//") {
        return format!("https:{}", url);
    }
    if url.starts_with("mailto:") || url.starts_with("javascript:") || url.starts_with("#") {
        return url.to_string();
    }
    match base_url {
        Some(base) => {
            let base = base.trim_end_matches('/');
            if url.starts_with('/') {
                // Absolute path relative to the base domain
                // Try to extract scheme + host from base
                if let Some(pos) = base.find("://") {
                    let after_scheme = &base[pos + 3..];
                    if let Some(slash_pos) = after_scheme.find('/') {
                        format!("{}://{}{}", &base[..pos], &after_scheme[..slash_pos], url)
                    } else {
                        format!("{}{}", base, url)
                    }
                } else {
                    format!("{}{}", base, url)
                }
            } else {
                format!("{}/{}", base, url)
            }
        }
        None => url.to_string(),
    }
}

/// Escape Markdown special characters.
fn escape_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => result.push_str("\\\\"),
            '`' => result.push_str("\\`"),
            '*' => result.push_str("\\*"),
            '_' => result.push_str("\\_"),
            '{' => result.push_str("\\{"),
            '}' => result.push_str("\\}"),
            '[' => result.push_str("\\["),
            ']' => result.push_str("\\]"),
            '(' => result.push_str("\\("),
            ')' => result.push_str("\\)"),
            '#' => result.push_str("\\#"),
            '+' => result.push_str("\\+"),
            '.' => result.push_str("\\."),
            '|' => result.push_str("\\|"),
            _ => result.push(ch),
        }
    }
    result
}

// ---------------------------------------------------------------------------
// MarkdownLowerer
// ---------------------------------------------------------------------------

/// Lowers a DOM tree into a Markdown string.
pub struct MarkdownLowerer;

impl MarkdownLowerer {
    /// Convert a DOM node tree directly into a Markdown string.
    ///
    /// This is the main entry point for producing Markdown output from a
    /// processed DOM tree. The `base_url` is used to resolve relative URLs
    /// in `<a href="…">` and `<img src="…">` elements.
    pub fn lower(node: &DomNode, base_url: Option<&str>) -> String {
        let mut out = String::new();
        Self::lower_nodes(&[node.clone()], base_url, &mut out, 0);
        Self::cap_size(out)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl MarkdownLowerer {
    /// Recursively lower nodes into a Markdown string builder.
    fn lower_nodes(nodes: &[DomNode], base_url: Option<&str>, out: &mut String, indent: usize) {
        for node in nodes {
            Self::lower_node(node, base_url, out, indent);
        }
    }

    /// Lower a single node.
    fn lower_node(node: &DomNode, base_url: Option<&str>, out: &mut String, indent: usize) {
        match node {
            DomNode::Text(text) => {
                out.push_str(&escape_markdown(text));
            }
            el @ DomNode::Element { .. } => Self::lower_element(el, base_url, out, indent),
            DomNode::Comment(_) | DomNode::Doctype(_) => {
                // Comments and doctypes are not rendered in Markdown.
            }
        }
    }

    /// Lower an element node.
    #[allow(clippy::too_many_lines)]
    fn lower_element(
        node: &DomNode,
        base_url: Option<&str>,
        out: &mut String,
        indent: usize,
    ) {
        let DomNode::Element { tag, attrs, children, .. } = node else {
            return;
        };
        match tag.as_str() {
            // ── Headings ───────────────────────────────────────────────
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag[1..].parse::<usize>().unwrap_or(1);
                let prefix = "#".repeat(level);
                let text = collect_text(children);
                out.push_str(&prefix);
                out.push(' ');
                out.push_str(&text);
                out.push_str("\n\n");
            }

            // ── Paragraphs ─────────────────────────────────────────────
            "p" => {
                Self::lower_nodes(children, base_url, out, indent);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }

            // ── Bold / Strong ──────────────────────────────────────────
            "strong" | "b" => {
                let text = Self::lower_inline(children, base_url);
                if !text.is_empty() {
                    out.push_str("**");
                    out.push_str(&text);
                    out.push_str("**");
                }
            }

            // ── Italic / Emphasis ──────────────────────────────────────
            "em" | "i" => {
                let text = Self::lower_inline(children, base_url);
                if !text.is_empty() {
                    out.push('*');
                    out.push_str(&text);
                    out.push('*');
                }
            }

            // ── Links ──────────────────────────────────────────────────
            "a" => {
                let href = get_attr(attrs, "href").unwrap_or("");
                let text = Self::lower_inline(children, base_url);
                if href.is_empty() {
                    out.push_str(&text);
                } else {
                    let resolved = resolve_url(href, base_url);
                    out.push('[');
                    out.push_str(&text);
                    out.push_str("](");
                    out.push_str(&resolved);
                    out.push(')');
                }
            }

            // ── Images ─────────────────────────────────────────────────
            "img" => {
                let src = get_attr(attrs, "src").unwrap_or("");
                let alt = get_attr(attrs, "alt").unwrap_or("");
                let resolved = resolve_url(src, base_url);
                out.push('!');
                out.push('[');
                out.push_str(alt);
                out.push_str("](");
                out.push_str(&resolved);
                out.push(')');
            }

            // ── Unordered lists ────────────────────────────────────────
            "ul" => {
                Self::lower_unordered_list(children, base_url, out, indent);
                out.push('\n');
            }

            // ── Ordered lists ──────────────────────────────────────────
            "ol" => {
                Self::lower_ordered_list(children, base_url, out, indent);
                out.push('\n');
            }

            // ── Blockquotes ────────────────────────────────────────────
            "blockquote" => {
                let inner = Self::lower_inline_block(children, base_url);
                for line in inner.lines() {
                    if line.trim().is_empty() {
                        out.push_str(">\n");
                    } else {
                        out.push_str("> ");
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                out.push('\n');
            }

            // ── Code blocks ────────────────────────────────────────────
            "pre" => {
                let (lang, code) = extract_code_block(children);
                out.push_str("```");
                out.push_str(&lang);
                out.push('\n');
                out.push_str(&code);
                if !code.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```\n\n");
            }

            // ── Inline code ────────────────────────────────────────────
            "code" => {
                let text = collect_text(children);
                out.push('`');
                out.push_str(&text);
                out.push('`');
            }

            // ── Math (LaTeXML MathML) ────────────────────────────────
            "math" => {
                let alttext = get_attr(attrs, "alttext").unwrap_or("");
                let display = get_attr(attrs, "display").unwrap_or("inline");
                if alttext.is_empty() {
                    // Fallback: render text content
                    let text = collect_text(children);
                    out.push_str(&text);
                } else if display == "block" || display == "display" {
                    out.push_str("$$\\displaystyle ");
                    out.push_str(alttext);
                    out.push_str("$$\n\n");
                } else {
                    out.push('$');
                    out.push_str(alttext);
                    out.push('$');
                }
            }

            // ── Horizontal rule ────────────────────────────────────────
            "hr" => {
                out.push_str("---\n\n");
            }

            // ── Tables ─────────────────────────────────────────────────
            "table" => {
                Self::lower_table(node, base_url, out);
            }

            // ── Fallback: render inner text only ───────────────────────
            _ => {
                Self::lower_nodes(children, base_url, out, indent);
            }
        }
    }

    /// Lower children as inline text (no trailing newlines).
    fn lower_inline(nodes: &[DomNode], base_url: Option<&str>) -> String {
        let mut buf = String::new();
        Self::lower_nodes(nodes, base_url, &mut buf, 0);
        buf
    }

    fn lower_inline_block(nodes: &[DomNode], base_url: Option<&str>) -> String {
        Self::lower_inline(nodes, base_url)
    }

    /// Lower an unordered list.
    #[allow(clippy::collapsible_if)]
    fn lower_unordered_list(
        children: &[DomNode],
        base_url: Option<&str>,
        out: &mut String,
        indent: usize,
    ) {
        for child in children {
            if let DomNode::Element { tag, children, .. } = child
                && tag == "li"
            {
                // li branch
                // Indent
                for _ in 0..indent {
                    out.push_str("  ");
                }
                out.push_str("- ");
                Self::lower_nodes(children, base_url, out, indent + 1);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }

    /// Lower an ordered list.
    #[allow(clippy::collapsible_if)]
    fn lower_ordered_list(
        children: &[DomNode],
        base_url: Option<&str>,
        out: &mut String,
        indent: usize,
    ) {
        let mut index = 1;
        for child in children {
            if let DomNode::Element { tag, children, .. } = child
                && tag == "li"
            {
                for _ in 0..indent {
                    out.push_str("  ");
                }
                out.push_str(&index.to_string());
                out.push_str(". ");
                Self::lower_nodes(children, base_url, out, indent + 1);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                index += 1;
            }
        }
    }

    /// Lower a table into GFM pipe-table format.
    /// Lower a table. Dispatches to raw HTML for complex tables (colspan,
    /// rowspan, block content in cells) or GFM pipe tables for simple tables.
    fn lower_table(node: &DomNode, base_url: Option<&str>, out: &mut String) {
        let DomNode::Element { children, .. } = node else {
            return;
        };

        // Check if the table is complex
        if table_is_complex(node) {
            // Emit as raw HTML
            let html = serialize_node_to_html(node);
            out.push_str("<div>\n");
            out.push_str(&html);
            out.push_str("\n</div>\n\n");
            return;
        }

        let rows = collect_table_rows(children);
        if rows.is_empty() {
            return;
        }

        // Determine if we have a header
        let has_header = rows.iter().any(|r| has_header_cells(r));

        // Collect all rows data as vectors of strings
        let mut md_rows: Vec<Vec<String>> = Vec::new();
        for row in &rows {
            let cells = collect_cells(row);
            let cell_texts: Vec<String> = cells
                .iter()
                .map(|c| {
                    let text = Self::lower_inline_block(
                        match c {
                            DomNode::Element { children, .. } => children,
                            _ => &[],
                        },
                        base_url,
                    );
                    text.trim().to_string()
                })
                .collect();
            md_rows.push(cell_texts);
        }

        if md_rows.is_empty() {
            return;
        }

        // Determine column count
        let col_count = md_rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if col_count == 0 {
            return;
        }

        // Normalize all rows to the same column count
        for row in &mut md_rows {
            while row.len() < col_count {
                row.push(String::new());
            }
        }

        // Write header
        let header_row = if has_header {
            &md_rows[0]
        } else {
            // Generate empty header
            &vec![String::new(); col_count]
        };

        out.push('|');
        for cell in header_row {
            out.push(' ');
            // Escape pipe characters inside cell content
            let escaped = cell.replace('|', "\\|");
            out.push_str(&escaped);
            out.push_str(" |");
        }
        out.push('\n');

        // Separator row
        out.push('|');
        for _ in 0..col_count {
            out.push_str(" --- |");
        }
        out.push('\n');

        // Data rows
        let data_start = if has_header { 1 } else { 0 };
        for row in &md_rows[data_start..] {
            out.push('|');
            for cell in row {
                out.push(' ');
                let escaped = cell.replace('|', "\\|");
                out.push_str(&escaped);
                out.push_str(" |");
            }
            out.push('\n');
        }

        out.push('\n');
    }

    /// Cap output size to MAX_OUTPUT_SIZE bytes.
    fn cap_size(mut s: String) -> String {
        if s.len() > MAX_OUTPUT_SIZE {
            s.truncate(MAX_OUTPUT_SIZE);
            s.push_str("\n\n[truncated: output exceeded 500 KiB]");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "gen_md_test.rs"]
mod tests;
