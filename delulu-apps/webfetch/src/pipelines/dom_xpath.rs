//! Minimal XPath 1.0 evaluator supporting the trafilatura subset.
//!
//! Supports:
//! - 3 axes: `child::`, `descendant-or-self::`, `self::`
//! - 7 functions: `re:test`, `contains`, `starts-with`, `translate`, `@attr`, `position()`, `text()`
//! - `or` / `|` operators
//! - `[N]` position predicates
//!
//! Reference: Trafilatura `_references_fetch/trafilatura/trafilatura/xpaths.py`

use regex::Regex;
use std::collections::HashMap;

use crate::pipelines::DomNode;

// ---------------------------------------------------------------------------
// XPathError — Phase 0b
// ---------------------------------------------------------------------------

/// Errors that can occur during XPath parsing and evaluation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum XPathError {
    // Parse errors
    /// Unterminated string literal at the given position.
    #[error("unterminated string at position {position}")]
    UnterminatedString { position: usize },
    /// Unexpected token at the given position.
    #[error("unexpected token at position {position}: expected {expected}, found {found}")]
    UnexpectedToken { expected: &'static str, found: String, position: usize },
    /// Empty XPath expression.
    #[error("empty XPath expression")]
    EmptyExpression,
    /// Nesting limit exceeded during parsing.
    #[error("nesting limit exceeded (max depth: {max_depth})")]
    NestingLimitExceeded { max_depth: usize },
    // Evaluation errors
    /// Unknown function name.
    #[error("unknown function: {name}")]
    InvalidFunction { name: String },
    /// Wrong number of arguments to a function.
    #[error("wrong argument count for {function}: expected {expected}, found {found}")]
    WrongArgumentCount { function: &'static str, expected: usize, found: usize },
    /// Type mismatch during evaluation.
    #[error("type mismatch: expected {expected}, found {found}")]
    TypeMismatch { expected: &'static str, found: &'static str },
    /// Maximum evaluation depth exceeded.
    #[error("max depth exceeded (depth: {depth}, max: {max_depth})")]
    MaxDepthExceeded { depth: usize, max_depth: usize },
    // Regex errors
    /// Invalid regex pattern.
    #[error("invalid regex pattern '{pattern}': {source}")]
    InvalidRegex { pattern: String, source: regex::Error },
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum depth for the XPath evaluator's tree traversal.
pub const MAX_XPATH_DEPTH: usize = 1000;

/// Maximum nesting depth for parsing expressions.
const MAX_PARSE_DEPTH: usize = 100;

// ---------------------------------------------------------------------------
// Tokenizer — Phase 1a
// ---------------------------------------------------------------------------

/// Tokens recognized by the XPath tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum XPathToken {
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `@`
    At,
    /// `.`
    Dot,
    /// `//`
    DoubleSlash,
    /// `/`
    Slash,
    /// `::`
    DoubleColon,
    /// `:`
    Colon,
    /// `|`
    Pipe,
    /// `or`
    Or,
    /// `and`
    And,
    /// `=`
    Equals,
    /// `!=`
    NotEquals,
    /// `,`
    Comma,
    /// A name (identifier, function name, or namespace prefix)
    Name(String),
    /// A string literal (single or double quoted)
    StringLiteral(String),
    /// A number literal
    NumberLiteral(f64),
    /// `*` (wildcard)
    Star,
}

/// Tokenize an XPath expression string into a sequence of tokens.
///
/// Pre: `input` is a valid UTF-8 string.
/// Post: Returns a `Vec<XPathToken>` representing the lexed expression.
///
/// Reference: Standard XPath 1.0 tokenization rules.
pub fn tokenize(input: &str) -> Result<Vec<XPathToken>, XPathError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < chars.len() {
        let ch = chars[pos];

        // Skip whitespace
        if ch.is_ascii_whitespace() {
            pos += 1;
            continue;
        }

        match ch {
            '(' => { tokens.push(XPathToken::LParen); pos += 1; }
            ')' => { tokens.push(XPathToken::RParen); pos += 1; }
            '[' => { tokens.push(XPathToken::LBracket); pos += 1; }
            ']' => { tokens.push(XPathToken::RBracket); pos += 1; }
            '@' => { tokens.push(XPathToken::At); pos += 1; }
            '.' => {
                if pos + 1 < chars.len() && chars[pos + 1].is_ascii_digit() {
                    let num = scan_number(&chars, &mut pos);
                    tokens.push(XPathToken::NumberLiteral(num));
                } else {
                    tokens.push(XPathToken::Dot);
                    pos += 1;
                }
            }
            ',' => { tokens.push(XPathToken::Comma); pos += 1; }
            '|' => { tokens.push(XPathToken::Pipe); pos += 1; }
            '*' => { tokens.push(XPathToken::Star); pos += 1; }
            '/' => {
                if pos + 1 < chars.len() && chars[pos + 1] == '/' {
                    tokens.push(XPathToken::DoubleSlash);
                    pos += 2;
                } else {
                    tokens.push(XPathToken::Slash);
                    pos += 1;
                }
            }
            ':' => {
                if pos + 1 < chars.len() && chars[pos + 1] == ':' {
                    tokens.push(XPathToken::DoubleColon);
                    pos += 2;
                } else {
                    tokens.push(XPathToken::Colon);
                    pos += 1;
                }
            }
            '\'' | '"' => {
                let quote = ch;
                let start = pos;
                pos += 1;
                let mut s = String::new();
                while pos < chars.len() && chars[pos] != quote {
                    s.push(chars[pos]);
                    pos += 1;
                }
                if pos >= chars.len() {
                    return Err(XPathError::UnterminatedString { position: start });
                }
                pos += 1; // skip closing quote
                tokens.push(XPathToken::StringLiteral(s));
            }
            '=' => { tokens.push(XPathToken::Equals); pos += 1; }
            '!' => {
                if pos + 1 < chars.len() && chars[pos + 1] == '=' {
                    tokens.push(XPathToken::NotEquals);
                    pos += 2;
                } else {
                    return Err(XPathError::UnexpectedToken {
                        expected: "!= or name",
                        found: format!("! at position {}", pos),
                        position: pos,
                    });
                }
            }
            _ => {
                if ch.is_ascii_digit() || (ch == '-' && pos + 1 < chars.len() && chars[pos + 1].is_ascii_digit()) {
                    let num = scan_number(&chars, &mut pos);
                    tokens.push(XPathToken::NumberLiteral(num));
                } else if is_name_start_char(ch) {
                    let name = scan_name(&chars, &mut pos);
                    // Check for keywords
                    match name.as_str() {
                        "or" => tokens.push(XPathToken::Or),
                        "and" => tokens.push(XPathToken::And),
                        _ => tokens.push(XPathToken::Name(name)),
                    }
                } else {
                    return Err(XPathError::UnexpectedToken {
                        expected: "valid XPath token",
                        found: format!("'{}' at position {}", ch, pos),
                        position: pos,
                    });
                }
            }
        }
    }

    Ok(tokens)
}

fn is_name_start_char(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
}

fn scan_name(chars: &[char], pos: &mut usize) -> String {
    let start = *pos;
    while *pos < chars.len() && is_name_char(chars[*pos]) {
        *pos += 1;
    }
    chars[start..*pos].iter().collect()
}

fn scan_number(chars: &[char], pos: &mut usize) -> f64 {
    let start = *pos;
    if chars[*pos] == '-' { *pos += 1; }
    while *pos < chars.len() && chars[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos < chars.len() && chars[*pos] == '.' {
        *pos += 1;
        while *pos < chars.len() && chars[*pos].is_ascii_digit() {
            *pos += 1;
        }
    }
    let s: String = chars[start..*pos].iter().collect();
    s.parse::<f64>().unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// AST — Phase 1a
// ---------------------------------------------------------------------------

/// An XPath expression.
#[derive(Debug, Clone)]
pub enum XPathExpr {
    /// A path expression (e.g., `div/p`, `.//article`)
    Path(PathExpr),
    /// A union of two expressions (`|`)
    Union(Box<XPathExpr>, Box<XPathExpr>),
    /// An or-expression (`or`)
    Or(Box<XPathExpr>, Box<XPathExpr>),
    /// An equality comparison (`=`)
    Equals(Box<XPathExpr>, Box<XPathExpr>),
    /// An inequality comparison (`!=`)
    NotEquals(Box<XPathExpr>, Box<XPathExpr>),
    /// A function call
    FunctionCall {
        name: String,
        args: Vec<XPathExpr>,
    },
    /// An attribute access (e.g., `@class`)
    Attribute(String),
    /// A literal string
    Literal(String),
    /// A literal number
    Number(f64),
    /// The `text()` node test or function
    TextNode,
    /// The `position()` function
    Position,
    /// A predicate expression
    Predicate(Box<XPathExpr>),
}

/// A path expression: a sequence of steps separated by `/` or `//`.
#[derive(Debug, Clone)]
pub struct PathExpr {
    /// The initial step or expression (e.g., `.` or a step)
    pub initial: Box<XPathExpr>,
    /// Subsequent steps
    pub steps: Vec<Step>,
}

/// A single step in a path expression.
#[derive(Debug, Clone)]
pub struct Step {
    /// The axis (child, descendant-or-self, self)
    pub axis: Axis,
    /// The node test
    pub node_test: NodeTest,
    /// Predicates
    pub predicates: Vec<Predicate>,
}

/// An axis specifier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Axis {
    /// `child::` (default)
    Child,
    /// `descendant-or-self::`
    DescendantOrSelf,
    /// `self::`
    SelfAxis,
}

/// A node test in a step.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeTest {
    /// Any element (`*`)
    Any,
    /// A specific element name (e.g., `div`, `p`)
    Name(String),
    /// `text()` as a node test
    Text,
    /// `comment()` as a node test
    Comment,
}

/// A predicate expression.
#[derive(Debug, Clone)]
pub struct Predicate {
    pub expr: Box<XPathExpr>,
}

/// An argument to an XPath function (can be a node-set, string, or number).
#[derive(Debug, Clone)]
pub enum XPathArg {
    NodeSet(Vec<DomNode>),
    String(String),
    Number(f64),
}

// ---------------------------------------------------------------------------
// Parser — Phase 1a
// ---------------------------------------------------------------------------

/// Parse a sequence of tokens into an XPath expression.
///
/// Pre: `tokens` is a valid sequence of XPath tokens from `tokenize()`.
/// Post: Returns an `XPathExpr` AST if parsing succeeds.
///
/// Reference: Standard XPath 1.0 parsing rules (simplified).
pub fn parse(tokens: &[XPathToken]) -> Result<XPathExpr, XPathError> {
    if tokens.is_empty() {
        return Err(XPathError::EmptyExpression);
    }
    let mut pos = 0;
    parse_or_expr(tokens, &mut pos, 0)
}

fn parse_or_expr(tokens: &[XPathToken], pos: &mut usize, depth: usize) -> Result<XPathExpr, XPathError> {
    if depth > MAX_PARSE_DEPTH {
        return Err(XPathError::NestingLimitExceeded { max_depth: MAX_PARSE_DEPTH });
    }
    let mut left = parse_equality_expr(tokens, pos, depth + 1)?;
    while *pos < tokens.len() && tokens[*pos] == XPathToken::Or {
        *pos += 1;
        let right = parse_equality_expr(tokens, pos, depth + 1)?;
        left = XPathExpr::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

/// Parse equality expression: UnionExpr (('=' | '!=') UnionExpr)*
fn parse_equality_expr(tokens: &[XPathToken], pos: &mut usize, depth: usize) -> Result<XPathExpr, XPathError> {
    if depth > MAX_PARSE_DEPTH {
        return Err(XPathError::NestingLimitExceeded { max_depth: MAX_PARSE_DEPTH });
    }
    let mut left = parse_union_expr(tokens, pos, depth + 1)?;
    while *pos < tokens.len() {
        match &tokens[*pos] {
            XPathToken::Equals => {
                *pos += 1;
                let right = parse_union_expr(tokens, pos, depth + 1)?;
                left = XPathExpr::Equals(Box::new(left), Box::new(right));
            }
            XPathToken::NotEquals => {
                *pos += 1;
                let right = parse_union_expr(tokens, pos, depth + 1)?;
                left = XPathExpr::NotEquals(Box::new(left), Box::new(right));
            }
            _ => break,
        }
    }
    Ok(left)
}

fn parse_union_expr(tokens: &[XPathToken], pos: &mut usize, depth: usize) -> Result<XPathExpr, XPathError> {
    if depth > MAX_PARSE_DEPTH {
        return Err(XPathError::NestingLimitExceeded { max_depth: MAX_PARSE_DEPTH });
    }
    let mut left = parse_path_expr(tokens, pos, depth + 1)?;
    while *pos < tokens.len() && tokens[*pos] == XPathToken::Pipe {
        *pos += 1;
        let right = parse_path_expr(tokens, pos, depth + 1)?;
        left = XPathExpr::Union(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_path_expr(tokens: &[XPathToken], pos: &mut usize, depth: usize) -> Result<XPathExpr, XPathError> {
    if depth > MAX_PARSE_DEPTH {
        return Err(XPathError::NestingLimitExceeded { max_depth: MAX_PARSE_DEPTH });
    }

    // Check for leading / or //
    let _initial_axis = if *pos < tokens.len() {
        match &tokens[*pos] {
            XPathToken::Slash => { *pos += 1; Some(Axis::Child) }
            XPathToken::DoubleSlash => { *pos += 1; Some(Axis::DescendantOrSelf) }
            _ => None,
        }
    } else {
        None
    };

    if *pos >= tokens.len() {
        return Err(XPathError::UnexpectedToken {
            expected: "step or expression after /",
            found: "end of input".to_string(),
            position: *pos,
        });
    }

    // Parse the first step or primary expression
    let initial = parse_step_or_primary(tokens, pos, depth + 1)?;

    // Parse subsequent steps
    let expr = match initial {
        XPathExpr::Path(pe) => {
            let mut steps = pe.steps;
            // Parse remaining /step or //step
            while *pos < tokens.len() {
                match &tokens[*pos] {
                    XPathToken::Slash => {
                        *pos += 1;
                        if *pos >= tokens.len() {
                            return Err(XPathError::UnexpectedToken {
                                expected: "step after /",
                                found: "end of input".to_string(),
                                position: *pos,
                            });
                        }
                        let step = parse_step(tokens, pos, Axis::Child, depth + 1)?;
                        steps.push(step);
                    }
                    XPathToken::DoubleSlash => {
                        *pos += 1;
                        let step = parse_step(tokens, pos, Axis::DescendantOrSelf, depth + 1)?;
                        steps.push(step);
                    }
                    _ => break,
                }
            }
            XPathExpr::Path(PathExpr {
                initial: pe.initial,
                steps,
            })
        }
        other => other,
    };

    Ok(expr)
}

fn parse_step_or_primary(tokens: &[XPathToken], pos: &mut usize, depth: usize) -> Result<XPathExpr, XPathError> {
    if depth > MAX_PARSE_DEPTH {
        return Err(XPathError::NestingLimitExceeded { max_depth: MAX_PARSE_DEPTH });
    }

    if *pos >= tokens.len() {
        return Err(XPathError::UnexpectedToken {
            expected: "expression",
            found: "end of input".to_string(),
            position: *pos,
        });
    }

    match &tokens[*pos] {
        XPathToken::LParen => {
            *pos += 1;
            let expr = parse_or_expr(tokens, pos, depth + 1)?;
            if *pos >= tokens.len() || tokens[*pos] != XPathToken::RParen {
                return Err(XPathError::UnexpectedToken {
                    expected: "')'",
                    found: if *pos < tokens.len() { format!("{:?}", tokens[*pos]) } else { "end of input".to_string() },
                    position: *pos,
                });
            }
            *pos += 1;
            Ok(expr)
        }
        XPathToken::Dot => {
            *pos += 1;
            // self::node()
            Ok(XPathExpr::Path(PathExpr {
                initial: Box::new(XPathExpr::Literal(String::new())),
                steps: vec![Step {
                    axis: Axis::SelfAxis,
                    node_test: NodeTest::Any,
                    predicates: Vec::new(),
                }],
            }))
        }
        XPathToken::Name(name) => {
            let name = name.clone();
            *pos += 1;

            // Check for axis::node-test
            if *pos < tokens.len() && tokens[*pos] == XPathToken::DoubleColon {
                *pos += 1;
                let axis = match name.as_str() {
                    "child" => Axis::Child,
                    "descendant-or-self" => Axis::DescendantOrSelf,
                    "self" => Axis::SelfAxis,
                    other => return Err(XPathError::UnexpectedToken {
                        expected: "child, descendant-or-self, or self",
                        found: format!("axis '{}'", other),
                        position: *pos - 2 - name.len(),
                    }),
                };
                let step = parse_step(tokens, pos, axis, depth + 1)?;
                Ok(XPathExpr::Path(PathExpr {
                    initial: Box::new(XPathExpr::Literal(String::new())),
                    steps: vec![step],
                }))
            } else if *pos < tokens.len() && tokens[*pos] == XPathToken::LParen {
                // Function call
                *pos += 1;
                let mut args = Vec::new();
                if *pos < tokens.len() && tokens[*pos] != XPathToken::RParen {
                    args.push(parse_or_expr(tokens, pos, depth + 1)?);
                    while *pos < tokens.len() && tokens[*pos] == XPathToken::Comma {
                        *pos += 1;
                        args.push(parse_or_expr(tokens, pos, depth + 1)?);
                    }
                }
                if *pos >= tokens.len() || tokens[*pos] != XPathToken::RParen {
                    return Err(XPathError::UnexpectedToken {
                        expected: "')'",
                        found: if *pos < tokens.len() { format!("{:?}", tokens[*pos]) } else { "end of input".to_string() },
                        position: *pos,
                    });
                }
                *pos += 1;
                Ok(XPathExpr::FunctionCall { name, args })
            } else {
                // It's a node test (element name) — treat as child::name
                let mut step = Step {
                    axis: Axis::Child,
                    node_test: NodeTest::Name(name),
                    predicates: Vec::new(),
                };
                // Parse predicates after the node test
                while *pos < tokens.len() && tokens[*pos] == XPathToken::LBracket {
                    *pos += 1;
                    let expr = parse_or_expr(tokens, pos, depth + 1)?;
                    if *pos >= tokens.len() || tokens[*pos] != XPathToken::RBracket {
                        return Err(XPathError::UnexpectedToken {
                            expected: "']'",
                            found: if *pos < tokens.len() { format!("{:?}", tokens[*pos]) } else { "end of input".to_string() },
                            position: *pos,
                        });
                    }
                    *pos += 1;
                    step.predicates.push(Predicate { expr: Box::new(expr) });
                }
                Ok(XPathExpr::Path(PathExpr {
                    initial: Box::new(XPathExpr::Literal(String::new())),
                    steps: vec![step],
                }))
            }
        }
        XPathToken::At => {
            *pos += 1;
            if *pos >= tokens.len() {
                return Err(XPathError::UnexpectedToken {
                    expected: "attribute name after @",
                    found: "end of input".to_string(),
                    position: *pos,
                });
            }
            if let XPathToken::Name(name) = &tokens[*pos] {
                let name = name.clone();
                *pos += 1;
                Ok(XPathExpr::Attribute(name))
            } else {
                return Err(XPathError::UnexpectedToken {
                    expected: "attribute name",
                    found: format!("{:?}", tokens[*pos]),
                    position: *pos,
                });
            }
        }
        XPathToken::StringLiteral(s) => {
            let s = s.clone();
            *pos += 1;
            Ok(XPathExpr::Literal(s))
        }
        XPathToken::NumberLiteral(n) => {
            let n = *n;
            *pos += 1;
            Ok(XPathExpr::Number(n))
        }
        XPathToken::Star => {
            *pos += 1;
            let step = Step {
                axis: Axis::Child,
                node_test: NodeTest::Any,
                predicates: Vec::new(),
            };
            Ok(XPathExpr::Path(PathExpr {
                initial: Box::new(XPathExpr::Literal(String::new())),
                steps: vec![step],
            }))
        }
        XPathToken::LBracket => {
            *pos += 1;
            let expr = parse_or_expr(tokens, pos, depth + 1)?;
            if *pos >= tokens.len() || tokens[*pos] != XPathToken::RBracket {
                return Err(XPathError::UnexpectedToken {
                    expected: "']'",
                    found: if *pos < tokens.len() { format!("{:?}", tokens[*pos]) } else { "end of input".to_string() },
                    position: *pos,
                });
            }
            *pos += 1;
            Ok(XPathExpr::Predicate(Box::new(expr)))
        }
        _ => Err(XPathError::UnexpectedToken {
            expected: "expression (name, @attr, (, ., string, number)",
            found: format!("{:?}", tokens[*pos]),
            position: *pos,
        }),
    }
}

fn parse_step(tokens: &[XPathToken], pos: &mut usize, axis: Axis, depth: usize) -> Result<Step, XPathError> {
    if depth > MAX_PARSE_DEPTH {
        return Err(XPathError::NestingLimitExceeded { max_depth: MAX_PARSE_DEPTH });
    }

    let node_test = if *pos < tokens.len() {
        match &tokens[*pos] {
            XPathToken::Name(name) => {
                let name = name.clone();
                *pos += 1;
                // Check for text() as node test
                if name == "text" && *pos < tokens.len() && tokens[*pos] == XPathToken::LParen {
                    *pos += 1;
                    if *pos >= tokens.len() || tokens[*pos] != XPathToken::RParen {
                        return Err(XPathError::UnexpectedToken {
                            expected: "')' after text(",
                            found: if *pos < tokens.len() { format!("{:?}", tokens[*pos]) } else { "end of input".to_string() },
                            position: *pos,
                        });
                    }
                    *pos += 1;
                    NodeTest::Text
                } else if name == "node" && *pos < tokens.len() && tokens[*pos] == XPathToken::LParen {
                    *pos += 1;
                    if *pos >= tokens.len() || tokens[*pos] != XPathToken::RParen {
                        return Err(XPathError::UnexpectedToken {
                            expected: "')' after node(",
                            found: format!("{:?}", tokens[*pos]),
                            position: *pos,
                        });
                    }
                    *pos += 1;
                    NodeTest::Any
                } else if name == "comment" && *pos < tokens.len() && tokens[*pos] == XPathToken::LParen {
                    *pos += 1;
                    if *pos >= tokens.len() || tokens[*pos] != XPathToken::RParen {
                        return Err(XPathError::UnexpectedToken {
                            expected: "')' after comment(",
                            found: format!("{:?}", tokens[*pos]),
                            position: *pos,
                        });
                    }
                    *pos += 1;
                    NodeTest::Comment
                } else {
                    NodeTest::Name(name)
                }
            }
            XPathToken::Star => {
                *pos += 1;
                NodeTest::Any
            }
            XPathToken::Dot => {
                // self::node()
                *pos += 1;
                NodeTest::Any
            }
            XPathToken::At => {
                return Err(XPathError::UnexpectedToken {
                    expected: "node test",
                    found: "@".to_string(),
                    position: *pos,
                });
            }
            _ => return Err(XPathError::UnexpectedToken {
                expected: "node test (name, *, text(), node())",
                found: format!("{:?}", tokens[*pos]),
                position: *pos,
            }),
        }
    } else {
        return Err(XPathError::UnexpectedToken {
            expected: "node test",
            found: "end of input".to_string(),
            position: *pos,
        });
    };

    // Parse predicates
    let mut predicates = Vec::new();
    while *pos < tokens.len() && tokens[*pos] == XPathToken::LBracket {
        *pos += 1;
        let expr = parse_or_expr(tokens, pos, depth + 1)?;
        if *pos >= tokens.len() || tokens[*pos] != XPathToken::RBracket {
            return Err(XPathError::UnexpectedToken {
                expected: "']'",
                found: if *pos < tokens.len() { format!("{:?}", tokens[*pos]) } else { "end of input".to_string() },
                position: *pos,
            });
        }
        *pos += 1;
        predicates.push(Predicate { expr: Box::new(expr) });
    }

    Ok(Step { axis, node_test, predicates })
}

// ---------------------------------------------------------------------------
// XPath Compiled Expression — Phase 1b
// ---------------------------------------------------------------------------

/// A compiled XPath expression, ready for evaluation.
///
/// Pre-compiled at module init time via `Lazy<XPath>`. Contains the parsed AST
/// and pre-compiled regex patterns for `re:test()` calls.
pub struct XPath {
    /// The parsed expression
    pub compiled: XPathExpr,
    /// Pre-compiled regex patterns (in order of re:test calls)
    pub regex_cache: Vec<(String, Regex)>,
}

impl XPath {
    /// Compile an XPath expression string.
    ///
    /// Tokenizes, parses, and extracts all `re:test` regex patterns for pre-compilation.
    ///
    /// # Panics
    ///
    /// This function returns `Result` — panics are deferred to the caller via `.expect()`.
    /// Typically called in `Lazy<XPath>::new(|| XPath::compile("...").expect("..."))`.
    pub fn compile(expr: &str) -> Result<Self, XPathError> {
        // Pre-process: handle re:test naming
        let processed = if expr.contains("re:test") {
            expr.replace("re:test", "re_test_placeholder")
        } else {
            expr.to_string()
        };

        let tokens = tokenize(&processed)?;
        let mut ast = parse(&tokens)?;

        // Restore function names and extract regex patterns
        let mut regex_cache = Vec::new();
        restore_re_test(&mut ast, &mut regex_cache)?;

        Ok(XPath {
            compiled: ast,
            regex_cache,
        })
    }

    /// Evaluate the compiled XPath expression against a DOM node.
    ///
    /// Returns a `Vec` of matching DOM node references in document order.
    /// Returns an empty `Vec` (not an error) when no nodes match.
    /// Returns `Err` for recoverable runtime errors (e.g., MAX_DEPTH exceeded).
    ///
    /// Pre: `node` is a valid DOM tree.
    /// Post: Matching nodes are returned in document order.
    pub fn eval<'a>(&self, node: &'a DomNode) -> Result<Vec<&'a DomNode>, XPathError> {
        let result = eval_expr(&self.compiled, node, node, &self.regex_cache, 0)?;
        Ok(result)
    }
}

/// Restore re:test function names and pre-compile regex patterns.
fn restore_re_test(expr: &mut XPathExpr, regex_cache: &mut Vec<(String, Regex)>) -> Result<(), XPathError> {
    match expr {
        XPathExpr::FunctionCall { name, args } => {
            if name == "re_test_placeholder" {
                *name = "re:test".to_string();
                if args.len() >= 2 {
                    if let XPathExpr::Literal(pattern) = &args[1] {
                        let re = Regex::new(pattern).map_err(|e| XPathError::InvalidRegex {
                            pattern: pattern.clone(),
                            source: e,
                        })?;
                        regex_cache.push((pattern.clone(), re));
                    }
                }
            }
            for arg in args.iter_mut() {
                restore_re_test(arg, regex_cache)?;
            }
        }
        XPathExpr::Path(pe) => {
            restore_re_test(&mut pe.initial, regex_cache)?;
            for step in pe.steps.iter_mut() {
                for pred in step.predicates.iter_mut() {
                    restore_re_test(&mut pred.expr, regex_cache)?;
                }
            }
        }
        XPathExpr::Union(left, right) | XPathExpr::Or(left, right) => {
            restore_re_test(left, regex_cache)?;
            restore_re_test(right, regex_cache)?;
        }
        XPathExpr::Equals(left, right) | XPathExpr::NotEquals(left, right) => {
            restore_re_test(left, regex_cache)?;
            restore_re_test(right, regex_cache)?;
        }
        XPathExpr::Predicate(inner) => {
            restore_re_test(inner, regex_cache)?;
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper: pointer-based node identity
// ---------------------------------------------------------------------------

/// Check if a node pointer is in a slice using pointer identity.
fn contains_ptr<'a>(v: &[&'a DomNode], target: &'a DomNode) -> bool {
    v.iter().any(|n| std::ptr::eq(*n, target))
}

// ---------------------------------------------------------------------------
// Evaluator — Phase 1b
// ---------------------------------------------------------------------------

/// Evaluate an XPath expression and return matching nodes.
fn eval_expr<'a>(
    expr: &XPathExpr,
    context: &'a DomNode,
    root: &'a DomNode,
    regex_cache: &[(String, Regex)],
    depth: usize,
) -> Result<Vec<&'a DomNode>, XPathError> {
    if depth > MAX_XPATH_DEPTH {
        return Err(XPathError::MaxDepthExceeded {
            depth,
            max_depth: MAX_XPATH_DEPTH,
        });
    }

    match expr {
        XPathExpr::Path(pe) => eval_path(pe, context, root, regex_cache, depth),
        XPathExpr::Union(left, right) => {
            let mut l = eval_expr(left, context, root, regex_cache, depth + 1)?;
            let r = eval_expr(right, context, root, regex_cache, depth + 1)?;
            // Merge in document order
            for node in r {
                if !contains_ptr(&l, node) {
                    l.push(node);
                }
            }
            // Sort by document order (pre-order)
            sort_by_document_order(&mut l, root);
            Ok(l)
        }
        XPathExpr::Or(left, right) => {
            let l = eval_expr(left, context, root, regex_cache, depth + 1)?;
            if !l.is_empty() {
                return Ok(l);
            }
            eval_expr(right, context, root, regex_cache, depth + 1)
        }
        XPathExpr::FunctionCall { name, args } => {
            eval_function(name, args, context, root, regex_cache, depth)
        }
        XPathExpr::Attribute(_) => {
            // Return the context node as a marker
            Ok(vec![context])
        }
        XPathExpr::Literal(_) => Ok(vec![]),
        XPathExpr::Number(_) => Ok(vec![]),
        XPathExpr::TextNode => {
            let mut result = Vec::new();
            collect_text_children(context, &mut result);
            Ok(result)
        }
        XPathExpr::Position => Ok(vec![]),
        XPathExpr::Equals(left, right) => {
            let l_val = eval_expr(left, context, root, regex_cache, depth + 1)?;
            let r_val = eval_expr(right, context, root, regex_cache, depth + 1)?;
            if l_val.is_empty() && r_val.is_empty() {
                Ok(vec![])
            } else {
                let l_str = string_value_of_expr(left, &l_val, root, regex_cache, depth + 1)?;
                let r_str = string_value_of_expr(right, &r_val, root, regex_cache, depth + 1)?;
                if l_str == r_str { Ok(vec![context]) } else { Ok(vec![]) }
            }
        }
        XPathExpr::NotEquals(left, right) => {
            let l_val = eval_expr(left, context, root, regex_cache, depth + 1)?;
            let r_val = eval_expr(right, context, root, regex_cache, depth + 1)?;
            if l_val.is_empty() && r_val.is_empty() {
                Ok(vec![])
            } else {
                let l_str = string_value_of_expr(left, &l_val, root, regex_cache, depth + 1)?;
                let r_str = string_value_of_expr(right, &r_val, root, regex_cache, depth + 1)?;
                if l_str != r_str { Ok(vec![context]) } else { Ok(vec![]) }
            }
        }
        XPathExpr::Predicate(_) => Ok(vec![]),
    }
}

/// Evaluate a path expression.
fn eval_path<'a>(
    pe: &PathExpr,
    context: &'a DomNode,
    root: &'a DomNode,
    regex_cache: &[(String, Regex)],
    depth: usize,
) -> Result<Vec<&'a DomNode>, XPathError> {
    if depth > MAX_XPATH_DEPTH {
        return Err(XPathError::MaxDepthExceeded { depth, max_depth: MAX_XPATH_DEPTH });
    }

    // Start with the context node(s) from the initial expression
    let mut current = eval_expr(&pe.initial, context, root, regex_cache, depth + 1)?;
    if current.is_empty() {
        // If initial is empty (e.g., Literal("") for a step-based path), start with context
        current = vec![context];
    }

    // Apply each step
    for step in &pe.steps {
        let mut next = Vec::new();
        for node in &current {
            let matched = eval_step(node, step, root, regex_cache, depth + 1)?;
            next.extend(matched);
        }
        current = next;
    }

    Ok(current)
}

/// Evaluate a single step.
fn eval_step<'a>(
    node: &'a DomNode,
    step: &Step,
    root: &'a DomNode,
    regex_cache: &[(String, Regex)],
    depth: usize,
) -> Result<Vec<&'a DomNode>, XPathError> {
    if depth > MAX_XPATH_DEPTH {
        return Err(XPathError::MaxDepthExceeded { depth, max_depth: MAX_XPATH_DEPTH });
    }

    let candidates = match step.axis {
        Axis::Child => get_children(node),
        Axis::DescendantOrSelf => {
            let mut v = vec![node];
            v.extend(dom_descendants(node));
            v
        }
        Axis::SelfAxis => vec![node],
    };

    // Filter by node test
    let matched: Vec<&'a DomNode> = candidates.into_iter().filter(|n| match_node_test(n, &step.node_test)).collect();

    // Apply predicates
    let mut result = matched;
    for pred in &step.predicates {
        result = apply_predicate(&result, &pred.expr, root, regex_cache, depth + 1)?;
    }

    Ok(result)
}

/// Check if a node matches a node test.
fn match_node_test(node: &DomNode, test: &NodeTest) -> bool {
    match test {
        NodeTest::Any => matches!(node, DomNode::Element { .. }),
        NodeTest::Name(name) => {
            matches!(node, DomNode::Element { tag, .. } if tag == name)
        }
        NodeTest::Text => matches!(node, DomNode::Text(_)),
        NodeTest::Comment => matches!(node, DomNode::Comment(_)),
    }
}

/// Apply a predicate expression to a list of candidate nodes.
fn apply_predicate<'a>(
    candidates: &[&'a DomNode],
    expr: &XPathExpr,
    root: &'a DomNode,
    regex_cache: &[(String, Regex)],
    depth: usize,
) -> Result<Vec<&'a DomNode>, XPathError> {
    if depth > MAX_XPATH_DEPTH {
        return Err(XPathError::MaxDepthExceeded { depth, max_depth: MAX_XPATH_DEPTH });
    }

    // Check if it's a numeric predicate (position check)
    if let XPathExpr::Number(n) = expr {
        let pos = *n as usize;
        if pos >= 1 && pos <= candidates.len() {
            return Ok(vec![candidates[pos - 1]]);
        }
        return Ok(Vec::new());
    }

    // For other predicates, evaluate against each candidate
    let mut result = Vec::new();
    for (i, candidate) in candidates.iter().enumerate() {
        let pred_result = eval_predicate_expr(expr, candidate, root, regex_cache, depth + 1, i + 1)?;
        if pred_result {
            result.push(*candidate);
        }
    }

    Ok(result)
}

/// Evaluate a predicate expression against a single candidate node.
fn eval_predicate_expr(
    expr: &XPathExpr,
    node: &DomNode,
    root: &DomNode,
    regex_cache: &[(String, Regex)],
    depth: usize,
    position: usize,
) -> Result<bool, XPathError> {
    if depth > MAX_XPATH_DEPTH {
        return Err(XPathError::MaxDepthExceeded { depth, max_depth: MAX_XPATH_DEPTH });
    }

    match expr {
        XPathExpr::Number(n) => {
            Ok((*n as usize) == position)
        }
        XPathExpr::FunctionCall { name, args: _args } => {
            if name == "position" {
                return Ok(true);
            }
            // Evaluate the function and check truthiness
            let result = eval_expr(expr, node, root, regex_cache, depth + 1)?;
            Ok(!result.is_empty())
        }
        XPathExpr::Or(left, right) => {
            let l = eval_predicate_expr(left, node, root, regex_cache, depth + 1, position)?;
            if l { return Ok(true); }
            eval_predicate_expr(right, node, root, regex_cache, depth + 1, position)
        }
        XPathExpr::Attribute(name) => {
            let val = get_attr_value(node, name);
            Ok(!val.is_empty())
        }
        XPathExpr::Path(pe) => {
            let result = eval_path(pe, node, root, regex_cache, depth + 1)?;
            Ok(!result.is_empty())
        }
        XPathExpr::Union(left, right) => {
            let l = eval_expr(left, node, root, regex_cache, depth + 1)?;
            if !l.is_empty() { return Ok(true); }
            let r = eval_expr(right, node, root, regex_cache, depth + 1)?;
            Ok(!r.is_empty())
        }
        XPathExpr::Literal(s) => Ok(!s.is_empty()),
        XPathExpr::TextNode => {
            let text = direct_text_content(node);
            Ok(!text.is_empty())
        }
        XPathExpr::Equals(left, right) => {
            let l_val = string_value_of_expr(left, &[node], root, regex_cache, depth + 1)?;
            let r_val = string_value_of_expr(right, &[node], root, regex_cache, depth + 1)?;
            Ok(l_val == r_val)
        }
        XPathExpr::NotEquals(left, right) => {
            let l_val = string_value_of_expr(left, &[node], root, regex_cache, depth + 1)?;
            let r_val = string_value_of_expr(right, &[node], root, regex_cache, depth + 1)?;
            Ok(l_val != r_val)
        }
        XPathExpr::Position => Ok(true),
        XPathExpr::Predicate(_) => Ok(true),
    }
}

/// Get the string value of an expression for comparison.
fn string_value_of_expr<'a>(
    expr: &XPathExpr,
    candidates: &[&'a DomNode],
    root: &'a DomNode,
    regex_cache: &[(String, Regex)],
    depth: usize,
) -> Result<String, XPathError> {
    if depth > MAX_XPATH_DEPTH {
        return Err(XPathError::MaxDepthExceeded { depth, max_depth: MAX_XPATH_DEPTH });
    }

    match expr {
        XPathExpr::Attribute(name) => {
            if let Some(node) = candidates.first() {
                Ok(get_attr_value(node, name).to_string())
            } else {
                Ok(String::new())
            }
        }
        XPathExpr::Literal(s) => Ok(s.clone()),
        XPathExpr::Number(n) => {
            if *n == (*n as u64 as f64) {
                Ok(format!("{}", *n as u64))
            } else {
                Ok(format!("{}", n))
            }
        }
        XPathExpr::FunctionCall { name, args: _args } => {
            if name == "text" || name == "text()" {
                if let Some(node) = candidates.first() {
                    Ok(direct_text_content(node))
                } else {
                    Ok(String::new())
                }
            } else {
                let result = eval_expr(expr, candidates.first().copied().unwrap_or(root), root, regex_cache, depth + 1)?;
                if let Some(node) = result.first() {
                    Ok(node_to_string(node))
                } else {
                    Ok(String::new())
                }
            }
        }
        XPathExpr::Union(left, right) => {
            // Union in function arguments: take first node's string value (matching lxml)
            // Handle @attr expressions: get attribute values directly instead of node text
            let ctx = candidates.first().copied().unwrap_or(root);
            let l_result = eval_expr(left, ctx, root, regex_cache, depth + 1)?;
            let r_result = eval_expr(right, ctx, root, regex_cache, depth + 1)?;
            let mut all = Vec::new();
            all.extend(l_result);
            for n in r_result {
                if !contains_ptr(&all, n) {
                    all.push(n);
                }
            }
            sort_by_document_order(&mut all, root);
            if let Some(node) = all.first() {
                // If the left side is an @attr expression, get the attribute value
                if let XPathExpr::Attribute(name) = left.as_ref() {
                    Ok(get_attr_value(ctx, name).to_string())
                } else {
                    Ok(node_to_string(node))
                }
            } else {
                Ok(String::new())
            }
        }
        XPathExpr::Equals(left, right) => {
            let l_val = string_value_of_expr(left, candidates, root, regex_cache, depth + 1)?;
            let r_val = string_value_of_expr(right, candidates, root, regex_cache, depth + 1)?;
            Ok(format!("{}", l_val == r_val))
        }
        XPathExpr::NotEquals(left, right) => {
            let l_val = string_value_of_expr(left, candidates, root, regex_cache, depth + 1)?;
            let r_val = string_value_of_expr(right, candidates, root, regex_cache, depth + 1)?;
            Ok(format!("{}", l_val != r_val))
        }
        _ => {
            let result = eval_expr(expr, candidates.first().copied().unwrap_or(root), root, regex_cache, depth + 1)?;
            if let Some(node) = result.first() {
                Ok(node_to_string(node))
            } else {
                Ok(String::new())
            }
        }
    }
}

/// Evaluate a function call.
fn eval_function<'a>(
    name: &str,
    args: &[XPathExpr],
    context: &'a DomNode,
    root: &'a DomNode,
    regex_cache: &[(String, Regex)],
    depth: usize,
) -> Result<Vec<&'a DomNode>, XPathError> {
    if depth > MAX_XPATH_DEPTH {
        return Err(XPathError::MaxDepthExceeded { depth, max_depth: MAX_XPATH_DEPTH });
    }

    match name {
        "re:test" => {
            if args.len() < 2 {
                return Err(XPathError::WrongArgumentCount {
                    function: "re:test",
                    expected: 2,
                    found: args.len(),
                });
            }
            // Get the string value of the first argument
            let node_set = eval_expr(&args[0], context, root, regex_cache, depth + 1)?;
            let string_val = string_value_of_expr(&args[0], &node_set, root, regex_cache, depth + 1)?;

            // Get the regex pattern from the second argument
            let pattern = match &args[1] {
                XPathExpr::Literal(s) => s.clone(),
                _ => return Err(XPathError::TypeMismatch {
                    expected: "string literal",
                    found: "expression",
                }),
            };

            // Find the pre-compiled regex
            let re = regex_cache.iter().find(|(p, _)| p == &pattern).map(|(_, r)| r);
            if let Some(re) = re {
                if re.is_match(&string_val) {
                    return Ok(vec![context]);
                }
            }
            Ok(Vec::new())
        }
        "contains" => {
            if args.len() < 2 {
                return Err(XPathError::WrongArgumentCount {
                    function: "contains",
                    expected: 2,
                    found: args.len(),
                });
            }
            let haystack = eval_expr(&args[0], context, root, regex_cache, depth + 1)?;
            let needle = eval_expr(&args[1], context, root, regex_cache, depth + 1)?;
            let haystack_str = string_value_of_expr(&args[0], &haystack, root, regex_cache, depth + 1)?;
            let needle_str = string_value_of_expr(&args[1], &needle, root, regex_cache, depth + 1)?;
            if haystack_str.contains(&needle_str) {
                return Ok(vec![context]);
            }
            Ok(Vec::new())
        }
        "starts-with" => {
            if args.len() < 2 {
                return Err(XPathError::WrongArgumentCount {
                    function: "starts-with",
                    expected: 2,
                    found: args.len(),
                });
            }
            let haystack = eval_expr(&args[0], context, root, regex_cache, depth + 1)?;
            let needle = eval_expr(&args[1], context, root, regex_cache, depth + 1)?;
            let haystack_str = string_value_of_expr(&args[0], &haystack, root, regex_cache, depth + 1)?;
            let needle_str = string_value_of_expr(&args[1], &needle, root, regex_cache, depth + 1)?;
            if haystack_str.starts_with(&needle_str) {
                return Ok(vec![context]);
            }
            Ok(Vec::new())
        }
        "translate" => {
            if args.len() < 3 {
                return Err(XPathError::WrongArgumentCount {
                    function: "translate",
                    expected: 3,
                    found: args.len(),
                });
            }
            let source = eval_expr(&args[0], context, root, regex_cache, depth + 1)?;
            let from = eval_expr(&args[1], context, root, regex_cache, depth + 1)?;
            let to = eval_expr(&args[2], context, root, regex_cache, depth + 1)?;
            let source_str = string_value_of_expr(&args[0], &source, root, regex_cache, depth + 1)?;
            let from_str = string_value_of_expr(&args[1], &from, root, regex_cache, depth + 1)?;
            let to_str = string_value_of_expr(&args[2], &to, root, regex_cache, depth + 1)?;
            let translated = translate_string(&source_str, &from_str, &to_str);
            if !translated.is_empty() {
                return Ok(vec![context]);
            }
            Ok(Vec::new())
        }
        "position" => {
            Ok(vec![context])
        }
        "text" | "text()" => {
            let mut result = Vec::new();
            collect_text_children(context, &mut result);
            Ok(result)
        }
        "not" => {
            if args.is_empty() {
                return Err(XPathError::WrongArgumentCount {
                    function: "not",
                    expected: 1,
                    found: 0,
                });
            }
            let result = eval_expr(&args[0], context, root, regex_cache, depth + 1)?;
            if result.is_empty() {
                Ok(vec![context])
            } else {
                Ok(Vec::new())
            }
        }
        _ => Err(XPathError::InvalidFunction { name: name.to_string() }),
    }
}

/// Perform character-by-character translation (matching Python/XPath translate()).
fn translate_string(source: &str, from: &str, to: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let to_chars: Vec<char> = to.chars().collect();
    for c in source.chars() {
        if let Some(pos) = from.chars().position(|fc| fc == c) {
            if pos < to_chars.len() {
                result.push(to_chars[pos]);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Get the string value of a node (for XPath string() function).
fn node_to_string(node: &DomNode) -> String {
    match node {
        DomNode::Text(t) => t.clone(),
        DomNode::Element { children, .. } => {
            let mut buf = String::new();
            for child in children {
                buf.push_str(&node_to_string(child));
            }
            buf
        }
        DomNode::Comment(c) => c.clone(),
        DomNode::Doctype(d) => d.clone(),
    }
}

/// Get the value of an attribute by name.
fn get_attr_value<'a>(node: &'a DomNode, name: &str) -> &'a str {
    match node {
        DomNode::Element { attrs, .. } => {
            for (k, v) in attrs {
                if k == name {
                    return v;
                }
            }
            ""
        }
        _ => "",
    }
}

/// Get direct text children of a node (for text() function/node-test).
fn direct_text_content(node: &DomNode) -> String {
    match node {
        DomNode::Text(t) => t.clone(),
        DomNode::Element { children, .. } => {
            let mut buf = String::new();
            for child in children {
                if let DomNode::Text(t) = child {
                    buf.push_str(t);
                }
            }
            buf
        }
        _ => String::new(),
    }
}

/// Collect direct text child nodes.
fn collect_text_children<'a>(node: &'a DomNode, result: &mut Vec<&'a DomNode>) {
    if let DomNode::Element { children, .. } = node {
        for child in children {
            if matches!(child, DomNode::Text(_)) {
                result.push(child);
            }
        }
    }
}

/// Get children of a node.
fn get_children<'a>(node: &'a DomNode) -> Vec<&'a DomNode> {
    match node {
        DomNode::Element { children, .. } => children.iter().collect(),
        _ => Vec::new(),
    }
}

/// Get all descendants (excluding self) in document order.
fn dom_descendants<'a>(node: &'a DomNode) -> Vec<&'a DomNode> {
    let mut result = Vec::new();
    collect_descendants(node, &mut result, 0);
    result
}

fn collect_descendants<'a>(node: &'a DomNode, result: &mut Vec<&'a DomNode>, depth: usize) {
    if depth > MAX_XPATH_DEPTH {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        for child in children {
            result.push(child);
            collect_descendants(child, result, depth + 1);
        }
    }
}

/// Sort nodes by document order (pre-order traversal).
fn sort_by_document_order<'a>(nodes: &mut Vec<&'a DomNode>, root: &'a DomNode) {
    // Build a position map using pointer keys
    let mut positions: HashMap<*const DomNode, usize> = HashMap::new();
    assign_positions(root, &mut positions, 0);
    nodes.sort_by_key(|n| positions.get(&(*n as *const DomNode)).copied().unwrap_or(usize::MAX));
}

fn assign_positions<'a>(node: &'a DomNode, positions: &mut HashMap<*const DomNode, usize>, next: usize) -> usize {
    let ptr: *const DomNode = node;
    positions.insert(ptr, next);
    let mut current = next + 1;
    if let DomNode::Element { children, .. } = node {
        for child in children {
            current = assign_positions(child, positions, current);
        }
    }
    current
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_elem(tag: &str, attrs: Vec<(&str, &str)>, children: Vec<DomNode>) -> DomNode {
        DomNode::Element {
            tag: tag.to_string(),
            attrs: attrs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            children,
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    fn make_text(text: &str) -> DomNode {
        DomNode::Text(text.to_string())
    }

    // Test: basic tokenization
    #[test]
    fn test_tokenize_basic() {
        let tokens = tokenize("div/p").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], XPathToken::Name("div".to_string()));
        assert_eq!(tokens[1], XPathToken::Slash);
        assert_eq!(tokens[2], XPathToken::Name("p".to_string()));
    }

    // Test: tokenize with predicates
    #[test]
    fn test_tokenize_predicate() {
        let tokens = tokenize("div[@class='foo']").unwrap();
        assert!(tokens.contains(&XPathToken::LBracket));
        assert!(tokens.contains(&XPathToken::RBracket));
    }

    // Test: empty expression
    #[test]
    fn test_empty_expression() {
        let result = parse(&[]);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), XPathError::EmptyExpression));
    }

    // Test: simple path parsing
    #[test]
    fn test_parse_simple_path() {
        let tokens = tokenize("div/p").unwrap();
        let expr = parse(&tokens).unwrap();
        match expr {
            XPathExpr::Path(pe) => {
                assert_eq!(pe.steps.len(), 2);
                assert_eq!(pe.steps[1].node_test, NodeTest::Name("p".to_string()));
            }
            _ => panic!("Expected Path expression"),
        }
    }

    // Test: attribute access
    #[test]
    fn test_parse_attribute() {
        let tokens = tokenize("@class").unwrap();
        let expr = parse(&tokens).unwrap();
        match expr {
            XPathExpr::Attribute(name) => assert_eq!(name, "class"),
            _ => panic!("Expected Attribute expression"),
        }
    }

    // Test: position predicate [1]
    #[test]
    fn test_position_predicate() {
        let tokens = tokenize("div[1]").unwrap();
        let expr = parse(&tokens).unwrap();
        match expr {
            XPathExpr::Path(pe) => {
                assert_eq!(pe.steps.len(), 1);
                assert_eq!(pe.steps[0].predicates.len(), 1);
            }
            _ => panic!("Expected Path expression"),
        }
    }

    // Test: simple eval — child axis
    #[test]
    fn test_eval_child() {
        let doc = make_elem("html", vec![], vec![
            make_elem("body", vec![], vec![
                make_elem("p", vec![("class", "text")], vec![make_text("hello")]),
                make_elem("div", vec![], vec![make_text("world")]),
            ]),
        ]);
        let compiled = XPath::compile("div").unwrap();
        let result = compiled.eval(&doc).unwrap();
        // Should find no div child of html (div is child of body)
        assert_eq!(result.len(), 0);
    }

    // Test: descendant-or-self axis via .//
    #[test]
    fn test_eval_descendant() {
        let doc = make_elem("html", vec![], vec![
            make_elem("body", vec![], vec![
                make_elem("div", vec![("class", "content")], vec![make_text("hello")]),
            ]),
        ]);
        let compiled = XPath::compile(".//div").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: self axis
    #[test]
    fn test_eval_self() {
        let doc = make_elem("html", vec![], vec![]);
        let compiled = XPath::compile("self::html").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: [1] position predicate
    #[test]
    fn test_position_predicate_eval() {
        let doc = make_elem("div", vec![], vec![
            make_elem("p", vec![], vec![make_text("first")]),
            make_elem("p", vec![], vec![make_text("second")]),
        ]);
        let compiled = XPath::compile("p[1]").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].text_content(), "first", "[1] should return the first match in document order");
    }

    // Test: union operator
    #[test]
    fn test_union() {
        let doc = make_elem("div", vec![], vec![
            make_elem("p", vec![("class", "a")], vec![make_text("p")]),
            make_elem("span", vec![], vec![make_text("span")]),
        ]);
        let compiled = XPath::compile("p|span").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text_content(), "p", "first result should be p in document order");
        assert_eq!(result[1].text_content(), "span", "second result should be span in document order");
    }

    // Test: re:test function
    #[test]
    fn test_re_test() {
        let doc = make_elem("div", vec![("class", "post-content")], vec![]);
        let compiled = XPath::compile("re:test(@class, 'post-?content')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: re:test with no match
    #[test]
    fn test_re_test_no_match() {
        let doc = make_elem("div", vec![("class", "sidebar")], vec![]);
        let compiled = XPath::compile("re:test(@class, 'post-?content')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 0);
    }

    // Test: translate function
    #[test]
    fn test_translate() {
        let doc = make_elem("div", vec![("class", "TEASER")], vec![]);
        let compiled = XPath::compile("translate(@class, 'TE', 'te')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: starts-with
    #[test]
    fn test_starts_with() {
        let doc = make_elem("div", vec![("class", "main-content")], vec![]);
        let compiled = XPath::compile("starts-with(@class, 'main')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: contains function
    #[test]
    fn test_contains() {
        let doc = make_elem("div", vec![("class", "main-content")], vec![]);
        let compiled = XPath::compile("contains(@class, 'main')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: contains no match
    #[test]
    fn test_contains_no_match() {
        let doc = make_elem("div", vec![("class", "sidebar")], vec![]);
        let compiled = XPath::compile("contains(@class, 'main')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 0);
    }

    // Test: contains empty needle
    #[test]
    fn test_contains_empty_needle() {
        let doc = make_elem("div", vec![("class", "anything")], vec![]);
        let compiled = XPath::compile("contains(@class, '')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: empty result returns empty vec (not error)
    #[test]
    fn test_empty_result() {
        let doc = make_elem("div", vec![], vec![]);
        let compiled = XPath::compile(".//nonexistent").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert!(result.is_empty());
    }

    // Test: re:test with union in args (@id|@class)
    #[test]
    fn test_union_in_args() {
        let doc = make_elem("div", vec![("id", ""), ("class", "comment")], vec![make_text("this is a comment with lots of text content")]);
        let compiled = XPath::compile("re:test(@id|@class, 'comment')").unwrap();
        let result = compiled.eval(&doc).unwrap();
        // First node in union is @id which is empty, so re:test should NOT match
        // This matches lxml behavior: first node's string value
        // The element has text content containing "comment" but @id is empty, so no match
        assert_eq!(result.len(), 0);
    }

    // Test: text() as node test
    #[test]
    fn test_text_node_test() {
        let doc = make_elem("p", vec![], vec![
            make_text("hello"),
            make_text(" "),
            make_text("world"),
        ]);
        let compiled = XPath::compile("text()").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 3);
    }

    // Test: text() as function in predicate — simplified test
    #[test]
    fn test_text_fn_predicate() {
        let doc = make_elem("div", vec![], vec![
            make_elem("p", vec![], vec![make_text("hello")]),
        ]);
        let compiled = XPath::compile("p[text()]").unwrap();
        let result = compiled.eval(&doc).unwrap();
        assert_eq!(result.len(), 1);
    }

    // Test: MAX_DEPTH enforcement
    #[test]
    fn test_max_depth() {
        let mut deep = make_elem("a", vec![], vec![]);
        for _ in 0..MAX_XPATH_DEPTH + 10 {
            deep = make_elem("a", vec![], vec![deep]);
        }
        let compiled = XPath::compile(".//a").unwrap();
        let result = compiled.eval(&deep);
        // The tree depth is bounded by collect_descendants (MAX_XPATH_DEPTH limit).
        // eval_expr depth limit is for expression nesting, not tree depth.
        // So the result should be Ok with matches limited by descendant depth.
        assert!(result.is_ok(), "expected Ok (descendant depth is bounded internally): {:?}", result);
        let matched = result.unwrap();
        // Should find some 'a' elements but not all 1010 (limited by MAX_XPATH_DEPTH in collect_descendants)
        assert!(matched.len() > 0, "should find some 'a' elements");
        assert!(matched.len() < MAX_XPATH_DEPTH + 10, "should be bounded by MAX_XPATH_DEPTH");
    }

    // Test: regex cache (compile counter)
    #[test]
    fn test_regex_cache() {
        let doc = make_elem("div", vec![("class", "post")], vec![]);
        let compiled = XPath::compile("re:test(@class, 'post')").unwrap();
        assert_eq!(compiled.regex_cache.len(), 1);
        let _ = compiled.eval(&doc).unwrap();
        let _ = compiled.eval(&doc).unwrap();
        assert_eq!(compiled.regex_cache.len(), 1);
    }

    // Test: re:test patterns from xpaths.py compile
    #[test]
    fn test_xpaths_py_patterns_compile() {
        let patterns = [
            "(?i)^shar|viral|social|syndication|newsletter|cookie|tags|\\bsidebar\\b|banner|bread-?crumb|author|button",
            "(?i)^(?:jp-|dpsp-content)|footer|Footer|share|Share|nav|Nav|related|menu|message-container|bmdh|premium",
            "(?i)^(?:nav|post-nav|ZendeskForm)| ad |footer|Footer|byline|Byline|elated|share-|sociable|embedded|embed|subnav|tag-list|\\bbar\\b|avigation|navbar|navbox|rating|(?:^| )widget(?: |$)|attachment|timestamp|user-info|user-profile|-ad-|-icon|article-infos|nfoline|outbrain|taboola|criteo|options|expand|consent|modal-content|permission|next-|-stories|most-popular|mol-factbox|message-container|yin|zlylin|xg1|slide|viewport|overlay|paid-?content|obfuscated|blurred",
            "(?i)hidden|reader-comments|akismet",
            "(?i)^hide-|^reply-|comments-title|nocomments|-reply-|(?:^| )message(?:[^- ]|$)|akismet|suggest-links|-hide-|hide-print| hidden| hide|noprint|notloaded",
            "(?i)\\blink\\b",
            "(?i)^post[-_]text|post-body|post-?entry|post[-_]?content|postContent|post_inner_wrapper|article-?text|articleText|article[-_]?content|article[-_]?maincontent|(?:entry|page|text|article|art)-content|article__content|article(?:-|__)?body|articleBody|ArticleContent|body-text|article__container",
            "(?i)^(?:content[-_]main|content(?:-|__)?body|contentBody|main-content|page-content)",
        ];
        for pattern in &patterns {
            Regex::new(pattern).unwrap_or_else(|e| panic!("Pattern '{}' failed: {}", pattern, e));
        }
    }

    // Test: translate edge cases
    #[test]
    fn test_translate_edge_cases() {
        // translate("TEST", "TE", "te") -> "test"
        assert_eq!(translate_string("TEST", "TE", "te"), "teSt");
        // empty from-string
        assert_eq!(translate_string("hello", "", "xyz"), "hello");
        // longer to-string
        assert_eq!(translate_string("abc", "abc", "xyz"), "xyz");
        // duplicate from-string chars
        assert_eq!(translate_string("aa", "a", "b"), "bb");
        // Unicode
        assert_eq!(translate_string("café", "é", "e"), "cafe");
    }
}
