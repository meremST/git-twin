//! Construction of partial patches (hunk or line selections).
//!
//! Two directions are supported:
//! * [`Direction::Forward`]: the patch is applied as-is. The *old* side of the
//!   source diff is the base that must stay intact (staging from an
//!   index→worktree diff with `git apply --cached`).
//! * [`Direction::Reverse`]: the patch is applied with `--reverse`. The *new*
//!   side of the source diff is the base (unstaging from a HEAD→index diff,
//!   or reverting lines from an index→worktree diff).
//!
//! Unselected changes that exist in the base become context, the others are
//! dropped. A base line lacking its final newline that ends up followed by
//! other lines is rewritten so the result stays well-formed.

use std::collections::BTreeSet;

use super::diff::{FileDiff, LineKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PatchError {
    Binary,
    Combined,
    Truncated,
    EmptySelection,
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PatchError::Binary => "binary file: only whole-file operations are supported",
            PatchError::Combined => {
                "conflict (combined) diff: resolve the file or use whole-file actions"
            }
            PatchError::Truncated => {
                "diff was truncated: partial operations are disabled for this file"
            }
            PatchError::EmptySelection => "no changed line selected",
        })
    }
}

#[derive(Clone, Debug)]
struct OutLine<'a> {
    marker: u8,
    text: &'a [u8],
    eol: bool,
}

fn needs_quote(p: &[u8]) -> bool {
    p.iter()
        .any(|&c| c < 0x20 || c == b'"' || c == b'\\' || c == 0x7f || c >= 0x80)
}

/// Formats `prefix + path` the way Git does (C-style quoting when needed).
pub fn quote_path(prefix: &str, p: &[u8]) -> Vec<u8> {
    let mut full = prefix.as_bytes().to_vec();
    full.extend_from_slice(p);
    if !needs_quote(&full) {
        return full;
    }
    let mut out = vec![b'"'];
    for &c in &full {
        match c {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            c if c < 0x20 || c >= 0x7f => out.extend_from_slice(format!("\\{c:03o}").as_bytes()),
            c => out.push(c),
        }
    }
    out.push(b'"');
    out
}

/// Every changed line of a hunk, as `(hunk, line)` pairs.
pub fn hunk_selection(file: &FileDiff, hunk: usize) -> BTreeSet<(usize, usize)> {
    file.hunks
        .get(hunk)
        .map(|h| {
            h.lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.kind != LineKind::Context)
                .map(|(i, _)| (hunk, i))
                .collect()
        })
        .unwrap_or_default()
}

/// Builds a patch containing only the selected changed lines.
/// `selected` holds `(hunk index, line index)` pairs; context lines in the
/// selection are ignored.
pub fn build_patch(
    file: &FileDiff,
    selected: &BTreeSet<(usize, usize)>,
    dir: Direction,
) -> Result<Vec<u8>, PatchError> {
    if file.binary {
        return Err(PatchError::Binary);
    }
    if file.combined {
        return Err(PatchError::Combined);
    }
    if file.truncated {
        return Err(PatchError::Truncated);
    }
    let mut hunks_out: Vec<(Vec<OutLine>, usize)> = Vec::new();
    for (hi, h) in file.hunks.iter().enumerate() {
        let any = h
            .lines
            .iter()
            .enumerate()
            .any(|(li, l)| l.kind != LineKind::Context && selected.contains(&(hi, li)));
        if !any {
            continue;
        }
        let mut out: Vec<OutLine> = Vec::with_capacity(h.lines.len());
        for (li, l) in h.lines.iter().enumerate() {
            let sel = selected.contains(&(hi, li));
            let eol = !l.no_eol;
            let marker = match (l.kind, dir, sel) {
                (LineKind::Context, _, _) => Some(b' '),
                (LineKind::Del, _, true) => Some(b'-'),
                (LineKind::Add, _, true) => Some(b'+'),
                (LineKind::Del, Direction::Forward, false) => Some(b' '),
                (LineKind::Add, Direction::Forward, false) => None,
                (LineKind::Add, Direction::Reverse, false) => Some(b' '),
                (LineKind::Del, Direction::Reverse, false) => None,
            };
            if let Some(marker) = marker {
                out.push(OutLine {
                    marker,
                    text: &l.text,
                    eol,
                });
            }
        }
        fix_eol(&mut out, dir);
        hunks_out.push((out, hi));
    }
    if hunks_out.is_empty() {
        return Err(PatchError::EmptySelection);
    }

    let count = |out: &[OutLine], side_old: bool| -> u32 {
        out.iter()
            .filter(|l| l.marker == b' ' || l.marker == if side_old { b'-' } else { b'+' })
            .count() as u32
    };
    let old_empty = hunks_out.iter().all(|(o, _)| count(o, true) == 0);
    let new_empty = hunks_out.iter().all(|(o, _)| count(o, false) == 0);

    let old_name = file.old_path.as_deref().or(file.new_path.as_deref()).unwrap_or(b"");
    let new_name = file.new_path.as_deref().or(file.old_path.as_deref()).unwrap_or(b"");
    let mut p = Vec::new();
    p.extend_from_slice(b"diff --git ");
    p.extend_from_slice(&quote_path("a/", old_name));
    p.push(b' ');
    p.extend_from_slice(&quote_path("b/", new_name));
    p.push(b'\n');
    let creates = file.new_file && old_empty;
    let deletes = file.deleted_file && new_empty;
    if creates {
        let m = file.new_mode.unwrap_or(0o100644);
        p.extend_from_slice(format!("new file mode {m:o}\n").as_bytes());
        p.extend_from_slice(b"--- /dev/null\n");
    } else {
        if deletes {
            let m = file.old_mode.unwrap_or(0o100644);
            p.extend_from_slice(format!("deleted file mode {m:o}\n").as_bytes());
        }
        p.extend_from_slice(b"--- ");
        p.extend_from_slice(&quote_path("a/", old_name));
        p.push(b'\n');
    }
    if deletes {
        p.extend_from_slice(b"+++ /dev/null\n");
    } else {
        p.extend_from_slice(b"+++ ");
        p.extend_from_slice(&quote_path("b/", new_name));
        p.push(b'\n');
    }

    let mut delta: i64 = 0;
    for (out, hi) in &hunks_out {
        let h = &file.hunks[*hi];
        let oc = count(out, true);
        let nc = count(out, false);
        let (os, ns) = match dir {
            Direction::Forward => {
                let pos = first_line(h.old_start, h.old_count) + delta;
                (h.old_start as i64, start_from_pos(pos, nc))
            }
            Direction::Reverse => {
                let pos = first_line(h.new_start, h.new_count) - delta;
                (start_from_pos(pos, oc), h.new_start as i64)
            }
        };
        delta += nc as i64 - oc as i64;
        p.extend_from_slice(format!("@@ -{os},{oc} +{ns},{nc} @@\n").as_bytes());
        for l in out {
            p.push(l.marker);
            p.extend_from_slice(l.text);
            p.push(b'\n');
            if !l.eol {
                p.extend_from_slice(b"\\ No newline at end of file\n");
            }
        }
    }
    Ok(p)
}

fn first_line(start: u32, count: u32) -> i64 {
    if count == 0 {
        start as i64 + 1
    } else {
        start as i64
    }
}

fn start_from_pos(pos: i64, count: u32) -> i64 {
    if count == 0 { (pos - 1).max(0) } else { pos.max(1) }
}

/// Ensures that, on the side produced by the patch, a line without final
/// newline is never followed by another line.
fn fix_eol(out: &mut Vec<OutLine>, dir: Direction) {
    // The produced side is "new" for Forward and "old" for Reverse.
    let produced = |m: u8| match dir {
        Direction::Forward => m == b' ' || m == b'+',
        Direction::Reverse => m == b' ' || m == b'-',
    };
    let mut i = 0;
    while i < out.len() {
        let l = &out[i];
        if produced(l.marker) && !l.eol && out[i + 1..].iter().any(|n| produced(n.marker)) {
            let text = l.text;
            match (l.marker, dir) {
                (b' ', Direction::Forward) => {
                    out[i] = OutLine {
                        marker: b'-',
                        text,
                        eol: false,
                    };
                    out.insert(
                        i + 1,
                        OutLine {
                            marker: b'+',
                            text,
                            eol: true,
                        },
                    );
                    i += 1;
                }
                (b' ', Direction::Reverse) => {
                    out[i] = OutLine {
                        marker: b'-',
                        text,
                        eol: true,
                    };
                    out.insert(
                        i + 1,
                        OutLine {
                            marker: b'+',
                            text,
                            eol: false,
                        },
                    );
                    i += 1;
                }
                _ => out[i].eol = true,
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diff::{ParseMode, parse_diff};

    fn file(d: &[u8]) -> FileDiff {
        parse_diff(d, ParseMode::Normal, false).files.remove(0)
    }

    fn sel(v: &[(usize, usize)]) -> BTreeSet<(usize, usize)> {
        v.iter().copied().collect()
    }

    const MOD: &[u8] = b"diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,4 +1,4 @@
 a
-b
-c
+B
+C
 d
";

    #[test]
    fn forward_single_addition() {
        let f = file(MOD);
        let p = build_patch(&f, &sel(&[(0, 3)]), Direction::Forward).unwrap();
        let s = String::from_utf8(p).unwrap();
        assert!(s.contains("@@ -1,4 +1,5 @@\n a\n b\n c\n+B\n d\n"), "{s}");
    }

    #[test]
    fn forward_single_deletion() {
        let f = file(MOD);
        let p = build_patch(&f, &sel(&[(0, 1)]), Direction::Forward).unwrap();
        let s = String::from_utf8(p).unwrap();
        assert!(s.contains("@@ -1,4 +1,3 @@\n a\n-b\n c\n d\n"), "{s}");
    }

    #[test]
    fn reverse_single_addition() {
        let f = file(MOD);
        let p = build_patch(&f, &sel(&[(0, 3)]), Direction::Reverse).unwrap();
        let s = String::from_utf8(p).unwrap();
        // Base is the new side (a B C d); only "+B" is reverted.
        assert!(s.contains("@@ -1,3 +1,4 @@\n a\n+B\n C\n d\n"), "{s}");
    }

    #[test]
    fn context_selection_is_ignored() {
        let f = file(MOD);
        assert_eq!(
            build_patch(&f, &sel(&[(0, 0)]), Direction::Forward),
            Err(PatchError::EmptySelection)
        );
    }

    #[test]
    fn no_eol_context_followed_by_addition() {
        let d = b"diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,2 +1,3 @@
 x
-a
\\ No newline at end of file
+a
+b
";
        let f = file(d);
        // Stage only "+b": "-a" unselected becomes context without EOL and
        // must be rewritten since a line follows it.
        let p = build_patch(&f, &sel(&[(0, 3)]), Direction::Forward).unwrap();
        let s = String::from_utf8(p).unwrap();
        assert!(
            s.contains("@@ -1,2 +1,3 @@\n x\n-a\n\\ No newline at end of file\n+a\n+b\n"),
            "{s}"
        );
    }

    #[test]
    fn new_file_partial_forward_keeps_creation() {
        let d = b"diff --git a/n b/n
new file mode 100755
--- /dev/null
+++ b/n
@@ -0,0 +1,2 @@
+1
+2
";
        let f = file(d);
        let p = build_patch(&f, &sel(&[(0, 1)]), Direction::Forward).unwrap();
        let s = String::from_utf8(p).unwrap();
        assert!(s.contains("new file mode 100755\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,1 @@\n+2\n"), "{s}");
    }

    #[test]
    fn new_file_partial_reverse_becomes_modification() {
        let d = b"diff --git a/n b/n
new file mode 100644
--- /dev/null
+++ b/n
@@ -0,0 +1,2 @@
+1
+2
";
        let f = file(d);
        let p = build_patch(&f, &sel(&[(0, 0)]), Direction::Reverse).unwrap();
        let s = String::from_utf8(p).unwrap();
        assert!(s.contains("--- a/n\n+++ b/n\n@@ -1,1 +1,2 @@\n+1\n 2\n"), "{s}");
        let all = build_patch(&f, &sel(&[(0, 0), (0, 1)]), Direction::Reverse).unwrap();
        assert!(String::from_utf8(all).unwrap().contains("new file mode"));
    }

    #[test]
    fn deleted_file_partial_forward_becomes_modification() {
        let d = b"diff --git a/d b/d
deleted file mode 100644
--- a/d
+++ /dev/null
@@ -1,2 +0,0 @@
-1
-2
";
        let f = file(d);
        let p = build_patch(&f, &sel(&[(0, 0)]), Direction::Forward).unwrap();
        let s = String::from_utf8(p).unwrap();
        assert!(!s.contains("deleted file mode"));
        assert!(s.contains("+++ b/d\n@@ -1,2 +1,1 @@\n-1\n 2\n"), "{s}");
        let all = build_patch(&f, &sel(&[(0, 0), (0, 1)]), Direction::Forward).unwrap();
        let s = String::from_utf8(all).unwrap();
        assert!(s.contains("deleted file mode 100644"));
        assert!(s.contains("@@ -1,2 +0,0 @@"), "{s}");
    }

    #[test]
    fn multiple_hunks_offsets() {
        let d = b"diff --git a/f b/f
--- a/f
+++ b/f
@@ -1,2 +1,3 @@
 a
+x
 b
@@ -10,2 +11,3 @@
 c
+y
 d
";
        let f = file(d);
        // Forward, skipping first hunk: second hunk new start = old start.
        let p = build_patch(&f, &sel(&[(1, 1)]), Direction::Forward).unwrap();
        assert!(String::from_utf8_lossy(&p).contains("@@ -10,2 +10,3 @@"));
        let p = build_patch(&f, &sel(&[(0, 1), (1, 1)]), Direction::Forward).unwrap();
        assert!(String::from_utf8_lossy(&p).contains("@@ -10,2 +11,3 @@"));
        // Reverse, skipping first hunk: new side keeps its numbering.
        let p = build_patch(&f, &sel(&[(1, 1)]), Direction::Reverse).unwrap();
        assert!(String::from_utf8_lossy(&p).contains("@@ -11,2 +11,3 @@"));
    }

    #[test]
    fn rejects_binary_combined_truncated() {
        let mut f = file(MOD);
        f.binary = true;
        assert_eq!(build_patch(&f, &sel(&[(0, 1)]), Direction::Forward), Err(PatchError::Binary));
        f.binary = false;
        f.truncated = true;
        assert_eq!(
            build_patch(&f, &sel(&[(0, 1)]), Direction::Forward),
            Err(PatchError::Truncated)
        );
        f.truncated = false;
        f.combined = true;
        assert_eq!(
            build_patch(&f, &sel(&[(0, 1)]), Direction::Forward),
            Err(PatchError::Combined)
        );
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_path("a/", b"x y"), b"a/x y");
        assert_eq!(quote_path("a/", b"t\tq"), b"\"a/t\\tq\"");
        assert_eq!(quote_path("b/", b"\xff"), b"\"b/\\377\"");
    }
}
