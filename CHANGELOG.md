# Release notes

## Unreleased

### Fixes

- Keep "Undo Last Revert" available when applying its patch fails or a
  read-only guard refuses it; clear it only after successful restoration.
- Report a failed external commit-message editor instead of leaving the
  previous success message visible; keep the original message.
- Keep the TUI alive when Ctrl-C or Ctrl-\\ interrupts a terminal handoff,
  so Git/editor failure can be reported and the draft retained.

### V1 preparation

- Add 22 isolated real-repository regression scenarios for Git mutations,
  failures/recovery and terminal handoffs; see `docs/testing.md`.
- Mark the 37 remaining `specified` parity entries as `not implemented for v1`,
  without changing the statuses of existing implementations.

## 0.3.1 — 2026-10-15

### New features
- Menus (menu bar and right-click) highlight the entry under the mouse;
  hovering over a submenu opens it.
- The "Compact refs" mode is remembered in the state file (`state.toml`) and
  restored at launch.
- Like gitk, the history keeps the starting tip (HEAD at launch) after a
  checkout, cherry-pick, etc., in addition to the current branch. `F5`
  (Update) keeps this state; `Shift-F5` (Reload) resets it.
- "Compact refs" mode (View menu or checkbox at the bottom right of the
  history): one marker per ref type (`[T]`, `[B]`, `[R]`, or icons in Nerd
  mode); clicking expands the commit's names, clicking the arrow collapses
  them.
- "Messages log" in the Help menu (and therefore in the command palette).

### Changes
- Branch or commit checkouts reload the history to include the new current
  branch.
- `Suppr` in the SHA1 field clears the whole field.

## 0.3.0 — 2026-10-12

Update since the `v0.2.0` tag: command palette, files opened in the editor
and clearer status messages.

### New features

- Command palette on `F12`: fuzzy search of all enabled commands of the
  current view and of the selection's context menu. Recently run commands
  appear first; unavailable commands are greyed out and cannot be run.
- File viewer with syntax highlighting (Rust, Python, JS/TS, Go, C/C++,
  Shell, TOML, YAML, JSON, Markdown, reStructuredText), horizontal and
  vertical scrolling, line numbers.
- "Open in editor" on files in the patch panes (History and Worktree);
  terminal editors (vim, nvim, nano, hx...) take over the terminal during
  editing.
- Status bar: "Ready" or "Working" depending on actual activity, followed by
  the last message. Clicking the message opens the message history.

### Changes

- Explicit operation completion messages (command name and result).
- "Online Documentation" entry removed from the Worktree Help menu.

### Distribution

- `Cargo.toml`, `Cargo.lock` and documentation versions bumped to 0.3.0.
- Expected archive: `git-twin-0.3.0-x86_64-unknown-linux-gnu.tar.gz`.

## 0.2.0 — 2026-10-08

Update since the `v0.1.0` tag: themes, preferences panel, stash handling and
a refined Worktree interface.

### New features

- Right-click on a file (patch pane of History and Worktree): "Open in
  editor". The editor is chosen in the preferences (`editor` in the
  configuration, `code` by default). Terminal editors (vim, nvim, nano,
  hx...) take over the terminal during editing.
- Gruvbox themes added, with a drop-down menu in the preferences; improved
  contrast for the selection and the active tab.
- Reference list available with F2; keyboard shortcut help reviewed.
- History limited to paths passed on the command line (`gitt path`).
- Graph: unified commit node and optional connection lines; improved
  readability on large histories.
- Worktree: commit controls reworked, dialog icons and labels clarified.
- Stash added; checkout process simplified.

### Distribution

- `Cargo.toml`, `Cargo.lock` and documentation versions bumped to 0.2.0.
- Expected archive: `git-twin-0.2.0-x86_64-unknown-linux-gnu.tar.gz`.

### Known limitations

Same as 0.1.0: see [docs/limitations.md](docs/limitations.md).

## 0.1.0 — first release in preparation

First Linux x86_64 distribution of Git Twin, a terminal Git client inspired
by gitk and git gui.

### Features

- Two entry points: `gitt` for History, `git tui` / `git-tui` for Worktree;
  switch between the two views with F6.
- History with graph, branches, tags, search, diffs, `--all` option and path
  filtering (`gitt path/file.c`, like gitk).
- Readable graph on large histories: capped width (extra columns summarized
  by `…`) and, like gitk, long inactive lines cut by clickable bold dotted
  markers `┇` that lead from one to the other (adjustable in Preferences).
- Staging/unstaging per file, hunk or lines, commit message editing and
  draft saving.
- Keyboard, mouse, menus, resizable panes, compact mode, dark/light/terminal
  themes and ASCII/Unicode modes.
- `--read-only` mode, help and local diagnostics with `doctor`.

### Distribution

- Linux x86_64 GNU archive with both executables, documentation, dependency
  licenses and SHA-256 checksum; user installation without Rust.
- Build from source with Rust 1.93.1 and locked dependencies.
- Installation in `~/bin` and an uninstall script for manual or Cargo
  installations, without deleting configuration, drafts or repositories.
- Strict CI (formatting, Clippy, tests), archive building and installation
  verification on Ubuntu 22.04/24.04 before creating a draft release.

### Known limitations

GNU binaries require glibc >= 2.35; no ARM, musl, Windows or macOS binary in
this first distribution. Git >= 2.25 is required at launch, but the minimum
version has not yet been tested directly.
Full parity with gitk/git gui, large histories and advanced scenarios
(conflicts, rebase, signing, hooks and remote transport) are not all
validated end to end. See [docs/limitations.md](docs/limitations.md).
