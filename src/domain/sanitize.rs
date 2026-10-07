//! Neutralisation of repository-controlled content before terminal rendering.
//!
//! Anything coming from the repository (paths, refs, messages, diffs, hook
//! output) must go through these helpers so that no escape sequence reaches
//! the terminal.

/// Converts raw bytes (possibly not UTF-8) into a printable string.
/// Invalid UTF-8 bytes become `\xNN`; control characters are made visible.
pub fn display_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                push_sanitized(&mut out, s, None);
                break;
            }
            Err(e) => {
                let (valid, after) = rest.split_at(e.valid_up_to());
                push_sanitized(&mut out, std::str::from_utf8(valid).unwrap_or(""), None);
                let bad = e.error_len().unwrap_or(after.len()).max(1).min(after.len());
                for b in &after[..bad] {
                    out.push_str(&format!("\\x{b:02X}"));
                }
                rest = &after[bad..];
                if rest.is_empty() {
                    break;
                }
            }
        }
    }
    out
}

/// Sanitizes a single display line: control characters are escaped, tabs
/// are replaced by a single space (use [`expand_tabs`] when alignment matters).
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    push_sanitized(&mut out, s, None);
    out
}

/// Sanitizes and expands tabs to the given width (diff/blob display).
pub fn expand_tabs(s: &str, tab: usize) -> String {
    let mut out = String::with_capacity(s.len());
    push_sanitized(&mut out, s, Some(tab.max(1)));
    out
}

/// Sanitizes bytes with tab expansion (diff content).
pub fn display_bytes_tabs(bytes: &[u8], tab: usize) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => expand_tabs(s, tab),
        Err(_) => {
            let lossy = display_bytes(bytes);
            expand_tabs(&lossy, tab)
        }
    }
}

fn is_dangerous(c: char) -> bool {
    c.is_control()
        || ('\u{202A}'..='\u{202E}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || c == '\u{200E}'
        || c == '\u{200F}'
        || c == '\u{FEFF}'
}

fn push_sanitized(out: &mut String, s: &str, tab: Option<usize>) {
    let mut col = 0usize;
    for c in s.chars() {
        if c == '\t' {
            match tab {
                Some(t) => {
                    let n = t - (col % t);
                    for _ in 0..n {
                        out.push(' ');
                    }
                    col += n;
                }
                None => {
                    out.push(' ');
                    col += 1;
                }
            }
            continue;
        }
        if c == '\n' || c == '\r' {
            // Line terminators are never rendered literally.
            if c == '\r' {
                out.push_str("^M");
                col += 2;
            }
            continue;
        }
        if is_dangerous(c) {
            let esc = if (c as u32) < 0x20 {
                format!("^{}", ((c as u8) + b'@') as char)
            } else if c == '\u{7f}' {
                "^?".to_string()
            } else {
                format!("\\u{{{:04X}}}", c as u32)
            };
            col += esc.len();
            out.push_str(&esc);
            continue;
        }
        out.push(c);
        col += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
    }
}

/// Removes credentials embedded in URLs (`scheme://user:pass@host`).
pub fn redact_urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find("://") {
        let (head, tail) = rest.split_at(pos + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '/' || c == '\'' || c == '"')
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        if let Some(at) = authority.rfind('@') {
            out.push_str("***@");
            out.push_str(&authority[at + 1..]);
        } else {
            out.push_str(authority);
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_terminal_sequences() {
        let s = sanitize("evil\x1b]0;title\x07\x1b[2Jx");
        assert!(!s.contains('\x1b'));
        assert!(!s.contains('\x07'));
        assert_eq!(s, "evil^[]0;title^G^[[2Jx");
    }

    #[test]
    fn escapes_c1_and_bidi() {
        let s = sanitize("a\u{9b}31mb\u{202e}c");
        assert!(!s.contains('\u{9b}'));
        assert!(!s.contains('\u{202e}'));
        assert!(s.contains("\\u{009B}"));
    }

    #[test]
    fn invalid_utf8_is_escaped() {
        assert_eq!(display_bytes(b"a\xffb"), "a\\xFFb");
        assert_eq!(display_bytes(b"\xc3"), "\\xC3");
        assert_eq!(display_bytes("é".as_bytes()), "é");
    }

    #[test]
    fn tabs_are_expanded_by_column() {
        assert_eq!(expand_tabs("a\tb", 4), "a   b");
        assert_eq!(expand_tabs("\t", 8), "        ");
        assert_eq!(expand_tabs("ab\r", 8), "ab^M");
    }

    #[test]
    fn redacts_credentials() {
        assert_eq!(
            redact_urls("fatal: https://user:tok@host/x.git failed"),
            "fatal: https://***@host/x.git failed"
        );
        assert_eq!(redact_urls("ssh://host/x"), "ssh://host/x");
        assert_eq!(redact_urls("no url"), "no url");
    }
}
