//! Mutating Git operations. Every function goes through `Runner::write`
//! (refused in read-only mode) or the hand-off launcher.

use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

use super::runner::{GitError, Opts, Runner, clean_message, os};
use crate::domain::oid::Oid;
use crate::domain::sanitize;

fn literal() -> Opts {
    Opts {
        literal_paths: true,
        ..Default::default()
    }
}

fn nul_list(paths: &[Vec<u8>]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in paths {
        v.extend_from_slice(p);
        v.push(0);
    }
    v
}

fn done(o: super::runner::Output) -> String {
    let mut s = o.stdout_text();
    let e = o.stderr_text();
    if !e.is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(&e);
    }
    s
}

/// Stages whole files (git gui "Stage To Commit").
pub fn stage(r: &Runner, paths: &[Vec<u8>]) -> Result<String, GitError> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    r.write(
        &[
            os("update-index"),
            os("--add"),
            os("--remove"),
            os("-z"),
            os("--stdin"),
        ],
        Some(&nul_list(paths)),
        literal(),
    )
    .map(done)
}

/// Unstages whole files. `base` is the commit to reset the index entries
/// to (HEAD, or HEAD^ in amend mode); None means the empty tree.
pub fn unstage(r: &Runner, paths: &[Vec<u8>], base: Option<&Oid>) -> Result<String, GitError> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    let input = nul_list(paths);
    let args: Vec<OsString> = match base {
        Some(b) => vec![
            os("reset"),
            os("-q"),
            os(b.to_hex()),
            os("--pathspec-from-file=-"),
            os("--pathspec-file-nul"),
        ],
        None => vec![
            os("rm"),
            os("--cached"),
            os("-q"),
            os("-r"),
            os("--ignore-unmatch"),
            os("--pathspec-from-file=-"),
            os("--pathspec-file-nul"),
        ],
    };
    r.write(&args, Some(&input), literal()).map(done)
}

/// Discards worktree changes of tracked files (checkout from the index).
pub fn revert_files(r: &Runner, paths: &[Vec<u8>]) -> Result<String, GitError> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    r.write(
        &[
            os("checkout-index"),
            os("--index"),
            os("-q"),
            os("-f"),
            os("-z"),
            os("--stdin"),
        ],
        Some(&nul_list(paths)),
        literal(),
    )
    .map(done)
}

/// Checks a repository-relative path and returns its location inside
/// `workdir`, refusing anything that could escape it.
pub fn safe_join(workdir: &Path, rel: &[u8]) -> Result<PathBuf, String> {
    let p = Path::new(std::ffi::OsStr::from_bytes(rel));
    if rel.is_empty() || p.is_absolute() {
        return Err("invalid path".into());
    }
    let mut cur = workdir.to_path_buf();
    let comps: Vec<Component> = p.components().collect();
    for (i, c) in comps.iter().enumerate() {
        match c {
            Component::Normal(n) => {
                if n.as_bytes() == b".git" {
                    return Err("refusing to touch .git".into());
                }
                cur.push(n);
                if i + 1 < comps.len() {
                    let md = std::fs::symlink_metadata(&cur).map_err(|e| e.to_string())?;
                    if !md.is_dir() {
                        return Err("parent is not a plain directory".into());
                    }
                }
            }
            _ => return Err("path escapes the working tree".into()),
        }
    }
    Ok(cur)
}

/// Deletes untracked files after confirmation (git gui "Revert" on
/// untracked files). The repository is never touched outside `workdir`.
pub fn delete_untracked(r: &Runner, workdir: &Path, paths: &[Vec<u8>]) -> Result<String, GitError> {
    if r.read_only {
        return Err(GitError::new(
            "delete",
            "refused: git-twin runs in --read-only mode",
        ));
    }
    let mut errors = Vec::new();
    for p in paths {
        match safe_join(workdir, p) {
            Ok(full) => {
                let md = std::fs::symlink_metadata(&full);
                let res = match md {
                    Ok(m) if m.is_dir() => std::fs::remove_dir(&full),
                    Ok(_) => std::fs::remove_file(&full),
                    Err(e) => Err(e),
                };
                if let Err(e) = res {
                    errors.push(format!("{}: {e}", sanitize::display_bytes(p)));
                }
            }
            Err(e) => errors.push(format!("{}: {e}", sanitize::display_bytes(p))),
        }
    }
    if errors.is_empty() {
        Ok(format!("Deleted {} file(s)", paths.len()))
    } else {
        Err(GitError::new("delete", errors.join("\n")))
    }
}

/// Applies a partial patch to the index or worktree after `--check`.
pub fn apply_patch(
    r: &Runner,
    patch: &[u8],
    cached: bool,
    reverse: bool,
    zero_context: bool,
) -> Result<String, GitError> {
    let mut base = vec![os("apply"), os("--whitespace=nowarn")];
    if cached {
        base.push(os("--cached"));
    }
    if reverse {
        base.push(os("--reverse"));
    }
    if zero_context {
        base.push(os("--unidiff-zero"));
    }
    let mut check = base.clone();
    check.push(os("--check"));
    check.push(os("-"));
    r.write(&check, Some(patch), Opts::default())?;
    base.push(os("-"));
    r.write(&base, Some(patch), Opts::default()).map(done)
}

pub struct CommitRequest<'a> {
    pub message: &'a str,
    pub amend: bool,
    /// Let git use the terminal (signing with a pinentry, hooks prompts).
    pub handoff: bool,
}

/// Commits the index with `git commit -F <tmp>` so that hooks, signing
/// and `commit.*` settings apply as usual.
pub fn commit(r: &Runner, req: &CommitRequest) -> Result<String, GitError> {
    if r.read_only {
        return Err(GitError::new(
            "git commit",
            "refused: git-twin runs in --read-only mode",
        ));
    }
    let mut tmp = tempfile::Builder::new()
        .prefix("git-twin-msg-")
        .tempfile()
        .map_err(|e| GitError::new("git commit", format!("cannot create temp file: {e}")))?;
    tmp.write_all(req.message.as_bytes())
        .and_then(|_| tmp.flush())
        .map_err(|e| GitError::new("git commit", e.to_string()))?;
    let mut a = vec![
        os("commit"),
        os("-F"),
        tmp.path().as_os_str().to_os_string(),
    ];
    if req.amend {
        a.push(os("--amend"));
    }
    if req.handoff {
        let st = handoff(r, &a, None)?;
        if st {
            Ok("Commit created".into())
        } else {
            Err(GitError::new(
                "git commit",
                "commit failed (see terminal output)",
            ))
        }
    } else {
        r.write(&a, None, Opts::default()).map(done)
    }
}

pub fn check_ref_format_branch(r: &Runner, name: &str) -> Result<(), GitError> {
    if name.is_empty() || name.starts_with('-') {
        return Err(GitError::new("branch", "invalid branch name"));
    }
    r.read_opts(
        &[os("check-ref-format"), os("--branch"), os(name)],
        Opts::default(),
        &[0],
    )
    .map(|_| ())
    .map_err(|_| {
        GitError::new(
            "branch",
            format!("'{}' is not a valid branch name", sanitize::sanitize(name)),
        )
    })
}

fn check_tag_name(r: &Runner, name: &str) -> Result<(), GitError> {
    if name.is_empty() || name.starts_with('-') {
        return Err(GitError::new("tag", "invalid tag name"));
    }
    r.read_opts(
        &[os("check-ref-format"), os(format!("refs/tags/{name}"))],
        Opts::default(),
        &[0],
    )
    .map(|_| ())
    .map_err(|_| {
        GitError::new(
            "tag",
            format!("'{}' is not a valid tag name", sanitize::sanitize(name)),
        )
    })
}

pub fn create_branch(
    r: &Runner,
    name: &str,
    start: &str,
    force: bool,
    checkout: bool,
) -> Result<String, GitError> {
    check_ref_format_branch(r, name)?;
    let mut a = vec![os("branch")];
    if force {
        a.push(os("-f"));
    }
    a.push(os("--end-of-options"));
    a.push(os(name));
    a.push(os(start));
    let mut msg = r.write(&a, None, Opts::default()).map(done)?;
    if checkout {
        let m = checkout_branch(r, name)?;
        if !m.is_empty() {
            msg.push_str(&m);
        }
    }
    Ok(msg)
}

/// Checks out a local branch, or detaches at a commit/tag/remote branch.
pub fn checkout_branch(r: &Runner, name: &str) -> Result<String, GitError> {
    if name.starts_with('-') {
        return Err(GitError::new("checkout", "invalid name"));
    }
    r.write(
        &[os("checkout"), os("-q"), os(name), os("--")],
        None,
        Opts::default(),
    )
    .map(done)
}

pub fn checkout_detach(r: &Runner, oid: &Oid) -> Result<String, GitError> {
    r.write(
        &[
            os("checkout"),
            os("-q"),
            os("--detach"),
            os(oid.to_hex()),
            os("--"),
        ],
        None,
        Opts::default(),
    )
    .map(done)
}

pub fn rename_branch(r: &Runner, old: &[u8], new: &str, force: bool) -> Result<String, GitError> {
    check_ref_format_branch(r, new)?;
    r.write(
        &[
            os("branch"),
            os(if force { "-M" } else { "-m" }),
            os("--end-of-options"),
            OsString::from_vec(old.to_vec()),
            os(new),
        ],
        None,
        Opts::default(),
    )
    .map(done)
}

/// Deletes a local branch; `force` uses `-D` (unmerged commits).
pub fn delete_branch(r: &Runner, short: &[u8], force: bool) -> Result<String, GitError> {
    r.write(
        &[
            os("branch"),
            os(if force { "-D" } else { "-d" }),
            os("--end-of-options"),
            OsString::from_vec(short.to_vec()),
        ],
        None,
        Opts::default(),
    )
    .map(done)
}

pub fn create_tag(r: &Runner, name: &str, oid: &Oid, message: &str) -> Result<String, GitError> {
    check_tag_name(r, name)?;
    let mut a = vec![os("tag")];
    if !message.trim().is_empty() {
        a.push(os("-a"));
        a.push(os("-m"));
        a.push(os(message));
    }
    a.push(os("--end-of-options"));
    a.push(os(name));
    a.push(os(oid.to_hex()));
    r.write(&a, None, Opts::default()).map(done)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

impl ResetMode {
    pub fn flag(self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
        }
    }
}

pub fn reset(r: &Runner, mode: ResetMode, oid: &Oid) -> Result<String, GitError> {
    r.write(
        &[
            os("reset"),
            os("-q"),
            os(mode.flag()),
            os(oid.to_hex()),
            os("--"),
        ],
        None,
        Opts::default(),
    )
    .map(done)
}

pub fn cherry_pick(r: &Runner, oid: &Oid, mainline: Option<usize>) -> Result<String, GitError> {
    let mut a = vec![os("cherry-pick")];
    if let Some(m) = mainline {
        a.push(os(format!("-m{m}")));
    }
    a.push(os(oid.to_hex()));
    r.write(&a, None, Opts::default()).map(done)
}

pub fn revert_commit(r: &Runner, oid: &Oid, mainline: Option<usize>) -> Result<String, GitError> {
    let mut a = vec![os("revert"), os("--no-edit")];
    if let Some(m) = mainline {
        a.push(os(format!("-m{m}")));
    }
    a.push(os(oid.to_hex()));
    r.write(&a, None, Opts::default()).map(done)
}

pub fn merge(r: &Runner, rev: &str) -> Result<String, GitError> {
    if rev.starts_with('-') {
        return Err(GitError::new("merge", "invalid revision"));
    }
    r.write(
        &[
            os("merge"),
            os("--no-edit"),
            os("--end-of-options"),
            os(rev),
        ],
        None,
        Opts::default(),
    )
    .map(done)
}

/// `git <op> --abort` (merge, rebase, cherry-pick, revert, am) or
/// `git bisect reset`.
pub fn abort_operation(r: &Runner, op: super::repo::Operation) -> Result<String, GitError> {
    let a = if op == super::repo::Operation::Bisect {
        vec![os("bisect"), os("reset")]
    } else {
        vec![os(op.command()), os("--abort")]
    };
    r.write(&a, None, Opts::default()).map(done)
}

pub fn remote_add(r: &Runner, name: &str, url: &str) -> Result<String, GitError> {
    if name.starts_with('-') || url.starts_with('-') || name.is_empty() || url.is_empty() {
        return Err(GitError::new("remote add", "invalid name or location"));
    }
    r.write(
        &[os("remote"), os("add"), os("--"), os(name), os(url)],
        None,
        Opts::default(),
    )
    .map(done)
}

pub fn remote_remove(r: &Runner, name: &str) -> Result<String, GitError> {
    r.write(
        &[os("remote"), os("remove"), os("--"), os(name)],
        None,
        Opts::default(),
    )
    .map(done)
}

/// Writes data to a user-chosen file without overwriting silently.
pub fn write_new_file(path: &Path, data: &[u8], overwrite: bool) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true);
    if overwrite {
        o.create(true).truncate(true);
    } else {
        o.create_new(true);
    }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    f.write_all(data).map_err(|e| e.to_string())
}

/// Runs git with the terminal handed over (the caller has suspended the
/// TUI). Returns whether git succeeded.
pub fn handoff(r: &Runner, args: &[OsString], banner: Option<&str>) -> Result<bool, GitError> {
    if r.read_only {
        return Err(GitError::new(
            &Runner::describe(args),
            "refused: git-twin runs in --read-only mode",
        ));
    }
    if let Some(b) = banner {
        println!("{b}");
    }
    println!("$ {}", Runner::describe(args));
    let mut c = r.command(
        args,
        Opts {
            interactive: true,
            ..Default::default()
        },
    );
    let st = c
        .status()
        .map_err(|e| GitError::new(&Runner::describe(args), format!("cannot run git: {e}")))?;
    Ok(st.success())
}

/// Runs an arbitrary user command through `sh -c` with the terminal
/// handed over (git gui tools). Only invoked on explicit user action.
pub fn handoff_shell(r: &Runner, cmd: &str, envs: &[(&str, OsString)]) -> Result<bool, GitError> {
    if r.read_only {
        return Err(GitError::new(
            "tool",
            "refused: git-twin runs in --read-only mode",
        ));
    }
    println!("$ {}", sanitize::sanitize(cmd));
    let mut c = std::process::Command::new("sh");
    c.arg("-c").arg(cmd).current_dir(&r.cwd);
    for (k, v) in envs {
        c.env(k, v);
    }
    let st = c
        .status()
        .map_err(|e| GitError::new("sh", format!("cannot run tool: {e}")))?;
    Ok(st.success())
}

pub fn error_text(e: &GitError) -> String {
    clean_message(e.to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_escape() {
        let d = tempfile::tempdir().unwrap();
        assert!(safe_join(d.path(), b"../x").is_err());
        assert!(safe_join(d.path(), b"/etc/passwd").is_err());
        assert!(safe_join(d.path(), b".git/config").is_err());
        assert!(safe_join(d.path(), b"").is_err());
        assert_eq!(safe_join(d.path(), b"a").unwrap(), d.path().join("a"));
        std::os::unix::fs::symlink("/tmp", d.path().join("link")).unwrap();
        assert!(safe_join(d.path(), b"link/x").is_err());
    }
}
