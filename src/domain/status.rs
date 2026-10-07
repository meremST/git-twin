//! Parser for `git status --porcelain=v2 -z --branch`.

use super::oid::Oid;
use super::path::RepoPath;
use super::sanitize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Changed,
    Renamed,
    Unmerged,
    Untracked,
    Ignored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: RepoPath,
    pub orig_path: Option<RepoPath>,
    pub kind: EntryKind,
    /// Index status (`.` when unchanged).
    pub x: u8,
    /// Worktree status (`.` when unchanged).
    pub y: u8,
    pub mode_head: u32,
    pub mode_index: u32,
    pub mode_wt: u32,
    pub submodule: bool,
}

impl StatusEntry {
    pub fn has_unstaged(&self) -> bool {
        match self.kind {
            EntryKind::Unmerged | EntryKind::Untracked => true,
            EntryKind::Changed | EntryKind::Renamed => self.y != b'.',
            EntryKind::Ignored => false,
        }
    }

    pub fn has_staged(&self) -> bool {
        matches!(self.kind, EntryKind::Changed | EntryKind::Renamed) && self.x != b'.'
    }

    /// Human readable state, close to git gui's wording.
    pub fn describe(&self, staged_side: bool) -> &'static str {
        match self.kind {
            EntryKind::Untracked => "Untracked, not staged",
            EntryKind::Ignored => "Ignored",
            EntryKind::Unmerged => "Requires merge resolution",
            _ => {
                let c = if staged_side { self.x } else { self.y };
                match (staged_side, c) {
                    (true, b'A') => "Staged for commit (new file)",
                    (true, b'D') => "Staged for removal",
                    (true, b'R') => "Staged for commit (renamed)",
                    (true, b'T') => "Staged for commit (type change)",
                    (true, _) => "Staged for commit",
                    (false, b'D') => "Missing (deleted in working tree)",
                    (false, b'T') => "File type changed, not staged",
                    (false, _) if self.x != b'.' => "Portions staged for commit",
                    (false, _) => "Modified, not staged",
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchStatus {
    pub oid: Option<Oid>,
    /// None when HEAD is detached.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoStatus {
    pub branch: BranchStatus,
    pub entries: Vec<StatusEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

fn mode(s: &[u8]) -> u32 {
    std::str::from_utf8(s)
        .ok()
        .and_then(|s| u32::from_str_radix(s, 8).ok())
        .unwrap_or(0)
}

fn fields(rec: &[u8], n: usize) -> Result<Vec<&[u8]>, ParseError> {
    let v: Vec<&[u8]> = rec.splitn(n, |b| *b == b' ').collect();
    if v.len() != n {
        return Err(ParseError(format!(
            "malformed status record: {}",
            sanitize::display_bytes(rec)
        )));
    }
    Ok(v)
}

pub fn parse_porcelain_v2(out: &[u8]) -> Result<RepoStatus, ParseError> {
    let mut st = RepoStatus::default();
    let mut tokens = out.split(|b| *b == 0).filter(|t| !t.is_empty());
    while let Some(rec) = tokens.next() {
        match rec[0] {
            b'#' => {
                let text = String::from_utf8_lossy(rec);
                let mut it = text.splitn(3, ' ');
                let _ = it.next();
                let key = it.next().unwrap_or("");
                let val = it.next().unwrap_or("");
                match key {
                    "branch.oid" => st.branch.oid = Oid::from_hex(val),
                    "branch.head" => {
                        st.branch.head = if val == "(detached)" {
                            None
                        } else {
                            Some(sanitize::sanitize(val))
                        }
                    }
                    "branch.upstream" => st.branch.upstream = Some(sanitize::sanitize(val)),
                    "branch.ab" => {
                        let mut p = val.split(' ');
                        st.branch.ahead = p
                            .next()
                            .and_then(|a| a.trim_start_matches('+').parse().ok())
                            .unwrap_or(0);
                        st.branch.behind = p
                            .next()
                            .and_then(|b| b.trim_start_matches('-').parse().ok())
                            .unwrap_or(0);
                    }
                    _ => {}
                }
            }
            b'1' => {
                let f = fields(rec, 9)?;
                st.entries.push(StatusEntry {
                    path: RepoPath::from_bytes(f[8]),
                    orig_path: None,
                    kind: EntryKind::Changed,
                    x: f[1][0],
                    y: *f[1].get(1).unwrap_or(&b'.'),
                    mode_head: mode(f[3]),
                    mode_index: mode(f[4]),
                    mode_wt: mode(f[5]),
                    submodule: f[2].first() == Some(&b'S'),
                });
            }
            b'2' => {
                let f = fields(rec, 10)?;
                let orig = tokens
                    .next()
                    .ok_or_else(|| ParseError("rename without original path".into()))?;
                st.entries.push(StatusEntry {
                    path: RepoPath::from_bytes(f[9]),
                    orig_path: Some(RepoPath::from_bytes(orig)),
                    kind: EntryKind::Renamed,
                    x: f[1][0],
                    y: *f[1].get(1).unwrap_or(&b'.'),
                    mode_head: mode(f[3]),
                    mode_index: mode(f[4]),
                    mode_wt: mode(f[5]),
                    submodule: f[2].first() == Some(&b'S'),
                });
            }
            b'u' => {
                let f = fields(rec, 11)?;
                st.entries.push(StatusEntry {
                    path: RepoPath::from_bytes(f[10]),
                    orig_path: None,
                    kind: EntryKind::Unmerged,
                    x: f[1][0],
                    y: *f[1].get(1).unwrap_or(&b'.'),
                    mode_head: mode(f[3]),
                    mode_index: mode(f[5]),
                    mode_wt: mode(f[6]),
                    submodule: f[2].first() == Some(&b'S'),
                });
            }
            b'?' | b'!' => {
                let path = rec.get(2..).unwrap_or_default();
                st.entries.push(StatusEntry {
                    path: RepoPath::from_bytes(path),
                    orig_path: None,
                    kind: if rec[0] == b'?' {
                        EntryKind::Untracked
                    } else {
                        EntryKind::Ignored
                    },
                    x: rec[0],
                    y: rec[0],
                    mode_head: 0,
                    mode_index: 0,
                    mode_wt: 0,
                    submodule: false,
                });
            }
            _ => {
                return Err(ParseError(format!(
                    "unknown status record: {}",
                    sanitize::display_bytes(rec)
                )));
            }
        }
    }
    Ok(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_record_kinds() {
        let oid = "0123456789abcdef0123456789abcdef01234567";
        let mut s = Vec::new();
        s.extend_from_slice(format!("# branch.oid {oid}\0").as_bytes());
        s.extend_from_slice(b"# branch.head main\0");
        s.extend_from_slice(b"# branch.upstream origin/main\0");
        s.extend_from_slice(b"# branch.ab +2 -3\0");
        s.extend_from_slice(
            format!("1 MM N... 100644 100644 100644 {oid} {oid} dir/a file\0").as_bytes(),
        );
        s.extend_from_slice(
            format!("2 R. N... 100644 100644 100644 {oid} {oid} R100 new\0old\0").as_bytes(),
        );
        s.extend_from_slice(
            format!("u UU N... 100644 100644 100644 100644 {oid} {oid} {oid} conf\0")
                .as_bytes(),
        );
        s.extend_from_slice(b"? new\xff\0");
        let st = parse_porcelain_v2(&s).unwrap();
        assert_eq!(st.branch.head.as_deref(), Some("main"));
        assert_eq!(st.branch.ahead, 2);
        assert_eq!(st.branch.behind, 3);
        assert_eq!(st.entries.len(), 4);
        assert_eq!(st.entries[0].path.as_bytes(), b"dir/a file");
        assert!(st.entries[0].has_staged() && st.entries[0].has_unstaged());
        assert_eq!(st.entries[1].orig_path.as_ref().unwrap().as_bytes(), b"old");
        assert_eq!(st.entries[2].kind, EntryKind::Unmerged);
        assert!(!st.entries[2].has_staged());
        assert_eq!(st.entries[3].path.as_bytes(), b"new\xff");
        assert_eq!(st.entries[3].kind, EntryKind::Untracked);
    }

    #[test]
    fn detached_and_initial() {
        let st = parse_porcelain_v2(b"# branch.oid (initial)\0# branch.head (detached)\0").unwrap();
        assert_eq!(st.branch.oid, None);
        assert_eq!(st.branch.head, None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_porcelain_v2(b"Z what\0").is_err());
        assert!(parse_porcelain_v2(b"1 MM\0").is_err());
    }
}
