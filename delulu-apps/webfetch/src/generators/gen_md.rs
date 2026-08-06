use crate::pipelines::DomNode;
use std::fmt::Write;

/// Maximum output size in bytes. Output exceeding this is truncated.
const MAX_OUTPUT_SIZE: usize = 500 * 1024; // 500 KiB

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the code language and content from a `<pre>` block.
///
/// The language is read from the `<pre>`'s own `class` first — the tf
/// pipeline's `normalize_pre_block` hoists it there. Un-normalized shapes
/// from other pipelines (rd keeps `pre>code`) fall back to a nested `<code>`
/// child's class. Only the first whitespace-delimited token is used, so a
/// multi-class value like `language-python highlight` yields `python`.
fn extract_code_block(node: &DomNode) -> (String, String) {
    let language = node
        .attr("class")
        .and_then(code_language_token)
        .or_else(|| {
            // Fallback: canonical pre>code.language-x shapes that the tf
            // normalization pass has not seen (e.g. rd pipeline output).
            node.children().iter().find_map(|child| match child {
                DomNode::Element { tag, attrs, .. } if tag == "code" => attrs
                    .iter()
                    .find_map(|(k, v)| (k == "class").then(|| code_language_token(v)).flatten()),
                _ => None,
            })
        })
        .unwrap_or_default();
    let code = node
        .children()
        .iter()
        .map(|c| c.text_content())
        .collect::<String>();
    (language, code)
}

/// First whitespace-delimited `language-*` token in a class value, if any.
fn code_language_token(class: &str) -> Option<String> {
    class
        .split_whitespace()
        .find_map(|token| token.strip_prefix("language-"))
        .map(str::to_string)
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
    if let DomNode::Element { tag, children, .. } = node {
        if tag == "td" || tag == "th" {
            // Check for colspan/rowspan
            if let Some(v) = node.attr("colspan")
                && v != "1"
            {
                return true;
            }
            if let Some(v) = node.attr("rowspan")
                && v != "1"
            {
                return true;
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
    false
}

/// Convert math alttext to LaTeX. Returns None if alttext is empty
/// (caller should render children as fallback).
fn math_to_latex(alttext: &str, display: &str) -> Option<String> {
    if alttext.is_empty() {
        return None;
    }
    if display == "block" || display == "display" {
        Some(format!("$$\\displaystyle {alttext}$$\n"))
    } else {
        Some(format!("${alttext}$"))
    }
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
        DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } => {
            // Special case: convert <math> to inline/display LaTeX
            if tag == "math" {
                let alttext = node.attr("alttext").unwrap_or("");
                let display = node.attr("display").unwrap_or("inline");
                if let Some(latex) = math_to_latex(alttext, display) {
                    return latex;
                }
                // No alttext — render children as fallback
                return children
                    .iter()
                    .map(serialize_node_to_html)
                    .collect::<String>();
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
            // NOTE: '+' and '.' are intentionally NOT escaped: they are only
            // special as line-start list markers in CommonMark, and escaping them
            // everywhere mangles decimals (3.1 -> 3\.1) and '+' signs (30%+).
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
        Self::lower_nodes(std::slice::from_ref(node), base_url, &mut out, 0);
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
    fn lower_element(node: &DomNode, base_url: Option<&str>, out: &mut String, indent: usize) {
        let DomNode::Element { tag, children, .. } = node else {
            return;
        };
        match tag.as_str() {
            // ── Headings ───────────────────────────────────────────────
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag[1..].parse::<usize>().unwrap_or(1);
                let prefix = "#".repeat(level);
                let text = children
                    .iter()
                    .map(|c| c.text_content())
                    .collect::<String>();
                out.push_str(&prefix);
                out.push(' ');
                out.push_str(&text);
                out.push_str("\n\n");
            }

            // ── Trafilatura headings: tf_convert_headings renames h1-h6 ->
            // <head rend="h1..h6"> (trafilatura's XML schema). Render as
            // markdown headings, not plain text, or the heading text jams
            // onto the following paragraph ("FAQQuick answers").
            "head" => {
                let rend = node.attr("rend").unwrap_or("");
                if let Some(level) = rend.strip_prefix('h').and_then(|n| n.parse::<usize>().ok())
                    && (1..=6).contains(&level)
                {
                    let prefix = "#".repeat(level);
                    let text = children
                        .iter()
                        .map(|c| c.text_content())
                        .collect::<String>();
                    // Block-level output: ensure the heading lands at
                    // line-start even when the preceding sibling was loose
                    // text — a `### ` marker glued mid-line is body text, not
                    // a CommonMark heading.
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&prefix);
                    out.push(' ');
                    out.push_str(&text);
                    out.push_str("\n\n");
                } else {
                    // Non-heading <head> (e.g. document head) — render text only.
                    Self::lower_nodes(children, base_url, out, indent);
                }
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
                let href = node.attr("href").unwrap_or("");
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
            // "ref" elements carry links in trafilatura's internal schema:
            // tf_convert_refs_and_details renames <a href> -> <ref target>.
            // Render them as markdown links so include_links is on by default
            // (python trafilatura defaults include_links=False; we want links).
            "ref" => {
                let target = node.attr("target").unwrap_or("");
                let text = Self::lower_inline(children, base_url);
                if target.is_empty() {
                    out.push_str(&text);
                } else {
                    let resolved = resolve_url(target, base_url);
                    out.push('[');
                    out.push_str(&text);
                    out.push_str("](");
                    out.push_str(&resolved);
                    out.push(')');
                }
            }

            // ── Images ─────────────────────────────────────────────────
            "img" => {
                let src = node.attr("src").unwrap_or("");
                let alt = node.attr("alt").unwrap_or("");
                let resolved = resolve_url(src, base_url);
                out.push('!');
                out.push('[');
                out.push_str(alt);
                out.push_str("](");
                out.push_str(&resolved);
                out.push(')');
            }

            // ── Unordered lists ────────────────────────────────────────
            "ul" | "list" => {
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
                // Block code is structurally `<pre>` (the tf pipeline keeps
                // pre as pre and hoists the language into its class); lower it
                // as a fenced block. Block output must land at line-start even
                // when the preceding sibling is loose text (e.g. a "BASH"
                // code-block header label) — `BASH```` is not a valid
                // CommonMark fence opener.
                let (lang, code) = extract_code_block(node);
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
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
                // Inline code is structurally `<code>`; render as inline
                // backticks, collapsing newlines to spaces so a degenerate
                // multi-line inline <code> stays valid inline markdown.
                let text = children
                    .iter()
                    .map(|c| c.text_content())
                    .collect::<String>();
                out.push('`');
                out.push_str(&text.replace('\n', " "));
                out.push('`');
            }

            // ── Math (LaTeXML MathML) ────────────────────────────────
            "math" => {
                let alttext = node.attr("alttext").unwrap_or("");
                let display = node.attr("display").unwrap_or("inline");
                if let Some(latex) = math_to_latex(alttext, display) {
                    out.push_str(&latex);
                } else {
                    // Fallback: render text content
                    let text = children
                        .iter()
                        .map(|c| c.text_content())
                        .collect::<String>();
                    out.push_str(&text);
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
            // Unknown containers render their children inline; block-ness is
            // carried by the tags themselves (pre stays pre), so there is no
            // context to thread.
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
                && (tag == "li" || tag == "item")
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
                && (tag == "li" || tag == "item")
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
#[path = "../../tests/unit/generators/gen_md_test.rs"]
mod tests;
