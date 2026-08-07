//! Pipeline-agnostic code-block normalization.
//!
//! Runs in EVERY pipeline that feeds the generators (tf, rd, dl_arxiv,
//! dl_doc) so that by the time a DOM reaches `gen_md`/`gen_html` all `<pre>`
//! elements are canonical: block code is structurally `<pre>` (never
//! `pre>code`), the language lives on the pre's own `class`, and the backends
//! stay dumb.

use crate::pipelines::{DomNode, WalkerAction};

// ---------------------------------------------------------------------------
// Code block normalization (pre)
// ---------------------------------------------------------------------------

/// ⚠️ CUSTOM PASS — not present in reference trafilatura (v2.1.0).
/// Improves general code-block markdown serialization: python's `convert_quotes()`
/// renames `pre → code`, losing block-ness in the XML schema; this module keeps
/// `<pre>` so generators emit fenced blocks. Used by the TF pipeline among others.
/// TODO: create a custom webfetch pipeline so that we can have the proper canonical
/// trafilatura behavior in the trafilatura pipeline (this logic belongs in a webfetch-specific
/// pipeline, not baked into the TF pipeline).
/// Normalize code blocks: `<pre>` stays `<pre>` (block code), inline `<code>`
/// stays `<code>`.
///
/// The canonical `pre>code.language-x` shape is rewritten in place — the
/// `<code>` child is unwrapped (its children spliced into the pre) and the
/// language is resolved from the pre's own class or the nested code's class,
/// appended to the pre's class as a single `language-<lang>` token (see
/// [`normalize_pre_block`]). Keeping `<pre>` as the block-code element makes
/// block-ness structural: every backend lowers it without re-deciding inline
/// vs. block (markdown: fenced block; HTML: `<pre>`), and no backend ever
/// needs to inspect `pre>code` nesting.
///
/// Deliberate deviation from python: python's `convert_quotes()` renames
/// `pre` → `code`, losing block-ness in the XML schema — which is why
/// python's own markdown output jams code blocks onto one line. We keep
/// `<pre>` so the backends stay dumb.
pub fn normalize_code_blocks(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } if tag == "pre" => {
            normalize_pre_block(attrs, children);
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

/// First whitespace-delimited `language-*` token in `class`, if any.
///
/// `class="language-python highlight"` yields `python`, not `python highlight`
/// (a multi-token class would produce an invalid fence info string).
fn code_language_from_class(class: &str) -> Option<String> {
    class
        .split_whitespace()
        .find_map(|token| token.strip_prefix("language-"))
        .map(str::to_string)
}

/// Append `language-<lang>` to an element's `class` (creating the attribute
/// if absent, deduplicating existing tokens). Shared by code-block
/// normalization and the code-header-label pass so both hoist the language
/// onto a class with identical token handling.
pub fn push_language_class(attrs: &mut Vec<(String, String)>, lang: &str) {
    let token = format!("language-{lang}");
    if let Some((_, class)) = attrs.iter_mut().find(|(k, _)| k == "class") {
        if !class.split_whitespace().any(|t| t == token) {
            class.push(' ');
            class.push_str(&token);
        }
    } else {
        attrs.push(("class".to_string(), token));
    }
}

/// Normalize a `<pre>` block in place: unwrap a nested `<code>` child and
/// resolve the language onto the pre's own class.
///
/// After this, backends read the language from a single canonical place — the
/// pre's `class` attribute — and the pre holds plain text (or inline markup)
/// directly. The code child's children are spliced into the pre at its
/// position; the pre's other children are untouched.
fn normalize_pre_block(attrs: &mut Vec<(String, String)>, children: &mut Vec<DomNode>) {
    // Resolve the language: the pre's own class takes precedence, then a
    // nested <code> child's class (the canonical pre>code.language-x shape).
    let mut language = attrs.iter().find_map(|(k, v)| {
        if k == "class" {
            code_language_from_class(v)
        } else {
            None
        }
    });
    let mut spliced: Vec<DomNode> = Vec::with_capacity(children.len());
    for child in children.drain(..) {
        if let DomNode::Element {
            tag,
            attrs: code_attrs,
            children: code_children,
            ..
        } = &child
            && tag == "code"
        {
            if language.is_none() {
                language = code_attrs.iter().find_map(|(k, v)| {
                    if k == "class" {
                        code_language_from_class(v)
                    } else {
                        None
                    }
                });
            }
            spliced.extend(code_children.clone());
        } else {
            spliced.push(child);
        }
    }
    *children = spliced;
    // Hoist the language onto the pre's own class (append, dedup).
    if let Some(lang) = language {
        push_language_class(attrs, &lang);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/code_blocks_test.rs"]
mod tests;
