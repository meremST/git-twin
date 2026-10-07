//! Read-only Git queries.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::sync::atomic::{AtomicBool, Ordering};

use super::log::{LOCAL_DATE, ViewSpec};
use super::runner::{GitError, Opts, Runner, clean_message, os};
use crate::domain::commit::CommitDetails;
use crate::domain::diff::{Diff, ParseMode, parse_diff};
use crate::domain::oid::Oid;
use crate::domain::refs::{GitRef, Head, RefSet};
use crate::domain::sanitize;
use crate::domain::status::{RepoStatus, parse_porcelain_v2};
use crate::domain::textmatch::{MatchKind, Matcher};

fn bytes_arg(b: &[u8]) -> OsString {
    OsString::from_vec(b.to_vec())
}

/// `<rev>:<path>` object name for `cat-file`.
pub fn rev_path(rev: &Oid, path: &[u8]) -> OsString {
    let mut v = rev.to_hex().into_bytes();
    v.push(b':');
    v.extend_from_slice(path);
    OsString::from_vec(v)
}

pub fn load_refs(r: &Runner) -> Result<RefSet, GitError> {
    let out = r.read(&[
        os("for-each-ref"),
        os("--format=%(objectname)%00%(*objectname)%00%(refname)"),
    ])?;
    let mut refs = Vec::new();
    for line in out.split(|c| *c == b'\n') {
        let mut f = line.split(|c| *c == 0);
        let (Some(obj), Some(peeled), Some(name)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let target = if peeled.is_empty() { obj } else { peeled };
        if let Some(oid) = Oid::from_hex_bytes(target) {
            refs.push(GitRef::from_full(name, oid));
        }
    }
    let head_oid = r
        .read_opts(
            &[os("rev-parse"), os("-q"), os("--verify"), os("HEAD^{commit}")],
            Opts::default(),
            &[0, 1, 128],
        )
        .ok()
        .and_then(|o| {
            let s = o.stdout;
            Oid::from_hex_bytes(s.strip_suffix(b"\n").unwrap_or(&s))
        });
    let sym = r.read_opts(
        &[os("symbolic-ref"), os("-q"), os("HEAD")],
        Opts::default(),
        &[0, 1],
    )?;
    let head = if sym.status.success() {
        let full = sym.stdout.strip_suffix(b"\n").unwrap_or(&sym.stdout).to_vec();
        let short = sanitize::display_bytes(full.strip_prefix(b"refs/heads/").unwrap_or(&full));
        Head::Branch {
            full,
            short,
            oid: head_oid,
        }
    } else {
        match head_oid {
            Some(o) => Head::Detached(o),
            None => return Err(GitError::new("git rev-parse", "cannot resolve HEAD")),
        }
    };
    Ok(RefSet::new(refs, head))
}

pub fn commit_details(r: &Runner, oid: &Oid) -> Result<CommitDetails, GitError> {
    let out = r.read(&[
        os("show"),
        os("-s"),
        os("--no-color"),
        os("--format=%H%x00%P%x00%an <%ae>%x00%ad%x00%cn <%ce>%x00%cd%x00%B"),
        os(LOCAL_DATE),
        os(oid.to_hex()),
    ])?;
    let mut f = out.splitn(7, |c| *c == 0);
    let mut next = || f.next().unwrap_or_default();
    let h = next();
    let parents = next()
        .split(|c| *c == b' ')
        .filter_map(Oid::from_hex_bytes)
        .collect();
    let author = sanitize::display_bytes(next());
    let author_date = sanitize::display_bytes(next());
    let committer = sanitize::display_bytes(next());
    let committer_date = sanitize::display_bytes(next());
    let message = String::from_utf8_lossy(next()).trim_end().to_string();
    Ok(CommitDetails {
        oid: Oid::from_hex_bytes(h),
        parents,
        author,
        author_date,
        committer,
        committer_date,
        message,
    })
}

/// Branches containing a commit, nearest preceding and following tags
/// (approximation of gitk's Branches / Follows / Precedes lines).
#[derive(Clone, Debug, Default)]
pub struct Related {
    pub branches: Vec<String>,
    pub follows: Option<String>,
    pub precedes: Option<String>,
}

pub fn related(r: &Runner, oid: &Oid) -> Related {
    let hex = oid.to_hex();
    let mut rel = Related::default();
    if let Ok(o) = r.read(&[
        os("for-each-ref"),
        os("--format=%(refname:short)"),
        os(format!("--contains={hex}")),
        os("refs/heads"),
        os("refs/remotes"),
    ]) {
        rel.branches = String::from_utf8_lossy(&o)
            .lines()
            .take(50)
            .map(sanitize::sanitize)
            .collect();
    }
    if let Ok(o) = r.read_opts(
        &[
            os("describe"),
            os("--tags"),
            os("--abbrev=0"),
            os(format!("{hex}^")),
        ],
        Opts::default(),
        &[0],
    ) {
        let s = o.stdout_text();
        if !s.is_empty() {
            rel.follows = Some(s);
        }
    }
    if let Ok(o) = r.read_opts(
        &[
            os("name-rev"),
            os("--tags"),
            os("--name-only"),
            os("--no-undefined"),
            os(&hex),
        ],
        Opts::default(),
        &[0],
    ) {
        let s = o.stdout_text();
        let tag = s.split(['~', '^']).next().unwrap_or("").to_string();
        if !tag.is_empty() {
            rel.precedes = Some(tag.trim_start_matches("tags/").to_string());
        }
    }
    rel
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WordMode {
    #[default]
    Line,
    Markup,
    Color,
}

impl WordMode {
    pub fn label(self) -> &'static str {
        match self {
            WordMode::Line => "Line diff",
            WordMode::Markup => "Markup words",
            WordMode::Color => "Color words",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffOpts {
    pub context: u32,
    pub ignore_space: bool,
    pub words: WordMode,
}

impl Default for DiffOpts {
    fn default() -> Self {
        DiffOpts {
            context: 3,
            ignore_space: false,
            words: WordMode::Line,
        }
    }
}

impl DiffOpts {
    fn args(&self) -> Vec<OsString> {
        let mut v = vec![os(format!("-U{}", self.context.min(10_000)))];
        if self.ignore_space {
            v.push(os("-b"));
        }
        if self.words != WordMode::Line {
            v.push(os("--word-diff=porcelain"));
        }
        v
    }
    fn mode(&self) -> ParseMode {
        if self.words == WordMode::Line {
            ParseMode::Normal
        } else {
            ParseMode::WordPorcelain
        }
    }
}

/// How merge commits are diffed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MergeDiff {
    /// Combined diff (`--cc`), as gitk does by default.
    #[default]
    Combined,
    /// Diff against parent N (1-based).
    Parent(usize),
}

fn run_diff(r: &Runner, args: Vec<OsString>, mode: ParseMode, ok: &[i32]) -> Result<Diff, GitError> {
    let o = r.read_opts(
        &args,
        Opts {
            literal_paths: true,
            ..Default::default()
        },
        ok,
    )?;
    Ok(parse_diff(&o.stdout, mode, o.truncated))
}

/// Diff introduced by a commit (paths restrict the output if not empty).
pub fn commit_diff(
    r: &Runner,
    oid: &Oid,
    parents: &[Oid],
    merge: MergeDiff,
    opts: &DiffOpts,
    paths: &[String],
) -> Result<Diff, GitError> {
    let mut a = vec![os("diff-tree"), os("-r"), os("-p"), os("--no-commit-id"), os("-M")];
    a.extend(r.diff_safety_args());
    a.extend(opts.args());
    match (parents.len(), merge) {
        (0, _) => {
            a.push(os("--root"));
            a.push(os(oid.to_hex()));
        }
        (1, _) => a.push(os(oid.to_hex())),
        (_, MergeDiff::Combined) => {
            a.push(os("--cc"));
            a.push(os(oid.to_hex()));
        }
        (n, MergeDiff::Parent(p)) => {
            let p = p.clamp(1, n);
            a.push(os(parents[p - 1].to_hex()));
            a.push(os(oid.to_hex()));
        }
    }
    a.push(os("--"));
    a.extend(paths.iter().map(os));
    let mut d = run_diff(r, a, opts.mode(), &[0])?;
    // Pure renames / mode changes have no hunks but are still listed.
    d.files.retain(|f| !f.header.is_empty() || !f.hunks.is_empty());
    Ok(d)
}

/// Diff between two arbitrary commits (`diff this -> selected`).
pub fn diff_commits(r: &Runner, from: &Oid, to: &Oid, opts: &DiffOpts) -> Result<Diff, GitError> {
    let mut a = vec![os("diff-tree"), os("-r"), os("-p"), os("-M")];
    a.extend(r.diff_safety_args());
    a.extend(opts.args());
    a.push(os(from.to_hex()));
    a.push(os(to.to_hex()));
    run_diff(r, a, opts.mode(), &[0])
}

/// Worktree vs index (`cached = false`) or index vs base (`cached = true`).
/// `base` overrides HEAD for the cached side (amend mode).
pub fn worktree_diff(
    r: &Runner,
    cached: bool,
    base: Option<&Oid>,
    opts: &DiffOpts,
    paths: &[&[u8]],
) -> Result<Diff, GitError> {
    let mut a = vec![os("diff"), os("-p")];
    a.extend(r.diff_safety_args());
    a.extend(opts.args());
    if cached {
        a.push(os("--cached"));
        if let Some(b) = base {
            a.push(os(b.to_hex()));
        }
    }
    a.push(os("--"));
    a.extend(paths.iter().map(|p| bytes_arg(p)));
    run_diff(r, a, opts.mode(), &[0])
}

/// Preview of an untracked file as a new-file diff.
pub fn untracked_diff(r: &Runner, path: &[u8], opts: &DiffOpts) -> Result<Diff, GitError> {
    let mut a = vec![os("diff"), os("--no-index"), os("-p")];
    a.extend(r.diff_safety_args());
    a.retain(|x| x != "--no-relative");
    a.extend(opts.args());
    a.push(os("--"));
    a.push(os("/dev/null"));
    a.push(bytes_arg(path));
    let mut d = run_diff(r, a, opts.mode(), &[0, 1])?;
    // `--no-index` prints the path relative to cwd: normalise to repo path.
    for f in &mut d.files {
        f.new_path = Some(path.to_vec());
    }
    Ok(d)
}

/// All paths in a commit's tree.
pub fn tree_paths(r: &Runner, oid: &Oid) -> Result<Vec<Vec<u8>>, GitError> {
    let out = r.read(&[
        os("ls-tree"),
        os("-r"),
        os("-z"),
        os("--name-only"),
        os("--full-tree"),
        os(oid.to_hex()),
    ])?;
    Ok(out
        .split(|c| *c == 0)
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}

pub struct Blob {
    pub data: Vec<u8>,
    pub truncated: bool,
}

impl Blob {
    pub fn is_binary(&self) -> bool {
        self.data.iter().take(8000).any(|c| *c == 0)
    }
}

pub fn blob(r: &Runner, oid: &Oid, path: &[u8]) -> Result<Blob, GitError> {
    let o = r.read_opts(
        &[os("cat-file"), os("blob"), rev_path(oid, path)],
        Opts {
            limit: Some(4 * 1024 * 1024),
            ..Default::default()
        },
        &[0],
    )?;
    Ok(Blob {
        data: o.stdout,
        truncated: o.truncated,
    })
}

pub fn status(r: &Runner) -> Result<RepoStatus, GitError> {
    let out = r.read(&[
        os("status"),
        os("--porcelain=v2"),
        os("-z"),
        os("--branch"),
        os("--untracked-files=all"),
        os("--no-renames"),
        os("--ignore-submodules=none"),
    ])?;
    parse_porcelain_v2(&out).map_err(|e| GitError::new("git status", e.0))
}

/// Staged entries against an explicit base (amend mode): (status, path).
pub fn staged_against(r: &Runner, base: Option<&Oid>) -> Result<Vec<(u8, Vec<u8>)>, GitError> {
    let empty = empty_tree(r)?;
    let base_arg = match base {
        Some(b) => os(b.to_hex()),
        None => os(empty),
    };
    let out = r.read(&[
        os("diff-index"),
        os("--cached"),
        os("-z"),
        os("--name-status"),
        os("--no-renames"),
        base_arg,
    ])?;
    let mut v = Vec::new();
    let mut it = out.split(|c| *c == 0).filter(|t| !t.is_empty());
    while let (Some(st), Some(path)) = (it.next(), it.next()) {
        v.push((st[0], path.to_vec()));
    }
    Ok(v)
}

pub fn empty_tree(r: &Runner) -> Result<String, GitError> {
    let o = r.read_stdin(
        &[os("hash-object"), os("-t"), os("tree"), os("--stdin")],
        b"",
        Opts::default(),
    )?;
    Ok(o.stdout_text())
}

/// Commit message of a revision (amend).
pub fn message_of(r: &Runner, rev: &str) -> Result<String, GitError> {
    let o = r.read(&[
        os("log"),
        os("-1"),
        os("--no-color"),
        os("--format=%B"),
        os("--end-of-options"),
        os(rev),
        os("--"),
    ])?;
    Ok(String::from_utf8_lossy(&o).trim_end().to_string())
}

pub fn config_get(r: &Runner, key: &str) -> Option<String> {
    let o = r
        .read_opts(&[os("config"), os("--get"), os(key)], Opts::default(), &[0, 1])
        .ok()?;
    if o.status.success() {
        Some(String::from_utf8_lossy(&o.stdout).trim_end().to_string())
    } else {
        None
    }
}

pub fn config_bool(r: &Runner, key: &str) -> Option<bool> {
    let o = r
        .read_opts(
            &[os("config"), os("--type=bool"), os("--get"), os(key)],
            Opts::default(),
            &[0, 1],
        )
        .ok()?;
    match o.stdout_text().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// `git var GIT_COMMITTER_IDENT` without the timestamp.
pub fn committer_ident(r: &Runner) -> Result<String, GitError> {
    let o = r.read(&[os("var"), os("GIT_COMMITTER_IDENT")])?;
    let s = String::from_utf8_lossy(&o).trim_end().to_string();
    let end = s.rfind('>').map(|i| i + 1).unwrap_or(s.len());
    Ok(s[..end].to_string())
}

pub fn remotes(r: &Runner) -> Result<Vec<String>, GitError> {
    let o = r.read(&[os("remote")])?;
    Ok(String::from_utf8_lossy(&o)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// User tools from `guitool.<name>.cmd`.
#[derive(Clone, Debug)]
pub struct GuiTool {
    pub name: String,
    pub cmd: String,
    pub confirm: bool,
    pub needs_file: bool,
    pub no_console: bool,
}

pub fn guitools(r: &Runner) -> Vec<GuiTool> {
    let Ok(o) = r.read_opts(
        &[os("config"), os("-z"), os("--get-regexp"), os(r"^guitool\..*\.")],
        Opts::default(),
        &[0, 1],
    ) else {
        return Vec::new();
    };
    let mut tools: Vec<GuiTool> = Vec::new();
    for entry in o.stdout.split(|c| *c == 0).filter(|e| !e.is_empty()) {
        let s = String::from_utf8_lossy(entry);
        let (key, val) = s.split_once('\n').unwrap_or((&s, ""));
        let Some(rest) = key.strip_prefix("guitool.") else {
            continue;
        };
        let Some((name, var)) = rest.rsplit_once('.') else {
            continue;
        };
        let idx = match tools.iter().position(|t| t.name == name) {
            Some(i) => i,
            None => {
                tools.push(GuiTool {
                    name: name.to_string(),
                    cmd: String::new(),
                    confirm: false,
                    needs_file: false,
                    no_console: false,
                });
                tools.len() - 1
            }
        };
        let t = &mut tools[idx];
        let truthy = matches!(val, "" | "true" | "yes" | "on" | "1");
        match var {
            "cmd" => t.cmd = val.to_string(),
            "confirm" => t.confirm = truthy,
            "needsfile" => t.needs_file = truthy,
            "noconsole" => t.no_console = truthy,
            _ => {}
        }
    }
    tools.retain(|t| !t.cmd.is_empty());
    tools
}

/// Line origin for gitk's "Show origin of this line".
pub fn blame_line(r: &Runner, rev: &Oid, path: &[u8], line: u32) -> Result<(Oid, u32), GitError> {
    let o = r.read_opts(
        &[
            os("blame"),
            os("-p"),
            os(format!("-L{line},{line}")),
            os(rev.to_hex()),
            os("--"),
            bytes_arg(path),
        ],
        Opts {
            literal_paths: true,
            ..Default::default()
        },
        &[0],
    )?;
    let first = o.stdout.split(|c| *c == b'\n').next().unwrap_or_default();
    let mut f = first.split(|c| *c == b' ');
    let oid = f
        .next()
        .and_then(Oid::from_hex_bytes)
        .ok_or_else(|| GitError::new("git blame", "unexpected output"))?;
    let orig_line = f
        .next()
        .and_then(|l| std::str::from_utf8(l).ok()?.parse().ok())
        .unwrap_or(line);
    Ok((oid, orig_line))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FindType {
    #[default]
    Containing,
    TouchingPaths,
    AddingRemoving,
    ChangingLines,
}

impl FindType {
    pub const ALL: [FindType; 4] = [
        FindType::Containing,
        FindType::TouchingPaths,
        FindType::AddingRemoving,
        FindType::ChangingLines,
    ];
    pub fn label(self) -> &'static str {
        match self {
            FindType::Containing => "containing:",
            FindType::TouchingPaths => "touching paths:",
            FindType::AddingRemoving => "adding/removing string:",
            FindType::ChangingLines => "changing lines matching:",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FindField {
    #[default]
    All,
    Headline,
    Comments,
    Author,
    Committer,
}

impl FindField {
    pub const ALL: [FindField; 5] = [
        FindField::All,
        FindField::Headline,
        FindField::Comments,
        FindField::Author,
        FindField::Committer,
    ];
    pub fn label(self) -> &'static str {
        match self {
            FindField::All => "All fields",
            FindField::Headline => "Headline",
            FindField::Comments => "Comments",
            FindField::Author => "Author",
            FindField::Committer => "Committer",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FindSpec {
    pub kind: FindType,
    pub pattern: String,
    pub matching: MatchKind,
    pub field: FindField,
}

/// Runs a find over the whole view; returns matching commits.
pub fn find(
    r: &Runner,
    view: &ViewSpec,
    spec: &FindSpec,
    cancel: &AtomicBool,
) -> Result<HashSet<Oid>, GitError> {
    let (opts, revs) = view.rev_args().map_err(|e| GitError::new("git log", e))?;
    let mut a = vec![os("log"), os("--no-color"), os("-z")];
    let containing = spec.kind == FindType::Containing;
    if containing {
        a.push(os("--format=%H%x00%an <%ae>%x00%cn <%ce>%x00%B"));
    } else {
        a.push(os("--format=%H"));
    }
    a.extend(opts);
    let mut paths: Vec<OsString> = view.path_list().into_iter().map(os).collect();
    match spec.kind {
        FindType::Containing => {}
        FindType::TouchingPaths => {
            paths = super::log::split_args(&spec.pattern)
                .map_err(|e| GitError::new("git log", e))?
                .into_iter()
                .map(os)
                .collect();
        }
        FindType::AddingRemoving => {
            a.push(os(format!("-S{}", spec.pattern)));
            if spec.matching == MatchKind::Regexp {
                a.push(os("--pickaxe-regex"));
            }
            if spec.matching == MatchKind::IgnCase {
                a.push(os("-i"));
            }
        }
        FindType::ChangingLines => {
            let pat = match spec.matching {
                MatchKind::Regexp => spec.pattern.clone(),
                _ => regex::escape(&spec.pattern),
            };
            a.push(os(format!("-G{pat}")));
            if spec.matching == MatchKind::IgnCase {
                a.push(os("-i"));
            }
        }
    }
    a.push(os("--end-of-options"));
    a.extend(revs);
    a.push(os("--"));
    a.extend(paths);
    let matcher = if containing {
        Some(Matcher::new(spec.matching, &spec.pattern).map_err(|e| GitError::new("find", e))?)
    } else {
        None
    };
    let mut child = r.spawn_stream(&a, Opts::default())?;
    let stdout = child.stdout.take().expect("piped");
    let mut rd = BufReader::new(stdout);
    let mut found = HashSet::new();
    let nfields = if containing { 4 } else { 1 };
    let mut fields: Vec<Vec<u8>> = Vec::with_capacity(4);
    let mut buf = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(found);
        }
        buf.clear();
        let n = rd
            .read_until(0, &mut buf)
            .map_err(|e| GitError::new("git log", e.to_string()))?;
        if n == 0 {
            break;
        }
        if buf.last() == Some(&0) {
            buf.pop();
        }
        let tok = if fields.is_empty() {
            buf.strip_prefix(b"\n").unwrap_or(&buf).to_vec()
        } else {
            buf.clone()
        };
        fields.push(tok);
        if fields.len() == nfields {
            if let Some(oid) = Oid::from_hex_bytes(fields[0].trim_ascii()) {
                let hit = match &matcher {
                    None => true,
                    Some(m) => {
                        let msg = String::from_utf8_lossy(&fields[3]);
                        let (head, body) = msg.split_once('\n').unwrap_or((&msg, ""));
                        let t = |b: &[u8]| m.is_match(&String::from_utf8_lossy(b));
                        match spec.field {
                            FindField::All => {
                                m.is_match(&msg) || t(&fields[1]) || t(&fields[2])
                            }
                            FindField::Headline => m.is_match(head),
                            FindField::Comments => m.is_match(body),
                            FindField::Author => t(&fields[1]),
                            FindField::Committer => t(&fields[2]),
                        }
                    }
                };
                if hit {
                    found.insert(oid);
                }
            }
            fields.clear();
        }
    }
    let st = child
        .wait()
        .map_err(|e| GitError::new("git log", e.to_string()))?;
    if !st.success() && found.is_empty() {
        let mut err = Vec::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_end(&mut err);
        }
        let msg = clean_message(&err);
        if !msg.is_empty() {
            return Err(GitError {
                command: "git log (find)".into(),
                code: st.code(),
                message: msg,
            });
        }
    }
    Ok(found)
}

/// Resolve a user expression (SHA1 prefix, ref) to a commit.
pub fn resolve_commit(r: &Runner, expr: &str) -> Result<Oid, GitError> {
    if expr.starts_with('-') || expr.trim().is_empty() {
        return Err(GitError::new("git rev-parse", format!("invalid revision: {expr}")));
    }
    let o = r.read_opts(
        &[
            os("rev-parse"),
            os("--verify"),
            os("-q"),
            os("--end-of-options"),
            os(format!("{}^{{commit}}", expr.trim())),
        ],
        Opts::default(),
        &[0, 1, 128],
    )?;
    let s = o.stdout_text();
    Oid::from_hex(&s).ok_or_else(|| {
        GitError::new(
            "git rev-parse",
            format!("{} is not a known commit", sanitize::sanitize(expr)),
        )
    })
}

pub fn count_objects(r: &Runner) -> Result<String, GitError> {
    let o = r.read(&[os("count-objects"), os("-v"), os("-H")])?;
    Ok(clean_message(&o))
}

pub fn is_ancestor(r: &Runner, a: &Oid, b: &Oid) -> bool {
    r.read_opts(
        &[
            os("merge-base"),
            os("--is-ancestor"),
            os(a.to_hex()),
            os(b.to_hex()),
        ],
        Opts::default(),
        &[0, 1],
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// Patch text for a commit (gitk "Write commit to file").
pub fn show_commit_text(r: &Runner, oid: &Oid) -> Result<Vec<u8>, GitError> {
    let mut a = vec![os("show"), os("--pretty=medium"), os("-p")];
    a.extend(r.diff_safety_args());
    a.push(os(oid.to_hex()));
    a.push(os("--"));
    r.read(&a)
}

/// Patch between two commits (gitk "Make patch").
pub fn patch_between(r: &Runner, from: &Oid, to: &Oid) -> Result<Vec<u8>, GitError> {
    let mut a = vec![os("diff-tree"), os("-r"), os("-p"), os("--stat")];
    a.extend(r.diff_safety_args());
    a.push(os(from.to_hex()));
    a.push(os(to.to_hex()));
    r.read(&a)
}

pub fn os_bytes(s: &OsString) -> &[u8] {
    s.as_os_str().as_bytes()
}
