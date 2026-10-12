//! Minimal, dependency-free syntax highlighting for the built-in file
//! viewer. This is not a full tokenizer: it recognises comments, string/char
//! literals, numbers and per-language keywords well enough to make source
//! code readable, without pulling in a grammar engine.

/// Highlighting category for a span of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    /// Inline/fenced code, e.g. Markdown `` `code` `` or RST ``literal``.
    Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Plain,
    C,
    Rust,
    Python,
    Js,
    Go,
    Shell,
    Toml,
    Yaml,
    Json,
    Markdown,
    Rst,
}

/// Picks a language from a file's extension (path is the repo-relative path,
/// as stored for tree entries; it need not exist on disk).
pub fn lang_for_path(path: &[u8]) -> Lang {
    let p = String::from_utf8_lossy(path);
    let name = p.rsplit('/').next().unwrap_or(&p);
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Lang::Rust,
        "py" | "pyw" | "pyi" => Lang::Python,
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => Lang::C,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" => Lang::Js,
        "go" => Lang::Go,
        "sh" | "bash" | "zsh" => Lang::Shell,
        "toml" => Lang::Toml,
        "yml" | "yaml" => Lang::Yaml,
        "json" => Lang::Json,
        "md" | "markdown" | "mkd" => Lang::Markdown,
        "rst" => Lang::Rst,
        _ => Lang::Plain,
    }
}

fn keywords(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Rust => &[
            "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false",
            "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
            "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
            "unsafe", "use", "where", "while", "async", "await",
        ],
        Lang::Python => &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
            "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import",
            "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return",
            "True", "try", "while", "with", "yield",
        ],
        Lang::Js => &[
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "debugger",
            "default",
            "delete",
            "do",
            "else",
            "export",
            "extends",
            "false",
            "finally",
            "for",
            "function",
            "if",
            "import",
            "in",
            "instanceof",
            "interface",
            "implements",
            "enum",
            "let",
            "new",
            "null",
            "return",
            "super",
            "switch",
            "this",
            "throw",
            "true",
            "try",
            "typeof",
            "var",
            "void",
            "while",
            "with",
            "yield",
            "async",
            "await",
            "of",
            "type",
            "as",
        ],
        Lang::Go => &[
            "break",
            "case",
            "chan",
            "const",
            "continue",
            "default",
            "defer",
            "else",
            "fallthrough",
            "for",
            "func",
            "go",
            "goto",
            "if",
            "import",
            "interface",
            "map",
            "package",
            "range",
            "return",
            "select",
            "struct",
            "switch",
            "type",
            "var",
            "true",
            "false",
            "nil",
        ],
        Lang::C => &[
            "auto",
            "break",
            "case",
            "char",
            "const",
            "continue",
            "default",
            "do",
            "double",
            "else",
            "enum",
            "extern",
            "float",
            "for",
            "goto",
            "if",
            "int",
            "long",
            "register",
            "return",
            "short",
            "signed",
            "sizeof",
            "static",
            "struct",
            "switch",
            "typedef",
            "union",
            "unsigned",
            "void",
            "volatile",
            "while",
            "class",
            "public",
            "private",
            "protected",
            "namespace",
            "template",
            "new",
            "delete",
            "this",
            "true",
            "false",
            "nullptr",
            "virtual",
            "override",
            "include",
        ],
        Lang::Shell => &[
            "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac",
            "function", "in", "return", "local", "export", "echo", "exit",
        ],
        Lang::Json => &["true", "false", "null"],
        Lang::Toml | Lang::Yaml | Lang::Plain | Lang::Markdown | Lang::Rst => &[],
    }
}

/// Prefix for an end-of-line comment, if any.
fn line_comment(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust | Lang::C | Lang::Go | Lang::Js => "//",
        Lang::Python | Lang::Shell | Lang::Toml | Lang::Yaml => "#",
        Lang::Json | Lang::Plain | Lang::Markdown | Lang::Rst => "",
    }
}

fn starts_at(chars: &[char], i: usize, pat: &str) -> bool {
    let mut pi = 0;
    let mut ci = i;
    for pc in pat.chars() {
        match chars.get(ci) {
            Some(c) if *c == pc => {
                ci += 1;
                pi += 1;
            }
            _ => return false,
        }
    }
    pi == pat.chars().count()
}

fn highlight_line(line: &str, kw: &[&str], comment: &str) -> Vec<(String, Token)> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut spans: Vec<(String, Token)> = Vec::new();
    let mut i = 0;
    while i < n {
        if !comment.is_empty() && starts_at(&chars, i, comment) {
            spans.push((chars[i..].iter().collect(), Token::Comment));
            break;
        }
        let c = chars[i];
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let start = i;
            i += 1;
            while i < n {
                if chars[i] == '\\' && i + 1 < n {
                    i += 2;
                    continue;
                }
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push((chars[start..i].iter().collect(), Token::Str));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            spans.push((chars[start..i].iter().collect(), Token::Number));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let tok = if kw.contains(&word.as_str()) {
                Token::Keyword
            } else {
                Token::Plain
            };
            spans.push((word, tok));
            continue;
        }
        let start = i;
        while i < n {
            let c = chars[i];
            if c == '"'
                || c == '\''
                || c == '`'
                || c.is_ascii_digit()
                || c.is_alphabetic()
                || c == '_'
            {
                break;
            }
            if !comment.is_empty() && starts_at(&chars, i, comment) {
                break;
            }
            i += 1;
        }
        spans.push((chars[start..i].iter().collect(), Token::Plain));
    }
    spans
}

fn merge_adjacent(spans: Vec<(String, Token)>) -> Vec<(String, Token)> {
    let mut out: Vec<(String, Token)> = Vec::with_capacity(spans.len());
    for (s, t) in spans {
        match out.last_mut() {
            Some(last) if last.1 == t => last.0.push_str(&s),
            _ => out.push((s, t)),
        }
    }
    out
}

fn find_delim(chars: &[char], from: usize, delim: &str) -> Option<usize> {
    (from..chars.len()).find(|&i| starts_at(chars, i, delim))
}

/// Finds the matching `](url)` for a link/image starting at `[` (or `!` for
/// images). Returns (end of "[text](", start of url, end of url).
fn md_link_span(chars: &[char], start: usize) -> Option<(usize, usize, usize)> {
    let n = chars.len();
    let open = if chars[start] == '!' {
        start + 1
    } else {
        start
    };
    if chars.get(open) != Some(&'[') {
        return None;
    }
    let mut j = open + 1;
    while j < n && chars[j] != ']' {
        j += 1;
    }
    if j >= n || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let url_start = j + 2;
    let mut k = url_start;
    while k < n && chars[k] != ')' {
        k += 1;
    }
    if k >= n {
        return None;
    }
    Some((j + 2, url_start, k))
}

/// Inline markup shared by Markdown and reStructuredText: code spans
/// (`` ` ``/`` `` ``), bold (`**`/`__`), emphasis (`*`/`_`) and
/// `[text](url)` links. Good enough for colouring, not a full parser.
fn inline_markup(chars: &[char]) -> Vec<(String, Token)> {
    let n = chars.len();
    let mut spans: Vec<(String, Token)> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '`' {
            let mut ticks = 0;
            while i + ticks < n && chars[i + ticks] == '`' {
                ticks += 1;
            }
            let delim: String = "`".repeat(ticks);
            if let Some(close) = find_delim(chars, i + ticks, &delim) {
                if !buf.is_empty() {
                    spans.push((std::mem::take(&mut buf), Token::Plain));
                }
                let end = close + ticks;
                spans.push((chars[i..end].iter().collect(), Token::Code));
                i = end;
                continue;
            }
        }
        if (c == '*' || c == '_') && chars.get(i + 1) == Some(&c) {
            let delim = format!("{c}{c}");
            if let Some(close) = find_delim(chars, i + 2, &delim) {
                if !buf.is_empty() {
                    spans.push((std::mem::take(&mut buf), Token::Plain));
                }
                let end = close + 2;
                spans.push((chars[i..end].iter().collect(), Token::Keyword));
                i = end;
                continue;
            }
        }
        if c == '*' || c == '_' {
            let delim = c.to_string();
            if let Some(close) = find_delim(chars, i + 1, &delim) {
                if !buf.is_empty() {
                    spans.push((std::mem::take(&mut buf), Token::Plain));
                }
                spans.push((chars[i..=close].iter().collect(), Token::Str));
                i = close + 1;
                continue;
            }
        }
        if (c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')))
            && let Some((paren_start, url_start, url_end)) = md_link_span(chars, i)
        {
            if !buf.is_empty() {
                spans.push((std::mem::take(&mut buf), Token::Plain));
            }
            spans.push((chars[i..paren_start].iter().collect(), Token::Plain));
            spans.push((chars[url_start..url_end].iter().collect(), Token::Str));
            spans.push((")".to_string(), Token::Plain));
            i = url_end + 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    if !buf.is_empty() {
        spans.push((buf, Token::Plain));
    }
    merge_adjacent(spans)
}

fn md_is_heading(line: &str) -> bool {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hashes) && matches!(t.as_bytes().get(hashes), Some(b' ') | None)
}

fn md_fence_marker(line: &str) -> Option<&'static str> {
    let t = line.trim_start();
    if t.starts_with("```") {
        Some("```")
    } else if t.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

fn md_is_hr(line: &str) -> bool {
    let t = line.trim();
    let Some(first) = t.chars().next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let stripped: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.len() >= 3 && stripped.chars().all(|c| c == first)
}

/// Length of a leading Markdown list marker (`- `, `* `, `+ `, `12. `, `3) `),
/// including indentation, or `None` if the line does not start with one.
fn md_list_marker_len(chars: &[char]) -> Option<usize> {
    let n = chars.len();
    let mut i = 0;
    while i < n && chars[i] == ' ' {
        i += 1;
    }
    if i >= n {
        return None;
    }
    if matches!(chars[i], '-' | '*' | '+') {
        return (chars.get(i + 1) == Some(&' ')).then_some(i + 2);
    }
    let digits_start = i;
    while i < n && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > digits_start
        && matches!(chars.get(i), Some('.') | Some(')'))
        && chars.get(i + 1) == Some(&' ')
    {
        return Some(i + 2);
    }
    None
}

fn highlight_markdown(text: &str) -> Vec<Vec<(String, Token)>> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut fence_marker = "";
    for line in text.lines() {
        if in_fence {
            out.push(vec![(line.to_string(), Token::Code)]);
            if line.trim_start().starts_with(fence_marker) {
                in_fence = false;
            }
            continue;
        }
        if let Some(marker) = md_fence_marker(line) {
            in_fence = true;
            fence_marker = marker;
            out.push(vec![(line.to_string(), Token::Code)]);
            continue;
        }
        if md_is_heading(line) {
            out.push(vec![(line.to_string(), Token::Keyword)]);
            continue;
        }
        if md_is_hr(line) {
            out.push(vec![(line.to_string(), Token::Comment)]);
            continue;
        }
        if line.trim_start().starts_with('>') {
            out.push(vec![(line.to_string(), Token::Comment)]);
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut spans = Vec::new();
        let mut i = 0;
        if let Some(len) = md_list_marker_len(&chars) {
            spans.push((chars[..len].iter().collect(), Token::Number));
            i = len;
        }
        spans.extend(inline_markup(&chars[i..]));
        out.push(merge_adjacent(spans));
    }
    out
}

/// Punctuation characters reStructuredText allows for section-title
/// over/underlines.
const RST_TITLE_PUNCT: &str = "=-~^\"'`#*+.:;,<>_";

fn is_rst_title_rule(line: &str) -> bool {
    let t = line.trim_end();
    let Some(first) = t.chars().next() else {
        return false;
    };
    t.len() >= 2 && RST_TITLE_PUNCT.contains(first) && t.chars().all(|c| c == first)
}

fn highlight_rst(text: &str) -> Vec<Vec<(String, Token)>> {
    let mut out: Vec<Vec<(String, Token)>> = Vec::new();
    for line in text.lines() {
        if is_rst_title_rule(line) {
            out.push(vec![(line.to_string(), Token::Keyword)]);
            // A title line directly above its over/underline is part of the
            // heading too; recolour it once we see the rule.
            if out.len() >= 2 {
                let idx = out.len() - 2;
                let title: String = out[idx].iter().map(|(s, _)| s.as_str()).collect();
                if !title.trim().is_empty() && !is_rst_title_rule(&title) {
                    out[idx] = vec![(title, Token::Keyword)];
                }
            }
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with(".. ") || trimmed == ".." {
            out.push(vec![(line.to_string(), Token::Comment)]);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(':')
            && let Some(close) = rest.find(':')
        {
            let indent = line.len() - trimmed.len();
            let field_len = indent + close + 2;
            let field: String = line.chars().take(field_len).collect();
            let remainder: Vec<char> = line.chars().skip(field_len).collect();
            let mut spans = vec![(field, Token::Keyword)];
            spans.extend(inline_markup(&remainder));
            out.push(merge_adjacent(spans));
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        out.push(inline_markup(&chars));
    }
    out
}

/// Highlights `text` as `lang`, one entry per source line (no wrapping).
pub fn highlight(text: &str, lang: Lang) -> Vec<Vec<(String, Token)>> {
    match lang {
        Lang::Plain => text
            .lines()
            .map(|l| vec![(l.to_string(), Token::Plain)])
            .collect(),
        Lang::Markdown => highlight_markdown(text),
        Lang::Rst => highlight_rst(text),
        _ => {
            let kw = keywords(lang);
            let comment = line_comment(lang);
            text.lines()
                .map(|l| highlight_line(l, kw, comment))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_language_by_extension() {
        assert_eq!(lang_for_path(b"src/main.rs"), Lang::Rust);
        assert_eq!(lang_for_path(b"README.md"), Lang::Markdown);
        assert_eq!(lang_for_path(b"docs/guide.rst"), Lang::Rst);
        assert_eq!(lang_for_path(b"a/b/c.py"), Lang::Python);
        assert_eq!(lang_for_path(b"LICENSE"), Lang::Plain);
    }

    #[test]
    fn highlights_keywords_strings_and_comments() {
        let out = highlight("fn main() { let s = \"hi\"; } // done", Lang::Rust);
        assert_eq!(out.len(), 1);
        let kinds: Vec<Token> = out[0].iter().map(|(_, t)| *t).collect();
        assert!(kinds.contains(&Token::Keyword));
        assert!(kinds.contains(&Token::Str));
        assert!(kinds.contains(&Token::Comment));
    }

    #[test]
    fn plain_language_keeps_one_span_per_line() {
        let out = highlight("hello\nworld", Lang::Plain);
        assert_eq!(
            out,
            vec![
                vec![("hello".to_string(), Token::Plain)],
                vec![("world".to_string(), Token::Plain)],
            ]
        );
    }

    #[test]
    fn markdown_colours_headings_lists_and_inline_markup() {
        let out = highlight(
            "# Title\n\n- an **item** with `code` and a [link](http://x)\n\n```\nraw\n```\n",
            Lang::Markdown,
        );
        assert_eq!(out[0], vec![("# Title".to_string(), Token::Keyword)]);
        let kinds: Vec<Token> = out[2].iter().map(|(_, t)| *t).collect();
        assert!(kinds.contains(&Token::Number)); // list marker
        assert!(kinds.contains(&Token::Keyword)); // **item**
        assert!(kinds.contains(&Token::Code)); // `code`
        assert!(kinds.contains(&Token::Str)); // link url
        assert_eq!(out[4], vec![("```".to_string(), Token::Code)]);
        assert_eq!(out[5], vec![("raw".to_string(), Token::Code)]);
    }

    #[test]
    fn rst_colours_titles_directives_and_inline_markup() {
        let out = highlight(
            "Title\n=====\n\n.. note::\n   some note\n\n:field: value with ``literal``\n",
            Lang::Rst,
        );
        assert_eq!(out[0], vec![("Title".to_string(), Token::Keyword)]);
        assert_eq!(out[1], vec![("=====".to_string(), Token::Keyword)]);
        assert_eq!(out[3], vec![(".. note::".to_string(), Token::Comment)]);
        let kinds: Vec<Token> = out[6].iter().map(|(_, t)| *t).collect();
        assert!(kinds.contains(&Token::Keyword)); // :field:
        assert!(kinds.contains(&Token::Code)); // ``literal``
    }
}
