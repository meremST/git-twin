use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;

use super::sanitize;

/// Path relative to the repository root, kept as raw bytes so that
/// non UTF-8 names survive round-trips to Git untouched.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct RepoPath(Vec<u8>);

impl RepoPath {
    pub fn from_bytes(b: impl Into<Vec<u8>>) -> RepoPath {
        RepoPath(b.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(&self.0)
    }

    pub fn to_os_string(&self) -> OsString {
        self.as_os_str().to_os_string()
    }

    /// Escaped representation for display only; never use it as identity.
    pub fn display(&self) -> String {
        sanitize::display_bytes(&self.0)
    }

    pub fn file_name(&self) -> &[u8] {
        match self.0.iter().rposition(|b| *b == b'/') {
            Some(p) => &self.0[p + 1..],
            None => &self.0,
        }
    }
}

/// Decodes a C-style quoted path as produced by Git (`"a\tb\303\251"`).
/// Unquoted input is returned as-is.
pub fn unquote_c_style(s: &[u8]) -> Vec<u8> {
    if s.len() < 2 || s[0] != b'"' || s[s.len() - 1] != b'"' {
        return s.to_vec();
    }
    let inner = &s[1..s.len() - 1];
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c != b'\\' || i + 1 >= inner.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let n = inner[i + 1];
        i += 2;
        match n {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b't' => out.push(b'\t'),
            b'n' => out.push(b'\n'),
            b'v' => out.push(11),
            b'f' => out.push(12),
            b'r' => out.push(b'\r'),
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b'0'..=b'7' => {
                let mut v: u32 = (n - b'0') as u32;
                let mut k = 0;
                while k < 2 && i < inner.len() && (b'0'..=b'7').contains(&inner[i]) {
                    v = v * 8 + (inner[i] - b'0') as u32;
                    i += 1;
                    k += 1;
                }
                out.push(v as u8);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_handles_octal_and_escapes() {
        assert_eq!(unquote_c_style(b"plain"), b"plain");
        assert_eq!(unquote_c_style(b"\"a\\tb\""), b"a\tb");
        assert_eq!(unquote_c_style(b"\"\\303\\251\""), "é".as_bytes());
        assert_eq!(unquote_c_style(b"\"\\377x\""), b"\xffx");
        assert_eq!(unquote_c_style(b"\"q\\\"\\\\\""), b"q\"\\");
    }

    #[test]
    fn display_is_escaped_identity_is_raw() {
        let p = RepoPath::from_bytes(b"dir/\xffname\x1b".to_vec());
        assert_eq!(p.as_bytes(), b"dir/\xffname\x1b");
        assert_eq!(p.display(), "dir/\\xFFname^[");
        assert_eq!(p.file_name(), b"\xffname\x1b");
    }
}
