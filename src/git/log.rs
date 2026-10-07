//! History loading: view specification and streamed `git log` parsing.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::runner::{GitError, Opts, Runner, clean_message, os};
use crate::domain::commit::{Commit, Interner};
use crate::domain::graph::{GraphBuilder, GraphRow};
use crate::domain::oid::Oid;
use crate::domain::sanitize;

/// Pseudo commit ids used for gitk's local-changes rows.
pub fn null_unstaged() -> Oid {
    Oid::from_hex(&"0".repeat(40)).expect("valid")
}
pub fn null_staged() -> Oid {
    Oid::from_hex(&format!("{}1", "0".repeat(39))).expect("valid")
}

/// gitk view definition (View > New view...).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSpec {
    pub name: String,
    pub remember: bool,
    pub refs: String,
    pub all_refs: bool,
    pub branches: bool,
    pub tags: bool,
    pub remotes: bool,
    pub author: String,
    pub committer: String,
    pub message: String,
    pub all_match: bool,
    pub match_none: bool,
    pub changes: String,
    pub changes_regex: bool,
    pub since: String,
    pub until: String,
    pub max_count: String,
    pub skip: String,
    pub date_order: bool,
    pub left_right: bool,
    pub first_parent: bool,
    pub simplify: bool,
    pub extra_args: String,
    pub paths: String,
}

/// Splits a command-line like string (quotes and backslashes), without
/// any shell evaluation.
pub fn split_args(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => match chars.next() {
                Some(n) => cur.push(n),
                None => return Err("trailing backslash".into()),
            },
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                match chars.next() {
                    Some(n) => cur.push(n),
                    None => return Err("trailing backslash".into()),
                }
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

impl ViewSpec {
    pub fn default_view() -> ViewSpec {
        ViewSpec {
            name: "All files".into(),
            ..Default::default()
        }
    }

    /// Whether HEAD is part of the selected revisions.
    pub fn includes_head(&self) -> bool {
        let refs = self.refs.split_whitespace().collect::<Vec<_>>();
        refs.is_empty() || refs.contains(&"HEAD") || self.all_refs
    }

    pub fn has_paths(&self) -> bool {
        !self.paths.trim().is_empty()
    }

    pub fn path_list(&self) -> Vec<String> {
        self.paths
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    /// Revision-limiting arguments (without format, order handled here too).
    pub fn rev_args(&self) -> Result<(Vec<OsString>, Vec<OsString>), String> {
        let mut opts: Vec<OsString> = Vec::new();
        let mut revs: Vec<OsString> = Vec::new();
        opts.push(os(if self.date_order {
            "--date-order"
        } else {
            "--topo-order"
        }));
        if self.all_refs {
            opts.push(os("--all"));
        }
        if self.branches {
            opts.push(os("--branches"));
        }
        if self.tags {
            opts.push(os("--tags"));
        }
        if self.remotes {
            opts.push(os("--remotes"));
        }
        let regexp_kv = |k: &str, v: &str| os(format!("--{k}={v}"));
        if !self.author.trim().is_empty() {
            opts.push(regexp_kv("author", self.author.trim()));
        }
        if !self.committer.trim().is_empty() {
            opts.push(regexp_kv("committer", self.committer.trim()));
        }
        if !self.message.trim().is_empty() {
            opts.push(regexp_kv("grep", self.message.trim()));
        }
        if self.all_match {
            opts.push(os("--all-match"));
        }
        if self.match_none {
            opts.push(os("--invert-grep"));
        }
        if !self.changes.is_empty() {
            opts.push(os(format!("-S{}", self.changes)));
            if self.changes_regex {
                opts.push(os("--pickaxe-regex"));
            }
        }
        if !self.since.trim().is_empty() {
            opts.push(os(format!("--since={}", self.since.trim())));
        }
        if !self.until.trim().is_empty() {
            opts.push(os(format!("--until={}", self.until.trim())));
        }
        if !self.max_count.trim().is_empty() {
            let n: u64 = self
                .max_count
                .trim()
                .parse()
                .map_err(|_| "Number to show must be an integer")?;
            opts.push(os(format!("--max-count={n}")));
        }
        if !self.skip.trim().is_empty() {
            let n: u64 = self
                .skip
                .trim()
                .parse()
                .map_err(|_| "Number to skip must be an integer")?;
            opts.push(os(format!("--skip={n}")));
        }
        if self.left_right {
            opts.push(os("--left-right"));
        }
        if self.first_parent {
            opts.push(os("--first-parent"));
        }
        if self.simplify {
            opts.push(os("--simplify-by-decoration"));
        }
        for a in split_args(&self.extra_args)? {
            if a.starts_with("--output")
                || a == "-o"
                || a.starts_with("--format")
                || a.starts_with("--pretty")
                || a == "-z"
            {
                return Err(format!("argument not allowed in a view: {a}"));
            }
            opts.push(os(a));
        }
        for r in split_args(&self.refs)? {
            if r.starts_with('-') {
                return Err(format!("reference may not start with '-': {r}"));
            }
            revs.push(os(r));
        }
        let no_rev_selection =
            revs.is_empty() && !self.all_refs && !self.branches && !self.tags && !self.remotes;
        if no_rev_selection {
            revs.push(os("HEAD"));
        }
        Ok((opts, revs))
    }

    /// Full argument vector for `git log` (format excluded).
    pub fn log_args(&self) -> Result<Vec<OsString>, String> {
        let (opts, revs) = self.rev_args()?;
        let mut a = opts;
        a.push(os("--end-of-options"));
        a.extend(revs);
        a.push(os("--"));
        for p in self.path_list() {
            a.push(os(p));
        }
        Ok(a)
    }
}

pub const LOG_FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ad%x00%s";
pub const LOCAL_DATE: &str = "--date=format-local:%Y-%m-%d %H:%M:%S";

pub struct LogBatch {
    pub commits: Vec<Commit>,
    pub graph: Vec<GraphRow>,
}

/// Parses one 5-field record (fields without terminators).
fn parse_record(f: &[Vec<u8>; 5], interner: &mut Interner) -> Option<Commit> {
    let oid = Oid::from_hex_bytes(&f[0])?;
    let parents: Vec<Oid> = f[1]
        .split(|c| *c == b' ')
        .filter(|p| !p.is_empty())
        .filter_map(Oid::from_hex_bytes)
        .collect();
    let author = sanitize::display_bytes(&f[2]);
    Some(Commit {
        oid,
        parents: parents.into_boxed_slice(),
        author: interner.intern(&author),
        date: sanitize::display_bytes(&f[3]).into_boxed_str(),
        subject: sanitize::display_bytes(&f[4]).into_boxed_str(),
    })
}

/// Pseudo commits at the top of the list (gitk "local changes" rows).
pub struct LocalRows {
    pub unstaged: bool,
    pub staged: bool,
    pub head: Option<Oid>,
}

/// Streams the history. `sink` receives batches and returns false to stop.
pub fn stream_log(
    runner: &Runner,
    view: &ViewSpec,
    local: Option<LocalRows>,
    cancel: &AtomicBool,
    mut sink: impl FnMut(LogBatch) -> bool,
) -> Result<usize, GitError> {
    let mut args: Vec<OsString> = vec![
        os("log"),
        os("--no-color"),
        os("-z"),
        os("--parents"),
        os(LOG_FORMAT),
        os(LOCAL_DATE),
    ];
    args.extend(view.log_args().map_err(|e| GitError::new("git log", e))?);
    let mut graph = GraphBuilder::new();
    let mut interner = Interner::default();
    let mut batch = LogBatch {
        commits: Vec::new(),
        graph: Vec::new(),
    };
    if let Some(l) = local {
        let mut push_fake = |oid: Oid, parent: Option<Oid>, subject: &str| {
            let parents: Vec<Oid> = parent.into_iter().collect();
            batch.graph.push(graph.push(oid, &parents));
            batch.commits.push(Commit {
                oid,
                parents: parents.into_boxed_slice(),
                author: interner.intern(""),
                date: "".into(),
                subject: subject.into(),
            });
        };
        let staged_parent = l.head;
        if l.unstaged {
            let parent = if l.staged {
                Some(null_staged())
            } else {
                staged_parent
            };
            push_fake(
                null_unstaged(),
                parent,
                "Local uncommitted changes, not checked in to index",
            );
        }
        if l.staged {
            push_fake(
                null_staged(),
                staged_parent,
                "Local changes checked in to index but not committed",
            );
        }
    }
    let mut child = runner.spawn_stream(&args, Opts::default())?;
    let mut stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.take(1 << 20).read_to_end(&mut b);
        }
        b
    });
    let stdout = child.stdout.take().expect("piped");
    let mut reader = BufReader::with_capacity(256 * 1024, stdout);
    let mut fields: [Vec<u8>; 5] = Default::default();
    let mut nf = 0;
    let mut total = 0usize;
    let mut buf = Vec::new();
    let mut first = true;
    let mut stopped = false;
    loop {
        buf.clear();
        let n = reader
            .read_until(0, &mut buf)
            .map_err(|e| GitError::new("git log", e.to_string()))?;
        if n == 0 {
            break;
        }
        if buf.last() == Some(&0) {
            buf.pop();
        }
        // The record terminator follows the subject; tolerate a leading
        // newline git may emit between records.
        let field = if nf == 0 {
            buf.strip_prefix(b"\n").unwrap_or(&buf)
        } else {
            &buf[..]
        };
        fields[nf] = field.to_vec();
        nf += 1;
        if nf == 5 {
            nf = 0;
            if let Some(c) = parse_record(&fields, &mut interner) {
                batch.graph.push(graph.push(c.oid, &c.parents));
                batch.commits.push(c);
                total += 1;
            }
            let threshold = if first { 300 } else { 5000 };
            if batch.commits.len() >= threshold {
                first = false;
                if cancel.load(Ordering::Relaxed) {
                    stopped = true;
                    break;
                }
                let b = std::mem::replace(
                    &mut batch,
                    LogBatch {
                        commits: Vec::new(),
                        graph: Vec::new(),
                    },
                );
                if !sink(b) {
                    stopped = true;
                    break;
                }
            }
        }
    }
    if stopped {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(total);
    }
    let status = child
        .wait()
        .map_err(|e| GitError::new("git log", e.to_string()))?;
    let err = err_thread.join().unwrap_or_default();
    if !batch.commits.is_empty() {
        sink(batch);
    }
    if !status.success() {
        let msg = clean_message(&err);
        // An unborn HEAD simply means an empty history.
        if total == 0 && msg.contains("HEAD") && view.refs.trim().is_empty() {
            return Ok(0);
        }
        return Err(GitError {
            command: "git log".into(),
            code: status.code(),
            message: msg,
        });
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args_quotes() {
        assert_eq!(
            split_args("a 'b c' \"d\\\"e\" f\\ g").unwrap(),
            vec!["a", "b c", "d\"e", "f g"]
        );
        assert!(split_args("'open").is_err());
        assert_eq!(split_args("  ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn view_args() {
        let mut v = ViewSpec::default_view();
        let a = v.log_args().unwrap();
        assert_eq!(
            a,
            vec![
                os("--topo-order"),
                os("--end-of-options"),
                os("HEAD"),
                os("--")
            ]
        );
        v.all_refs = true;
        v.author = "bob".into();
        v.paths = "src\n docs \n".into();
        v.max_count = "10".into();
        let a = v.log_args().unwrap();
        assert!(a.contains(&os("--all")));
        assert!(a.contains(&os("--author=bob")));
        assert!(a.contains(&os("--max-count=10")));
        assert!(!a.contains(&os("HEAD")));
        assert_eq!(&a[a.len() - 2..], &[os("src"), os("docs")]);
        v.refs = "-x".into();
        assert!(v.log_args().is_err());
        v.refs.clear();
        v.extra_args = "--output=/tmp/x".into();
        assert!(v.log_args().is_err());
        v.extra_args.clear();
        v.max_count = "abc".into();
        assert!(v.log_args().is_err());
    }
}
