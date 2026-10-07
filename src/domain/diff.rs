//! Parser for unified and combined diffs produced by Git.
//!
//! Content is kept as raw bytes: the parsed structure is the source of
//! partial patches, so it must be byte exact.

use super::path::unquote_c_style;
use super::sanitize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Raw prefix (one char, or one per parent for combined diffs).
    pub prefix: Vec<u8>,
    /// Content without prefix and without the trailing newline.
    pub text: Vec<u8>,
    /// Followed by `\ No newline at end of file`.
    pub no_eol: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub header: Vec<u8>,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
    /// Word diff segments per display line (only for word-diff output).
    pub words: Vec<Vec<(LineKind, Vec<u8>)>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub header: Vec<Vec<u8>>,
    pub old_path: Option<Vec<u8>>,
    pub new_path: Option<Vec<u8>>,
    pub new_file: bool,
    pub deleted_file: bool,
    pub old_mode: Option<u32>,
    pub new_mode: Option<u32>,
    pub binary: bool,
    pub combined: bool,
    pub rename: bool,
    pub hunks: Vec<Hunk>,
    pub truncated: bool,
}

impl FileDiff {
    pub fn path(&self) -> &[u8] {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or(b"")
    }

    pub fn display_path(&self) -> String {
        match (&self.old_path, &self.new_path) {
            (Some(o), Some(n)) if o != n => format!(
                "{} -> {}",
                sanitize::display_bytes(o),
                sanitize::display_bytes(n)
            ),
            _ => sanitize::display_bytes(self.path()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub files: Vec<FileDiff>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseMode {
    Normal,
    WordPorcelain,
}

fn parse_range(s: &[u8]) -> Option<(u32, u32)> {
    let s = std::str::from_utf8(s).ok()?;
    let s = s.trim_start_matches(['-', '+']);
    match s.split_once(',') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

/// Parses `@@ -a,b +c,d @@` (or the combined `@@@ ... @@@` form, keeping
/// the first parent range as "old").
fn parse_hunk_header(line: &[u8]) -> Option<Hunk> {
    let ats = line.iter().take_while(|c| **c == b'@').count();
    if ats < 2 {
        return None;
    }
    let rest = &line[ats..];
    let end = rest
        .windows(ats)
        .position(|w| w.iter().all(|c| *c == b'@'))?;
    let ranges: Vec<&[u8]> = rest[..end]
        .split(|c| *c == b' ')
        .filter(|s| !s.is_empty())
        .collect();
    if ranges.len() < 2 {
        return None;
    }
    let (old_start, old_count) = parse_range(ranges[0])?;
    let (new_start, new_count) = parse_range(ranges[ranges.len() - 1])?;
    Some(Hunk {
        header: line.to_vec(),
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
        words: Vec::new(),
    })
}

fn path_from_marker(rest: &[u8], prefix: &[u8]) -> Option<Vec<u8>> {
    // `--- a/path`, `+++ b/path`, `--- /dev/null`; may carry a trailing tab.
    let rest = if rest.first() == Some(&b'"') {
        unquote_c_style(rest)
    } else {
        let mut r = rest.to_vec();
        if r.last() == Some(&b'\t') {
            r.pop();
        }
        r
    };
    if rest == b"/dev/null" {
        return None;
    }
    Some(rest.strip_prefix(prefix).unwrap_or(&rest).to_vec())
}

fn split_git_header(rest: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    // `diff --git a/x b/y` – handles quoted forms and identical halves.
    if rest.first() == Some(&b'"') {
        if let Some(end) = rest[1..].iter().position(|c| *c == b'"') {
            let first = &rest[..end + 2];
            let second = rest.get(end + 3..).unwrap_or_default();
            let a = unquote_c_style(first);
            let b = unquote_c_style(second);
            return (
                a.strip_prefix(b"a/").map(|s| s.to_vec()),
                b.strip_prefix(b"b/").map(|s| s.to_vec()),
            );
        }
    }
    if rest.len() >= 5 && rest.starts_with(b"a/") && (rest.len() - 5) % 2 == 0 {
        let half = (rest.len() - 5) / 2;
        let a = &rest[2..2 + half];
        let tail = &rest[2 + half..];
        if tail.starts_with(b" b/") && &tail[3..] == a {
            return (Some(a.to_vec()), Some(a.to_vec()));
        }
    }
    if let Some(pos) = rest.windows(3).position(|w| w == b" b/") {
        let a = rest[..pos].strip_prefix(b"a/").map(|s| s.to_vec());
        let b = Some(rest[pos + 3..].to_vec());
        return (a, b);
    }
    (None, None)
}

fn octal(s: &[u8]) -> Option<u32> {
    let s = std::str::from_utf8(s).ok()?.trim();
    u32::from_str_radix(s, 8).ok()
}

/// Parses Git diff output. `truncated` marks that the input was cut.
pub fn parse_diff(data: &[u8], mode: ParseMode, truncated: bool) -> Diff {
    let mut diff = Diff {
        files: Vec::new(),
        truncated,
    };
    let mut lines: Vec<&[u8]> = data.split(|c| *c == b'\n').collect();
    if lines.last() == Some(&&b""[..]) {
        lines.pop();
    }
    let mut i = 0;
    let mut cur: Option<FileDiff> = None;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with(b"diff --git ")
            || line.starts_with(b"diff --cc ")
            || line.starts_with(b"diff --combined ")
        {
            if let Some(f) = cur.take() {
                diff.files.push(f);
            }
            let mut f = FileDiff {
                header: vec![line.to_vec()],
                ..Default::default()
            };
            if let Some(rest) = line.strip_prefix(b"diff --git ") {
                let (a, b) = split_git_header(rest);
                f.old_path = a;
                f.new_path = b;
            } else {
                f.combined = true;
                let rest = line
                    .strip_prefix(b"diff --cc ")
                    .or_else(|| line.strip_prefix(b"diff --combined "))
                    .unwrap_or_default();
                let p = unquote_c_style(rest);
                f.old_path = Some(p.clone());
                f.new_path = Some(p);
            }
            cur = Some(f);
            i += 1;
            continue;
        }
        let Some(f) = cur.as_mut() else {
            i += 1;
            continue;
        };
        if line.starts_with(b"@@") {
            let Some(mut h) = parse_hunk_header(line) else {
                f.header.push(line.to_vec());
                i += 1;
                continue;
            };
            i += 1;
            let (mut old_left, mut new_left) = (h.old_count, h.new_count);
            let nparents = if f.combined {
                line.iter().take_while(|c| **c == b'@').count() - 1
            } else {
                1
            };
            let mut word_line: Vec<(LineKind, Vec<u8>)> = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.starts_with(b"\\") {
                    if mode == ParseMode::WordPorcelain {
                        i += 1;
                        continue;
                    }
                    if let Some(last) = h.lines.last_mut() {
                        last.no_eol = true;
                    }
                    i += 1;
                    continue;
                }
                let finished = if f.combined || mode == ParseMode::WordPorcelain {
                    l.starts_with(b"diff ") || l.starts_with(b"@@")
                } else {
                    old_left == 0 && new_left == 0
                };
                if finished {
                    break;
                }
                if mode == ParseMode::WordPorcelain {
                    match l.first() {
                        Some(b'~') => {
                            h.words.push(std::mem::take(&mut word_line));
                        }
                        Some(b'+') => word_line.push((LineKind::Add, l[1..].to_vec())),
                        Some(b'-') => word_line.push((LineKind::Del, l[1..].to_vec())),
                        Some(_) => word_line.push((LineKind::Context, l[1..].to_vec())),
                        None => word_line.push((LineKind::Context, Vec::new())),
                    }
                    i += 1;
                    continue;
                }
                let (prefix, text) = if l.is_empty() {
                    (vec![b' '; nparents], Vec::new())
                } else if l.len() >= nparents {
                    (l[..nparents].to_vec(), l[nparents..].to_vec())
                } else {
                    (l.to_vec(), Vec::new())
                };
                let kind = if prefix.contains(&b'+') {
                    LineKind::Add
                } else if prefix.contains(&b'-') {
                    LineKind::Del
                } else if prefix.iter().all(|c| *c == b' ') {
                    LineKind::Context
                } else {
                    break;
                };
                match kind {
                    LineKind::Context => {
                        old_left = old_left.saturating_sub(1);
                        new_left = new_left.saturating_sub(1);
                    }
                    LineKind::Add => new_left = new_left.saturating_sub(1),
                    LineKind::Del => old_left = old_left.saturating_sub(1),
                }
                h.lines.push(DiffLine {
                    kind,
                    prefix,
                    text,
                    no_eol: false,
                });
                i += 1;
            }
            if !word_line.is_empty() {
                h.words.push(word_line);
            }
            if !f.combined && mode == ParseMode::Normal && (old_left > 0 || new_left > 0) {
                f.truncated = true;
            }
            f.hunks.push(h);
            continue;
        }
        // Extended header lines.
        if let Some(r) = line.strip_prefix(b"--- ") {
            f.old_path = path_from_marker(r, b"a/");
            if f.old_path.is_none() {
                f.new_file = true;
            }
        } else if let Some(r) = line.strip_prefix(b"+++ ") {
            f.new_path = path_from_marker(r, b"b/");
            if f.new_path.is_none() {
                f.deleted_file = true;
            }
        } else if let Some(r) = line.strip_prefix(b"new file mode ") {
            f.new_file = true;
            f.new_mode = octal(r);
            f.old_path = None;
        } else if let Some(r) = line.strip_prefix(b"deleted file mode ") {
            f.deleted_file = true;
            f.old_mode = octal(r);
            f.new_path = None;
        } else if let Some(r) = line.strip_prefix(b"old mode ") {
            f.old_mode = octal(r);
        } else if let Some(r) = line.strip_prefix(b"new mode ") {
            f.new_mode = octal(r);
        } else if let Some(r) = line.strip_prefix(b"rename from ") {
            f.rename = true;
            f.old_path = Some(unquote_c_style(r));
        } else if let Some(r) = line.strip_prefix(b"rename to ") {
            f.rename = true;
            f.new_path = Some(unquote_c_style(r));
        } else if let Some(r) = line.strip_prefix(b"copy from ") {
            f.old_path = Some(unquote_c_style(r));
        } else if let Some(r) = line.strip_prefix(b"copy to ") {
            f.new_path = Some(unquote_c_style(r));
        } else if line.starts_with(b"index ") && f.combined {
            // `index a,b..c` in combined diffs; nothing to extract.
        } else if let Some(r) = line.strip_prefix(b"index ") {
            if let Some(sp) = r.iter().position(|c| *c == b' ') {
                f.old_mode = f.old_mode.or(octal(&r[sp + 1..]));
                f.new_mode = f.new_mode.or(octal(&r[sp + 1..]));
            }
        } else if line.starts_with(b"Binary files ") || line.starts_with(b"GIT binary patch") {
            f.binary = true;
        }
        f.header.push(line.to_vec());
        i += 1;
    }
    if let Some(mut f) = cur.take() {
        if truncated {
            f.truncated = true;
        }
        diff.files.push(f);
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &[u8] = b"diff --git a/f.txt b/f.txt
index 1111111..2222222 100644
--- a/f.txt
+++ b/f.txt
@@ -1,3 +1,3 @@
 a
-b
+B
 c
@@ -10,2 +10,2 @@ fn x()
-y
\\ No newline at end of file
+y
+z
\\ No newline at end of file
";

    #[test]
    fn parses_hunks_and_eol_markers() {
        // second hunk counts: -10,2 vs 1 line deleted; adjust header to match body
        let data = String::from_utf8_lossy(SIMPLE).replace("@@ -10,2 +10,2 @@", "@@ -10,1 +10,2 @@");
        let d = parse_diff(data.as_bytes(), ParseMode::Normal, false);
        assert_eq!(d.files.len(), 1);
        let f = &d.files[0];
        assert_eq!(f.path(), b"f.txt");
        assert_eq!(f.old_mode, Some(0o100644));
        assert_eq!(f.hunks.len(), 2);
        assert_eq!(f.hunks[0].lines.len(), 4);
        assert_eq!(f.hunks[1].old_start, 10);
        assert_eq!(f.hunks[1].header, b"@@ -10,1 +10,2 @@ fn x()");
        let l = &f.hunks[1].lines;
        assert!(l[0].no_eol && l[0].kind == LineKind::Del);
        assert!(!l[1].no_eol);
        assert!(l[2].no_eol);
        assert!(!f.truncated);
    }

    #[test]
    fn new_deleted_binary_and_quoted() {
        let data = b"diff --git a/n b/n
new file mode 100755
index 0000000..e69de29
--- /dev/null
+++ b/n
@@ -0,0 +1 @@
+x
diff --git a/d b/d
deleted file mode 100644
--- a/d
+++ /dev/null
@@ -1 +0,0 @@
-x
diff --git a/bin b/bin
Binary files a/bin and b/bin differ
diff --git \"a/t\\tq\" \"b/t\\tq\"
--- \"a/t\\tq\"
+++ \"b/t\\tq\"
@@ -1 +1 @@
-1
+2
";
        let d = parse_diff(data, ParseMode::Normal, false);
        assert_eq!(d.files.len(), 4);
        assert!(d.files[0].new_file && d.files[0].old_path.is_none());
        assert_eq!(d.files[0].new_mode, Some(0o100755));
        assert!(d.files[1].deleted_file && d.files[1].new_path.is_none());
        assert!(d.files[2].binary && d.files[2].hunks.is_empty());
        assert_eq!(d.files[3].path(), b"t\tq");
    }

    #[test]
    fn line_starting_with_dashes_is_content() {
        let data = b"diff --git a/x b/x
--- a/x
+++ b/x
@@ -1,2 +1,2 @@
--- a/fake
+++ b/fake
";
        let d = parse_diff(data, ParseMode::Normal, false);
        let h = &d.files[0].hunks[0];
        assert_eq!(h.lines.len(), 2);
        assert_eq!(h.lines[0].text, b"-- a/fake");
        assert_eq!(h.lines[1].text, b"++ b/fake");
    }

    #[test]
    fn combined_diff() {
        let data = b"diff --cc f
index 1,2..3
--- a/f
+++ b/f
@@@ -1,1 -1,1 +1,2 @@@
- a
 +b
++c
";
        let d = parse_diff(data, ParseMode::Normal, false);
        let f = &d.files[0];
        assert!(f.combined);
        assert_eq!(f.hunks[0].lines.len(), 3);
        assert_eq!(f.hunks[0].lines[0].kind, LineKind::Del);
        assert_eq!(f.hunks[0].lines[1].kind, LineKind::Add);
        assert_eq!(f.hunks[0].lines[2].prefix, b"++");
    }

    #[test]
    fn truncated_hunk_is_flagged() {
        let data = b"diff --git a/x b/x
--- a/x
+++ b/x
@@ -1,5 +1,5 @@
 a
";
        let d = parse_diff(data, ParseMode::Normal, true);
        assert!(d.files[0].truncated);
    }

    #[test]
    fn word_porcelain() {
        let data = b"diff --git a/x b/x
--- a/x
+++ b/x
@@ -1 +1 @@
 hello 
-world
+there
~
";
        let d = parse_diff(data, ParseMode::WordPorcelain, false);
        let h = &d.files[0].hunks[0];
        assert_eq!(h.words.len(), 1);
        assert_eq!(h.words[0].len(), 3);
        assert_eq!(h.words[0][1], (LineKind::Del, b"world".to_vec()));
    }
}
