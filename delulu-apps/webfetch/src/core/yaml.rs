//! YAML scalar escaping for frontmatter output.
//!
//! `yaml_escape` makes attacker-controlled metadata safe to embed in a YAML
//! frontmatter block. It is a `pub` helper in the library so both the library
//! (`src/lib.rs`) and the binaries' shared markdown module (`src/core/markdown.rs`,
//! included via `#[path]`) can use the same implementation.

/// Make a string safe to embed as a YAML scalar value in the frontmatter.
///
/// Attacker-controlled page metadata (title, author, permalink, source_url)
/// must never break out of the `---` frontmatter block or inject a new
/// `key: value` line. Embedded CR/LF are collapsed onto a single line (as a
/// literal `\n` escape), and values that would otherwise be ambiguous (a
/// leading YAML indicator char, or containing `: ` or `#`) are wrapped in
/// double quotes with internal backslashes/quotes escaped.
pub fn yaml_escape(value: &str) -> String {
    let single_line = value.replace('\r', " ").replace('\n', "\\n");
    let needs_quotes = single_line.starts_with(|c: char| {
        matches!(
            c,
            '-' | '?'
                | ':'
                | '{'
                | '}'
                | '['
                | ']'
                | '#'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '\''
                | '"'
                | '%'
                | '@'
                | '`'
        )
    }) || single_line.contains(": ")
        || single_line.contains('#');
    if needs_quotes {
        let escaped = single_line.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        single_line
    }
}
