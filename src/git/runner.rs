//! Execution of Git subprocesses.
//!
//! Arguments are always passed separately (no shell). Output meant for
//! parsing is produced with pager, colours and external drivers disabled.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::domain::sanitize;

/// Default cap on captured output (diffs, blobs).
pub const DEFAULT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitError {
    pub command: String,
    pub code: Option<i32>,
    pub message: String,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(c) => write!(f, "{} (exit {}): {}", self.command, c, self.message),
            None => write!(f, "{}: {}", self.command, self.message),
        }
    }
}

impl GitError {
    pub fn new(command: &str, message: impl Into<String>) -> GitError {
        GitError {
            command: command.to_string(),
            code: None,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: ExitStatus,
    pub truncated: bool,
}

impl Output {
    pub fn stderr_text(&self) -> String {
        clean_message(&self.stderr)
    }
    pub fn stdout_text(&self) -> String {
        clean_message(&self.stdout)
    }
}

/// Sanitized, credential-free text suitable for display.
pub fn clean_message(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let lines: Vec<String> = s
        .lines()
        .map(|l| sanitize::sanitize(&sanitize::redact_urls(l)))
        .collect();
    let mut out = lines.join("\n").trim().to_string();
    if out.len() > 8000 {
        let mut cut = 8000;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str("\n[...]");
    }
    out
}

pub fn os(s: impl AsRef<OsStr>) -> OsString {
    s.as_ref().to_os_string()
}

#[derive(Clone, Debug)]
pub struct Runner {
    /// Directory where commands run (worktree root, or git dir when bare).
    pub cwd: PathBuf,
    pub read_only: bool,
    /// Allow external diff drivers / textconv in previews.
    pub allow_ext_diff: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Opts {
    pub literal_paths: bool,
    /// Let the child keep the controlling terminal (hand-off mode).
    pub interactive: bool,
    pub limit: Option<usize>,
}

impl Runner {
    pub fn new(cwd: PathBuf, read_only: bool) -> Runner {
        Runner {
            cwd,
            read_only,
            allow_ext_diff: false,
        }
    }

    pub fn describe(args: &[OsString]) -> String {
        let mut s = String::from("git");
        for a in args.iter().take(12) {
            s.push(' ');
            s.push_str(&sanitize::redact_urls(&sanitize::display_bytes(
                std::os::unix::ffi::OsStrExt::as_bytes(a.as_os_str()),
            )));
        }
        if args.len() > 12 {
            s.push_str(" ...");
        }
        s
    }

    /// Base command with safe defaults.
    pub fn command(&self, args: &[OsString], opts: Opts) -> Command {
        let mut c = Command::new("git");
        c.current_dir(&self.cwd);
        c.arg("--no-pager");
        for kv in [
            "color.ui=never",
            "core.quotepath=false",
            "log.showSignature=false",
            "diff.noprefix=false",
            "diff.mnemonicPrefix=false",
        ] {
            c.arg("-c").arg(kv);
        }
        c.args(args);
        c.env("GIT_PAGER", "cat").env("PAGER", "cat");
        if self.read_only {
            c.env("GIT_OPTIONAL_LOCKS", "0");
        }
        if opts.literal_paths {
            c.env("GIT_LITERAL_PATHSPECS", "1");
        } else {
            c.env_remove("GIT_LITERAL_PATHSPECS");
        }
        if !opts.interactive {
            c.env("GIT_TERMINAL_PROMPT", "0");
            // Captured commands must never open an editor on our terminal.
            c.env("GIT_EDITOR", "false");
            c.env("GIT_SEQUENCE_EDITOR", "false");
            // Detach from the controlling terminal so that nothing can
            // read from or write to it behind the TUI.
            unsafe {
                c.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        c
    }

    fn run_inner(
        &self,
        args: &[OsString],
        stdin: Option<&[u8]>,
        opts: Opts,
        cancel: Option<&AtomicBool>,
    ) -> Result<Output, GitError> {
        let desc = Self::describe(args);
        let mut cmd = self.command(args, opts);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = cmd
            .spawn()
            .map_err(|e| GitError::new(&desc, format!("cannot run git: {e}")))?;
        let writer = stdin.map(|data| {
            let data = data.to_vec();
            let mut sin = child.stdin.take();
            std::thread::spawn(move || {
                if let Some(s) = sin.as_mut() {
                    let _ = s.write_all(&data);
                }
            })
        });
        let mut err_pipe = child.stderr.take();
        let err_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(e) = err_pipe.as_mut() {
                let _ = e.take(1 << 20).read_to_end(&mut buf);
                let _ = std::io::copy(e, &mut std::io::sink());
            }
            buf
        });
        let limit = opts.limit.unwrap_or(DEFAULT_LIMIT);
        let mut out = Vec::new();
        let mut truncated = false;
        if let Some(mut so) = child.stdout.take() {
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    let _ = child.kill();
                    break;
                }
                match so.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        if out.len() + n > limit {
                            out.extend_from_slice(&chunk[..limit - out.len()]);
                            truncated = true;
                            let _ = child.kill();
                            break;
                        }
                        out.extend_from_slice(&chunk[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        }
        let status = child
            .wait()
            .map_err(|e| GitError::new(&desc, format!("wait failed: {e}")))?;
        if let Some(w) = writer {
            let _ = w.join();
        }
        let stderr = err_thread.join().unwrap_or_default();
        Ok(Output {
            stdout: out,
            stderr,
            status,
            truncated,
        })
    }

    fn check(desc: String, o: Output, ok: &[i32]) -> Result<Output, GitError> {
        if o.truncated || o.status.code().is_some_and(|c| ok.contains(&c)) {
            Ok(o)
        } else {
            Err(GitError {
                command: desc,
                code: o.status.code(),
                message: if o.stderr.is_empty() {
                    o.stdout_text()
                } else {
                    o.stderr_text()
                },
            })
        }
    }

    /// Read-only command; succeeds only with exit code 0.
    pub fn read(&self, args: &[OsString]) -> Result<Vec<u8>, GitError> {
        self.read_opts(args, Opts::default(), &[0]).map(|o| o.stdout)
    }

    pub fn read_opts(&self, args: &[OsString], opts: Opts, ok: &[i32]) -> Result<Output, GitError> {
        let o = self.run_inner(args, None, opts, None)?;
        Self::check(Self::describe(args), o, ok)
    }

    pub fn read_cancellable(
        &self,
        args: &[OsString],
        opts: Opts,
        cancel: &AtomicBool,
    ) -> Result<Output, GitError> {
        let o = self.run_inner(args, None, opts, Some(cancel))?;
        Self::check(Self::describe(args), o, &[0])
    }

    /// Read-only command with stdin.
    pub fn read_stdin(&self, args: &[OsString], input: &[u8], opts: Opts) -> Result<Output, GitError> {
        let o = self.run_inner(args, Some(input), opts, None)?;
        Self::check(Self::describe(args), o, &[0])
    }

    /// Mutating command. Refused in read-only mode.
    pub fn write(&self, args: &[OsString], stdin: Option<&[u8]>, opts: Opts) -> Result<Output, GitError> {
        let desc = Self::describe(args);
        if self.read_only {
            return Err(GitError::new(&desc, "refused: git-twin runs in --read-only mode"));
        }
        let o = self.run_inner(args, stdin, opts, None)?;
        Self::check(desc, o, &[0])
    }

    /// Spawns a streaming reader (history, find).
    pub fn spawn_stream(&self, args: &[OsString], opts: Opts) -> Result<Child, GitError> {
        let mut cmd = self.command(args, opts);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        cmd.spawn()
            .map_err(|e| GitError::new(&Self::describe(args), format!("cannot run git: {e}")))
    }

    /// Diff-related flags that keep output parseable and safe.
    pub fn diff_safety_args(&self) -> Vec<OsString> {
        let mut v = vec![
            os("--no-color"),
            os("--src-prefix=a/"),
            os("--dst-prefix=b/"),
            os("--no-relative"),
        ];
        if !self.allow_ext_diff {
            v.push(os("--no-ext-diff"));
            v.push(os("--no-textconv"));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_message_redacts_and_sanitizes() {
        let m = clean_message(b"fatal: https://u:p@h/x \x1b[31mred\n");
        assert_eq!(m, "fatal: https://***@h/x ^[[31mred");
    }

    #[test]
    fn describe_is_redacted() {
        let d = Runner::describe(&[os("fetch"), os("https://user:secret@host/r.git")]);
        assert!(!d.contains("secret"));
    }
}
