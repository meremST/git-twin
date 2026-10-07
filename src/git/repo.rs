//! Repository discovery and state that does not depend on the UI.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::runner::{GitError, Opts, Runner, clean_message, os};

#[derive(Clone, Debug)]
pub struct RepoInfo {
    /// Working tree root; None for bare repositories.
    pub workdir: Option<PathBuf>,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

impl RepoInfo {
    pub fn is_bare(&self) -> bool {
        self.workdir.is_none()
    }

    pub fn cwd(&self) -> &Path {
        self.workdir.as_deref().unwrap_or(&self.git_dir)
    }

    pub fn display_name(&self) -> String {
        let p = self.workdir.as_deref().unwrap_or(&self.git_dir);
        crate::domain::sanitize::display_bytes(p.as_os_str().as_bytes())
    }
}

fn path_from_line(base: &Path, line: &[u8]) -> PathBuf {
    let p = PathBuf::from(std::ffi::OsStr::from_bytes(line));
    if p.is_absolute() { p } else { base.join(p) }
}

/// Discovers the repository containing `start` by asking Git.
/// Never creates a repository.
pub fn discover(start: &Path) -> Result<RepoInfo, GitError> {
    let probe = Runner::new(start.to_path_buf(), true);
    let args: Vec<OsString> = vec![
        os("rev-parse"),
        os("--is-bare-repository"),
        os("--is-inside-work-tree"),
        os("--absolute-git-dir"),
        os("--git-common-dir"),
    ];
    let o = probe.read_opts(&args, Opts::default(), &[0, 128])?;
    if !o.status.success() {
        return Err(GitError {
            command: "git rev-parse".into(),
            code: o.status.code(),
            message: format!(
                "not a Git repository (or not accessible): {}\n{}",
                crate::domain::sanitize::display_bytes(start.as_os_str().as_bytes()),
                clean_message(&o.stderr)
            ),
        });
    }
    let out = o.stdout;
    let lines: Vec<&[u8]> = out.split(|c| *c == b'\n').collect();
    if lines.len() < 4 {
        return Err(GitError::new(
            "git rev-parse",
            "unexpected rev-parse output",
        ));
    }
    let bare = lines[0] == b"true";
    let inside = lines[1] == b"true";
    let git_dir = path_from_line(start, lines[2]);
    let common_dir = path_from_line(start, lines[3]);
    let workdir = if bare {
        None
    } else if inside {
        let top = probe.read(&[os("rev-parse"), os("--show-toplevel")])?;
        let top = top.strip_suffix(b"\n").unwrap_or(&top);
        Some(path_from_line(start, top))
    } else {
        return Err(GitError::new(
            "git rev-parse",
            "inside the .git directory: run gitt or git tui from the working tree",
        ));
    };
    Ok(RepoInfo {
        workdir,
        git_dir,
        common_dir,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    Am,
    CherryPick,
    Revert,
    Bisect,
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::Am => "am",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
            Operation::Bisect => "bisect",
        }
    }

    /// Git sub-command able to continue/abort the operation.
    pub fn command(self) -> &'static str {
        match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::Am => "am",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
            Operation::Bisect => "bisect",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoState {
    pub operations: Vec<Operation>,
    pub index_locked: bool,
}

/// Detects operations in progress from the per-worktree git directory.
pub fn repo_state(info: &RepoInfo) -> RepoState {
    let g = &info.git_dir;
    let mut ops = Vec::new();
    if g.join("rebase-merge").is_dir() {
        ops.push(Operation::Rebase);
    }
    if g.join("rebase-apply").is_dir() {
        if g.join("rebase-apply/applying").exists() {
            ops.push(Operation::Am);
        } else {
            ops.push(Operation::Rebase);
        }
    }
    if g.join("MERGE_HEAD").exists() {
        ops.push(Operation::Merge);
    }
    if g.join("CHERRY_PICK_HEAD").exists() {
        ops.push(Operation::CherryPick);
    }
    if g.join("REVERT_HEAD").exists() {
        ops.push(Operation::Revert);
    }
    if g.join("BISECT_LOG").exists() {
        ops.push(Operation::Bisect);
    }
    RepoState {
        operations: ops,
        index_locked: g.join("index.lock").exists(),
    }
}

/// `git version` parsed as (major, minor, patch).
pub fn git_version() -> Option<(u32, u32, u32, String)> {
    let o = std::process::Command::new("git")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    let v = s.strip_prefix("git version ")?;
    let mut it = v.split(|c: char| !c.is_ascii_digit());
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    Some((a, b, c, s))
}

pub const MIN_GIT: (u32, u32) = (2, 25);

/// Resolves a command-line path (relative to `cwd`, like gitk) into a
/// pathspec relative to the working tree root, which is where Git runs.
pub fn workdir_pathspec(info: &RepoInfo, cwd: &Path, path: &Path) -> Result<String, String> {
    let shown = || crate::domain::sanitize::display_bytes(path.as_os_str().as_bytes());
    let rel = match &info.workdir {
        None if path.is_absolute() => {
            return Err(format!(
                "absolute path not supported in a bare repository: {}",
                shown()
            ));
        }
        None => normalize(path),
        Some(top) => {
            let abs = normalize(&cwd.join(path));
            abs.strip_prefix(top)
                .map(Path::to_path_buf)
                .ok()
                .or_else(|| {
                    let top = top.canonicalize().ok()?;
                    let abs = canonicalize_existing(&abs)?;
                    abs.strip_prefix(&top).map(Path::to_path_buf).ok()
                })
                .ok_or_else(|| format!("path is outside the repository: {}", shown()))?
        }
    };
    let s = rel
        .to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", shown()))?;
    if s.contains('\n') {
        return Err(format!("path contains a newline: {}", shown()));
    }
    Ok(if s.is_empty() { ".".into() } else { s.into() })
}

/// Lexically removes `.` and `..` components (the file may not exist,
/// e.g. it was deleted in a later commit).
fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !p.is_absolute() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Canonicalizes the longest existing ancestor and appends the rest.
fn canonicalize_existing(p: &Path) -> Option<PathBuf> {
    let mut base = p;
    let mut rest = Vec::new();
    loop {
        if let Ok(c) = base.canonicalize() {
            return Some(rest.iter().rev().fold(c, |acc, r| acc.join(r)));
        }
        rest.push(base.file_name()?);
        base = base.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(top: Option<&str>) -> RepoInfo {
        RepoInfo {
            workdir: top.map(PathBuf::from),
            git_dir: PathBuf::from("/r/.git"),
            common_dir: PathBuf::from("/r/.git"),
        }
    }

    #[test]
    fn pathspec_resolution() {
        let r = info(Some("/nonexistent-gitt/repo"));
        let cwd = Path::new("/nonexistent-gitt/repo/src/ui");
        let f = |p: &str| workdir_pathspec(&r, cwd, Path::new(p));
        assert_eq!(f("main.c").unwrap(), "src/ui/main.c");
        assert_eq!(f("../lib/./x.c").unwrap(), "src/lib/x.c");
        assert_eq!(f("../..").unwrap(), ".");
        assert_eq!(f("/nonexistent-gitt/repo/README").unwrap(), "README");
        assert!(f("../../../other").is_err());
        let bare = info(None);
        assert_eq!(
            workdir_pathspec(&bare, cwd, Path::new("a/./b")).unwrap(),
            "a/b"
        );
        assert!(workdir_pathspec(&bare, cwd, Path::new("/a")).is_err());
    }
}
