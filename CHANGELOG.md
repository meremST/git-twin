# Release notes

## Unreleased

Improvements planned for 1.1; the development package version remains 1.0.1.

### New features

- Independent, case-insensitive path filters in the staged and unstaged
  Worktree lists, with wildcard matching, visible/total counts and clear buttons.
  Selection and staging actions operate on visible files; commits still include
  the entire index.
- History reference labels reserve space for commit subjects, prioritize HEAD
  and show an overflow `+N` browser scoped to the commit. Compact mode counts
  references by category; View > Reference labels hides categories without
  changing the graph, while keeping HEAD identifiable.
- The fixed F2 reference browser leaves visible History elements interactive,
  including selection, scrolling and context menus. Click to move keyboard
  focus, use F2 to return to the global reference browser, and keep confirmation
  dialogs modal. Reference lists refresh after ref changes.

### Fixes

- Preserve annotated-tag line breaks, blank lines and tab indentation without
  allowing terminal control sequences or losing escaped invalid UTF-8 bytes.

## 1.0.1 — 2026-10-28

### Maintenance

- Bump the project version and align installation documentation.

## 1.0.0 — 2026-10-16

First public release of Git Twin. The 0.x entries below describe internal
preparation versions, not earlier public releases.

### Highlights

- Two entry points for the same terminal Git client: `gitt` opens History,
  `git tui` / `git-tui` opens Worktree; F6 switches between them.
- History graph, branches, tags, search, diffs, all-ref and path filtering,
  compact refs and a syntax-highlighted file viewer.
- Stage/unstage by file, hunk or lines; discard with confirmation and undo;
  commit/amend, saved message drafts, stash and conflict-resolution actions.
- Keyboard and mouse navigation, menus, F12 command palette, resizable panes,
  compact layout, configurable themes and optional Nerd Font icons.
- Read-only mode, local `doctor` diagnostics and external editor integration.

### Changes

- History's Patch list follows navigation in Details / Diff: scrolling tracks
  the top visible line, while cursor movement, clicks and search track the
  active line. The highlighted file is kept visible; Tree selection is unchanged.

### Fixes

- Keep "Undo Last Revert" available when applying its patch fails or a
  read-only guard refuses it; clear it only after successful restoration.
- Report a failed external commit-message editor instead of leaving the
  previous success message visible; keep the original message.
- Keep the TUI alive when Ctrl-C or Ctrl-\\ interrupts a terminal handoff,
  so Git/editor failure can be reported and the draft retained.

### Reliability

- Add 22 isolated real-repository regression scenarios for Git mutations,
  failures/recovery and terminal handoffs; see `docs/testing.md`.
- Mark the 37 remaining `specified` parity entries as `not implemented for v1`,
  without changing the statuses of existing implementations.

### Distribution and compatibility

- Linux x86_64 GNU archive containing `gitt`, `git-tui`, documentation,
  project/dependency licenses and an uninstall script, with a SHA-256 checksum.
- Requires glibc >= 2.35, `libgcc_s.so.1`, Git >= 2.25 and an interactive
  UTF-8 terminal (79 x 24 minimum, 120 x 35 recommended).
- Source builds use Rust 1.93.1 and locked dependencies. Distribution is via
  GitHub archives and sources, not crates.io.
- Existing 0.x installations can replace both binaries; the `git-twin`
  configuration, state and draft paths are unchanged.
- Download from the [official release page](https://github.com/meremST/git-twin/releases/tag/v1.0.0)
  and follow the [installation guide](https://github.com/meremST/git-twin/blob/v1.0.0/docs/build-install.md).

### Known limitations

Full gitk/git gui parity is not claimed; 37 parity entries are explicitly
outside v1. No prebuilt macOS, Windows, ARM or musl binaries are provided.
The Git 2.25 minimum is checked at launch but has not been tested directly.
Large-history performance, successful signing with real keys/pinentry,
remote authentication and arbitrary external hooks/tools are not fully
validated. See the [v1 limitations](https://github.com/meremST/git-twin/blob/v1.0.0/docs/limitations.md)
and [test coverage](https://github.com/meremST/git-twin/blob/v1.0.0/docs/testing.md).

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

## 0.1.0 — internal release preparation

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
