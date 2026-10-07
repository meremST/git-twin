//! User configuration (TOML) and persistent UI state, stored under the XDG
//! directories. Nothing is ever written inside a repository.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::log::ViewSpec;

pub const APP: &str = "git-twin";

fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() && Path::new(&v).is_absolute() => Some(PathBuf::from(v).join(APP)),
        _ => std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(fallback).join(APP)),
    }
}

pub fn config_dir() -> Option<PathBuf> {
    xdg("XDG_CONFIG_HOME", ".config")
}
pub fn state_dir() -> Option<PathBuf> {
    xdg("XDG_STATE_HOME", ".local/state")
}
pub fn cache_dir() -> Option<PathBuf> {
    xdg("XDG_CACHE_HOME", ".cache")
}
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}
pub fn state_file() -> Option<PathBuf> {
    state_dir().map(|d| d.join("state.toml"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
    /// Uses the terminal's own default colors and 16-color palette instead of
    /// a fixed one, so the UI follows whatever scheme the user's terminal applies.
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum IconMode {
    /// Unicode box drawing and symbols (no Nerd Font needed).
    #[default]
    Unicode,
    /// Nerd Font glyphs.
    Nerd,
    /// Plain ASCII only.
    Ascii,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Detect from COLORTERM / TERM.
    #[default]
    Auto,
    Truecolor,
    #[serde(rename = "256")]
    #[value(name = "256")]
    C256,
    #[serde(rename = "16")]
    #[value(name = "16")]
    C16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StageUntracked {
    #[default]
    Ask,
    Yes,
    No,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Handoff {
    /// Hand the terminal over when `commit.gpgsign` is set.
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub theme: ThemeName,
    pub icons: IconMode,
    pub colors: ColorMode,
    pub mouse: bool,
    /// Copy to the system clipboard with OSC 52 (off by default).
    pub osc52: bool,
    /// gitk "Lines of context".
    pub history_context: u32,
    /// git gui `gui.diffcontext`.
    pub worktree_context: u32,
    pub tab_width: u8,
    /// git gui `gui.commitmsgwidth` (visual guide).
    pub commit_width: u16,
    pub stage_untracked: StageUntracked,
    pub commit_handoff: Handoff,
    /// Allow external diff drivers and textconv in previews.
    pub allow_textconv: bool,
    pub double_click_ms: u64,
    /// Ask before quitting while the commit message is not empty.
    pub confirm_quit: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: ThemeName::Dark,
            icons: IconMode::Unicode,
            colors: ColorMode::Auto,
            mouse: true,
            osc52: false,
            history_context: 3,
            worktree_context: 5,
            tab_width: 8,
            commit_width: 75,
            stage_untracked: StageUntracked::Ask,
            commit_handoff: Handoff::Auto,
            allow_textconv: false,
            double_click_ms: 400,
            confirm_quit: true,
        }
    }
}

impl Config {
    pub fn load_from(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Loads the user configuration; errors fall back to defaults.
    pub fn load() -> (Config, Option<String>) {
        match config_file() {
            Some(p) => match Config::load_from(&p) {
                Ok(c) => (c.clamped(), None),
                Err(e) => (Config::default(), Some(e)),
            },
            None => (
                Config::default(),
                Some("no configuration directory (HOME unset)".into()),
            ),
        }
    }

    pub fn clamped(mut self) -> Config {
        self.history_context = self.history_context.min(1000);
        self.worktree_context = self.worktree_context.min(1000);
        self.tab_width = self.tab_width.clamp(1, 16);
        self.commit_width = self.commit_width.clamp(20, 400);
        self.double_click_ms = self.double_click_ms.clamp(100, 2000);
        self
    }

    pub fn save(&self) -> Result<(), String> {
        let p = config_file().ok_or("no configuration directory (HOME unset)")?;
        let s = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(&p, s.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    #[default]
    History,
    Worktree,
}

/// Splitter positions, stored as per-mille ratios.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Splits {
    pub history_top: u16,
    pub history_bottom_left: u16,
    pub history_author_col: u16,
    pub history_date_col: u16,
    pub worktree_left: u16,
    pub worktree_files: u16,
    pub worktree_diff: u16,
}

impl Default for Splits {
    fn default() -> Self {
        Splits {
            history_top: 420,
            history_bottom_left: 680,
            history_author_col: 22,
            history_date_col: 19,
            worktree_left: 300,
            worktree_files: 500,
            worktree_diff: 720,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct State {
    pub last_view: Option<ViewKind>,
    pub splits: Splits,
    /// gitk views marked "Remember this view".
    pub views: Vec<ViewSpec>,
}

impl State {
    pub fn load() -> Result<State, String> {
        let p = state_file().ok_or("no state directory (HOME unset)")?;
        Self::load_from(&p)
    }

    pub fn load_from(path: &Path) -> Result<State, String> {
        match read_optional(path)? {
            Some(s) => toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
            None => Ok(State::default()),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let p = state_file().ok_or("no state directory (HOME unset)")?;
        let s = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(&p, s.as_bytes())
    }
}

/// Writes `data` to `path` atomically (temp file + rename), mode 0600.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("invalid path")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".tmp-")
        .tempfile_in(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    tmp.write_all(data)
        .and_then(|_| tmp.as_file().sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    tmp.persist(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// Commit message drafts, one file per repository, mode 0600.
pub fn draft_path(git_dir: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    git_dir.hash(&mut h);
    state_dir().map(|d| d.join("drafts").join(format!("{:016x}.txt", h.finish())))
}

pub fn save_draft(git_dir: &Path, text: &str) -> Result<(), String> {
    let p = draft_path(git_dir).ok_or("no state directory")?;
    if text.trim().is_empty() {
        return remove_optional(&p);
    }
    atomic_write(&p, text.as_bytes())
}

pub fn load_draft(git_dir: &Path) -> Result<Option<String>, String> {
    let path = draft_path(git_dir).ok_or("no draft directory (HOME unset)")?;
    read_optional(&path)
}

pub fn clear_draft(git_dir: &Path) -> Result<(), String> {
    let p = draft_path(git_dir).ok_or("no draft directory (HOME unset)")?;
    remove_optional(&p)
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn remove_optional(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Creates a private file (0600) for exports when needed.
pub fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_and_unknown_keys() {
        let c = Config::default();
        let s = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
        assert!(toml::from_str::<Config>("bogus = 1").is_err());
        let partial: Config = toml::from_str("theme = \"light\"\nicons = \"ascii\"").unwrap();
        assert_eq!(partial.theme, ThemeName::Light);
        assert_eq!(partial.icons, IconMode::Ascii);
        assert!(partial.mouse);
        let colors: Config = toml::from_str("colors = \"256\"").unwrap();
        assert_eq!(colors.colors, ColorMode::C256);
    }

    #[test]
    fn atomic_write_is_private() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/state.toml");
        atomic_write(&p, b"x = 1").unwrap();
        atomic_write(&p, b"x = 2").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "x = 2");
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn state_roundtrip() {
        let mut s = State {
            last_view: Some(ViewKind::Worktree),
            ..Default::default()
        };
        s.views.push(ViewSpec {
            name: "mine".into(),
            author: "me".into(),
            ..Default::default()
        });
        let t = toml::to_string_pretty(&s).unwrap();
        let back: State = toml::from_str(&t).unwrap();
        assert_eq!(s, back);
    }
}
