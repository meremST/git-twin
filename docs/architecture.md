# Architecture

Overview of the prototype's layers, from the lowest (no internal dependency)
to the highest. `src/lib.rs` exposes all of them:
`pub mod app; pub mod config; pub mod domain; pub mod git; pub mod ui;`.

```
main.rs / bin/git-tui.rs   Thin entry points, share cli.rs
cli.rs        CLI (clap), terminal (raw mode, alt screen, mouse),
              event loop, handoff to a subprocess
ui/           ratatui rendering; stacks clickable areas (ui/hit.rs) while drawing
app/          State + behavior, no terminal IO; App::dispatch(Action)
config/       Config / State (TOML), enums (theme, commit_handoff, stage_untracked...)
git/          runner (Command without shell), repo discovery, streamed log, reads, operations
domain/       oid, sanitize, path, commit, refs, status, diff, patch, graph, textmatch
```

Each layer depends only on the layers below it in this diagram. `domain/` is
pure (no IO, no Git dependency); `git/` encapsulates all calls to the `git`
binary (never a shell, always `--` before paths); `app/` never touches the
terminal directly; `ui/` never modifies state, it reads it and pushes
hit-test areas.

## `domain/` — pure logic, unit tested

- `oid`: commit identifier (SHA), hexadecimal and short forms.
- `path`: paths as raw bytes plus a display form (handles non-UTF-8 names and
  control characters).
- `sanitize`: strips control/escape sequences before any display (defense
  against hostile file names or messages).
- `commit`, `refs`: commit/ref models (`GitRef`, `Head`, `RefSet`, `RefKind`).
- `status`: parser for porcelain v2 `-z` (`git status`).
- `diff`: unified diff parser; `patch`: builds partial Forward/Reverse patches
  for staging, unstaging and reverting lines.
- `graph`: lane layout of the commit graph (gitk style).
- `textmatch`: exact / case-insensitive / regex search.

## `git/` — adapter for the `git` binary, unit tested

- `runner`: argument construction only (never a shell string), `--` always
  before paths, hardening of `GIT_*` variables, cancellation and output limits.
- `repo`: repo discovery (`discover`), never creates a repo; minimum version
  `MIN_GIT`.
- `log`: batched stream of `git log`, `ViewSpec` (equivalent of the gitk
  "view": refs, branches/tags/remotes, author/committer/message, dates,
  ordering...).
- `read`: read-only operations — status, diffs (work tree, index, commit),
  tree, blob, refs, search (`find`), blame, commit resolution, annotated tag
  details (`tag_details`), etc.
- `ops`: mutations — staging, patch application, commit, branch, tag, reset,
  merge, cherry-pick, revert, checkout, handoff to a subprocess (hand-off) for
  commands that need a terminal (editor, signing, external merge/diff).

## `app/` — state and behavior, no terminal IO

- `mod.rs`: `App` struct, `dispatch(Action)`, `on_msg`, `on_op`, `confirm`,
  `run_op`, `After::*` (what to do once a mutation is confirmed on the Git
  side: refresh, select a commit, offer a forced deletion...).
- `action.rs`: `Action` enum, one entry per gitk / git gui command, common
  entry point for keyboard, mouse and menus.
- `worker.rs`: background thread pool, `Payload::*` messages, cancellation
  tokens; a single serialized writer for mutations.
- `dialog.rs`: Confirm / Form / Info / Text / List dialogs, `on_key` ->
  `DialogResult`.
- `diffview.rs`: diff panel state (cursor, selection, scrolling, search, hunk
  selection).
- `input.rs`: `LineInput` and `TextArea` (UTF-8 graphemes, wide characters).
- `history.rs` (~1700 lines, gitk behavior): `HistoryState`, loading (log
  stream + pseudo lines for local changes), selection, back/forward
  navigation, SHA1 entry, search (debounced, on a thread), search in the
  diff, Patch/Tree file list, diff options, comparison between two commits,
  annotated tag information (`TagInfoPanel`, `show_tag_info`, `on_tag_info`),
  all gitk context menu actions, `about_text`, `KEYS_HELP`.
- `worktree.rs` (git gui behavior): Unstaged/Staged lists, partial staging,
  commit/amend, synchronization with the history, forms and hand-offs.
- `forms.rs`: form builders (preferences, views, branches, tags, reset...),
  `on_dialog_result`.
- `menu.rs`, `keys.rs`, `mouse.rs`: menu trees, keyboard event priority
  (dialog > menu > view), clickable areas (`HitMap`), drag-and-drop of
  separators and of the diff selection, double-click.

## `ui/` — ratatui rendering

- `mod.rs`: `draw(frame, &mut App)`; History and Worktree layouts, switch to
  a compact tabbed mode below 120 x 35.
- `theme.rs`: `Theme::new(&cfg)`. `Dark` and `Light` modes, with a `Default`
  or `Gruvbox` color scheme (`cfg.color_scheme`) adapted to the mode (fixed
  RGB colors, degraded to 256/16 colors depending on the detected terminal or
  forced by `--colors`), and `Terminal` (uses only `Color::Reset` and the
  named ANSI colors — Red, Green, Blue... — so that the interface follows the
  palette the user configured in their terminal emulator, without imposing
  RGB, except for the dark variants of colored text in the selected commit:
  RGB or 256 colors on a light inverse background, original ANSI colors in
  16-color mode).
- `hit.rs`: `Hit`, `PaneId`, `SplitId`, `HitMap` (mouse hit-testing).
- `overlays.rs`: menus, dialogs, forms.

Separators are per-mille ratios stored in `state.splits`. Below 79 x 24, a
"too small" message is displayed instead of the interface.

## Entry points

- `src/main.rs` -> `gitt` binary (default view: History).
- `src/bin/git-tui.rs` -> `git-tui` binary, also invocable via `git tui`
  (default view: Worktree).
- `src/cli.rs`: shared logic (clap, terminal, event loop, hand-off,
  `doctor`). `--view` overrides the default view of each entry point;
  `--all` (like `gitk --all`) makes the default view cover all refs rather
  than HEAD alone; positional paths (like `gitk path...`) are converted by
  `repo::workdir_pathspec` into paths relative to the root and restrict the
  default view ("Command line").

## Event flow

1. `cli.rs` reads a crossterm event (keyboard/mouse/resize) or a worker
   message (`workers.rx`).
2. `app/keys.rs` or `app/mouse.rs` translates it into an `Action` (or directly
   calls an `App` method for complex gestures such as clicking on a tag).
3. `App::dispatch` or the called method updates the state, and possibly
   launches a background job (`workers.read` / `workers.write`) that returns a
   `Payload` through the channel.
4. `App::on_msg` applies the `Payload` to the state (`on_details`,
   `on_tag_info`, `on_commit_diff`...).
5. `ui::draw` reads the state on each loop iteration and redraws, pushing the
   new clickable areas.

No step blocks the main loop on a Git command: reads go through the worker
pool, mutations through a serialized writer, and operations that need a
terminal (editor, external merge/diff, push with a prompt) suspend the TUI via
`Handoff`.

## History refresh

The unfiltered HEAD view can reuse a fully loaded history. A probe limited to
64 commits detects linear extensions (simple commit, cherry-pick, revert).
Only the new commits are added to the cache; the graph suffix is kept if the
`GraphBuilder` state at the junction is identical. The indexes and positions
of the cut points are shifted without recomputing that suffix. Changes to
local lines use the same junction, without rereading the commits when HEAD is
unchanged.

A reset to an already loaded commit can keep the list if the old HEAD is one
of the base tips retained by the view. Otherwise, as with new merges, longer
extensions, filters or an incompatible graph junction, the application
rereads the history. Shallow repos, grafts and replacement refs disable
reuse: ancestry can change there without any change of identifiers.

During a full reread, the old list stays displayed until the first
replacement batch. Request identifiers reject results that have become
stale. Shift-F5 always forces a full reread and recaptures the base tips; F5
may use the incremental path.

A mutation's success message waits for the active view to refresh: refs, list
and displayed diff. Hidden panels, supplementary metadata and the rest of the
old history may keep loading in the background. In the history, the expected
selection must be available in the new batches before the completion is
announced. Meanwhile, the bar indicates that the view is being refreshed. A
refresh error cancels the deferred success message.
