# Git Twin — development status

Last update: 2026-10-07, first Linux release preparation.
Spec: `design.md`. Version 0.1.0 is prepared, not published; full parity is
not a release claim.

## TL;DR

| Layer | Path | State |
|---|---|---|
| Domain (pure logic) | `src/domain/` | ✅ compiled, 38 unit tests pass |
| Git adapter (git CLI) | `src/git/` | ✅ compiled, 5 unit tests pass |
| Config | `src/config/mod.rs` | Compiled; config/state unit tests |
| App core | `src/app/{mod,action,dialog,diffview,input,worker,history}.rs` | Compiled and connected to the event loop |
| App interactions | `src/app/{worktree,forms,menu,keys,mouse}.rs` | Implemented; advanced operation scenarios still need coverage |
| UI | `src/ui/{mod,theme,hit,overlays}.rs` | Both views, graph, diffs, message editor, menus, dialogs and hit zones |
| Binaries | `src/main.rs`, `src/bin/git-tui.rs`, shared `src/cli.rs` | `gitt` defaults to History; `git tui` defaults to Worktree; About dialogs updated |
| Integration / PTY tests | `tests/prototype.rs` | Rendering/resizing, stage, read-only, commit, overlays, CLI and PTY smoke tests |
| Docs | README, limitations, parity inventory, architecture, build/install and testing guides | ✅ |
| CI | `.github/workflows/{ci,release}.yml` | Strict checks, Linux archive/install jobs and draft release; hosted run pending |
| Repo | Local Git repository, no remote configured | No tag, push or publication performed |

`src/lib.rs` exports all five layers. The suite now includes the application,
config and UI rather than only domain/Git code.

## Try from source

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build
cargo run --example preview -- --demo
./target/debug/gitt --repo /path/to/repository --read-only
PATH="$PWD/target/debug:$PATH" git tui --repo /path/to/repository
```

The preview builds an isolated temporary repository with branches, a merge,
staged and unstaged changes, then renders both actual 120 x 35 screens.
It does not save preferences or touch existing repositories.

F6 switches views, F10 opens menus, F11 maximizes the focused pane and Ctrl-Q
quits. Startup no longer depends on a cursor-position query: this matters for
PTYs and terminals that do not answer terminal queries.

## Release preparation

- Linux installation guide for prebuilt archives and pinned source builds,
  PATH setup, upgrades, uninstall, runtime compatibility and troubleshooting.
- Installation defaults to `~/bin`; `scripts/uninstall.sh` handles manual
  installs and Cargo installs (`--cargo`), preserving user data. Included in
  release archives and exercised by isolated tests and archive-install CI.
- Linux x86_64 GNU packaging with both launchers, docs, dependency licenses and
  SHA-256. `scripts/package-release.py --tag v0.1.0` rejects version mismatches.
- CI for formatting, strict Clippy, tests, archive building on Ubuntu 22.04,
  and installation on Ubuntu 22.04/24.04. Tag pushes create a draft only after
  all checks pass; publication remains a maintainer decision.
- Rust formatting baseline and compiler-assisted Clippy cleanups, without
  intentional changes to application behavior.
- `CHANGELOG.md` added. crates.io publishing stays disabled; no official
  repository URL is invented while a remote is not configured.
- Pending publication work: configure the official remote, run hosted CI,
  review release notes/licenses, then tag and review the generated draft.

## Earlier completed work (session continuation)

- `.gitignore` added (`/target/`, `*.rs.bk`, `*.pdb`, `.DS_Store`).
- `--all` CLI flag (`gitt --all`, `git tui --all`), equivalent to `gitk --all`:
  sets `all_refs` on the default History view so all branches/tags/remotes
  are included, not only `HEAD`'s ancestry.
- Terminal-following theme (`--theme terminal`, and a Preferences form choice):
  uses only `Color::Reset`/named ANSI colors so the UI follows the terminal's
  own palette instead of a fixed dark/light RGB theme.
- Clicking an annotated or lightweight tag label in the History tree now shows
  "Tag / Tagger / date / message" (or "Lightweight tag (no annotation).") in
  the bottom details pane, like gitk, via a new `git for-each-ref`-based
  `tag_details` read and an async worker payload.
- `LICENSE` file added (Apache-2.0, matching `Cargo.toml`).
- New docs: `docs/architecture.md`, `docs/build-install.md`, `docs/testing.md`.

## Latest completed work (previous session)

- Quit confirmation for nonempty messages; Cancel preserves editing. Failed
  persistence prevents normal exit and keeps the draft dirty for retry.
- Commit-width guide and column counter, with Unicode-aware display widths.
- Compact, tabbed layout below 120 x 35; both layouts preserve access to controls.
- Long forms: wrapped labels, manual scrolling independent of focus, pinned
  wrapping buttons, and correct hit zones for partially visible text areas.
- Fine diff text selection by mouse and Shift-Left/Right, including graphemes
  and horizontal scrolling. Kept separate from whole-line Git patch operations.
- Explicit config/state/draft read, write and removal errors, including
  shutdown errors. No success-shaped fallback for failed saves.
- `gitt` / `git-tui` binaries and `git tui` integration; `--view` still overrides
  the entry point's default. Existing XDG paths are retained.

Current suite: 71 library tests and 14 integration tests, including both
entry points in a PTY, Unicode rendering, persistence failure/recovery and
compact/form interaction checks.

## Next priorities

1. Extend integration coverage to partial staging/reverting, amend, conflicts,
   unusual filenames, editor/signing handoffs and failure recovery, and to the
   new tag-info-on-click flow (currently only exercised by unit tests).
2. Further exercise the new interactions against real repositories and
   terminals, especially external edits and concurrent Git operations.
3. Validate remaining parity entries against the graphical references. Only
   promote a row to verified when its specific behavior is covered.
4. Execute the new workflows on the official GitHub repository before
   publishing the first release; local checks do not replace hosted runs.
5. Benchmark large histories and verify the Git minimum. Validate other
   architectures/platforms before expanding beyond Linux x86_64.

## Environment

- Rust 1.93.1, pinned in `rust-toolchain.toml`. Cargo lives in `~/.cargo/bin`
  (`export PATH=$HOME/.cargo/bin:$PATH`).
- Main crates: ratatui 0.30, crossterm 0.29, clap 4, regex, serde/toml, tempfile,
  unicode-width, unicode-segmentation. Dev crates: portable-pty 0.9, vt100 0.16.
- git 2.43 (minimum supported version `MIN_GIT = (2, 25)`). No Xvfb, so no gitk/git gui screenshots.
- License: Apache-2.0 (see `LICENSE`).

## Architecture (layers, top depends on bottom)

```
main.rs      CLI (clap), terminal setup, event loop, hand-off (suspend TUI)
ui/          ratatui rendering; pushes hit zones (ui/hit.rs) while drawing
app/         state + behaviour (no terminal I/O); App::dispatch(Action)
config/      Config / State (toml), enums (theme, commit_handoff, stage_untracked…)
git/         runner (safe Command, no shell), repo discover, log stream, read, ops
domain/      oid, sanitize, path, commit, refs, status, diff, patch, graph, textmatch
```

### What's done in detail

**domain/** (tested):
- `oid`, `path` (raw bytes plus a display form), `sanitize` (strips control/escape sequences).
- `commit`, `refs`, `status` (porcelain v2 -z parser).
- `diff` (unified diff parser), `patch` (builds partial patches Forward/Reverse for
  stage/unstage/revert lines).
- `graph` (lane layout, gitk style), `textmatch` (exact/ignore case/regexp).

**git/** (tested):
- `runner` (argv only, `--` before paths, `GIT_*` env hardening, cancellation, output limits).
- `repo` (discover, never creates a repo).
- `log` (streaming in batches).
- `read`: status, diffs, tree, blob, refs, find, blame, `resolve_commit`…
- `ops`: stage, unstage, apply_patch, commit, branch, tag, reset, merge, cherry-pick,
  revert, checkout…

**app/** (compiled; original core inventory):
- `mod.rs`: `App` struct, `dispatch`, `on_msg`, `on_op`, `confirm`, `run_op`, `After::*`.
- `action.rs`: `Action` enum for every gitk / git gui command.
- `worker.rs`: background threads, messages `Payload::*`, cancellation tokens.
- `dialog.rs`: Confirm / Form / Info / Text / List dialogs, with `on_key → DialogResult`.
- `diffview.rs`: diff pane state (cursor, selection, scroll, search, hunk selection).
- `input.rs`: `LineInput` and `TextArea` (UTF-8 graphemes, wide chars).
- `history.rs` (gitk behaviour, ~1600 lines):
  - `HistoryState`, loading (log stream + local-changes pseudo rows), selection, back/forward.
  - SHA1 entry, find (debounced; on a thread), diff search, Patch/Tree file list,
    diff options, and compare (diff between two commits).
  - Every gitk context-menu action, `about_text`, and `KEYS_HELP`.
- `config/mod.rs`: Config + persisted State (splits, views, geometry).
- `ui/hit.rs`: `Hit`, `PaneId`, `SplitId`, and `HitMap` (mouse hit-testing).

## Original session-1 implementation checklist (historical)

The checklist below records the original design decisions. Sections 1-5 are
now implemented at prototype level; use **Next priorities** above for current
remaining work. This historical checklist is not a claim that all actions have
been verified.

### 1. `src/app/worktree.rs` (git gui behaviour)

`app/mod.rs` expects:
- `WorktreeState::new(&cfg)`.
- Fields: `message: TextArea`, `branch_label: String`, `last_revert: Option<Vec<u8>>`,
  `amend: bool`, `saved_message: Option<String>`, `focus: WFocus {Unstaged, Staged, Diff, Message}`,
  `diff: DiffView`, `loading: bool`.
- Methods: `cycle_focus(dir)`, and `cursor_file()` returning an item with a `display: String` field.
- `App` methods: `rescan()`, `on_status(Result<WtSnapshot, GitError>)`,
  `on_wt_diff(Result<Diff, GitError>)`, `on_amend_message(Result<String, GitError>)`,
  `dispatch_worktree(&Action) -> bool`.

Design:
- **Lists and diff**
  - `WFile {path, display, kind, x/y status, untracked, unmerged, typechange}`.
  - Two lists (Unstaged / Staged), each with its own set of selected paths, plus a cursor/anchor,
    `last_list`, and a `top` per list.
  - The diff is loaded for the file under the cursor:
    - Unstaged: `untracked_diff` or `worktree_diff(false, …)`.
    - Staged: `worktree_diff(true, base, …)`, where `base` is HEAD, or HEAD^ in amend mode
      (the list then comes from `WtSnapshot.amend_staged`).
- **Partial staging**
  - Stage: `patch::build` Forward + `apply_patch(cached)`.
  - Unstage: Reverse + `cached` + `reverse`.
  - Revert lines: Reverse + `apply --reverse` after a confirmation, then `After::Reverted(patch)`.
    Undo applies the same patch forward.
- **Staging and revert commands**
  - Ctrl-T/Ctrl-U toggle staging in the lists.
  - Ctrl-I stages every changed file. Untracked files follow `cfg.stage_untracked`;
    when it is `Ask`, open the `UntrackedStage` form.
  - Staging an unmerged file asks for confirmation if conflict markers are present.
  - Revert asks "Revert changes in file %s?" (`revert_files`) or "Delete untracked file %s?"
    (`delete_untracked`).
- **Commit and amend**
  - Commit refuses with "No changes to commit" (unless a merge is in progress) and refuses an
    empty message.
  - It hands off to the terminal when `cfg.commit_handoff` is Always, or when it is Auto and
    `commit.gpgsign` is set. Otherwise `run_op(After::Committed)`.
  - Amend is refused on an unborn HEAD or during a merge. It saves the current message and
    loads HEAD's message (`Payload::Amend`).
- **History sync**: in `on_status`, if the (unstaged, staged) flags of tracked files differ
  from `hist.local_flags` and the view includes HEAD, reload history. If the selected row is a
  pseudo row, re-request its diff.
- **Dialogs and hand-offs**
  - Forms: GuiCreateBranch, Checkout, Rename, Delete, Merge, Push, AddRemote, DeleteRemoteBranch.
  - Browser: synchronous `tree_paths`; the blob is shown in a dialog.
  - DbStats/DbVerify run as Text read jobs.
  - Terminal hand-offs: DbCompress, Fetch, Push, mergetool, ExploreWorkingCopy (`xdg-open .`).
  - UseVersion: `checkout --ours/--theirs` then `add`. ShowSshKey reads `~/.ssh/*.pub`.
  - guitools: shell hand-off with env `GIT_GUITOOL`, `FILENAME`, `CUR_BRANCH`.

### 2. `src/app/forms.rs`

- `open_preferences()`.
- The history forms that `history.rs` already calls: `open_list_refs`,
  `open_view_editor(Option<usize>)`, `open_make_patch(from, to)`, `open_create_tag(oid)`,
  `open_write_commit(oid)`, `open_create_branch_at(oid)`, `open_reset(oid)`,
  `open_rename_branch(full)`, `open_cherry_pick_merge(oid, nparents)`.
- The worktree forms listed above.
- `App::on_dialog_result`: Confirm → `run_pending` on button 0; Form → submit according to
  `FormKind`; Info → close.

### 3. `src/app/menu.rs`, `keys.rs`, `mouse.rs`

- **menu.rs**
  - `MenuState` and `App::open_menu(i)`.
  - Menu trees built from app state for each view (see `docs/reference-notes.md`).
  - Context menus for commit, ref, file, diff and worktree diff.
- **keys.rs**: `App::on_key`.
  - Priority: dialog > menu > view. F10 or Alt+letter opens menus.
  - gitk keys (letters work outside text fields). F6/Ctrl-Tab switches view; F11 maximizes
    the focused pane; Shift-F10 opens the context menu.
  - In the worktree diff, `s` stages/unstages the hunk or selected lines and `r` reverts.
    Ctrl-Enter and Alt-Enter commit.
  - Add a `confirm_quit` check in `request_quit`.
- **mouse.rs**: `ClickTracker::new(Duration)`, `Drag`, `App::on_mouse`.
  - Left click goes through `HitMap::at`; right click opens a context menu; the wheel scrolls
    via `pane_at`.
  - Dragging moves splits or extends the diff selection.
  - Double-click selects a hunk in a diff and confirms in dialog lists.
  - In git gui file lists, a click on the status icon toggles staging. git gui has **no**
    double-click on its lists: fix the parity row that says otherwise.

### 4. `src/ui/`

- `mod.rs` (`draw(frame, &mut App)`), `theme.rs` (`Theme::new(&cfg)`), and renderers for
  history, worktree, menus, dialogs and the status bar.
- **History layout**:
  - Menu bar with view tabs; commit list (graph+subject │ author │ date, with draggable columns).
  - Toolbar: SHA1 entry, ← →, Row n/total. Then the find row.
  - Bottom area: diff on the left (options: Search, Diff/Old/New, context ±, ignore space,
    word mode) and the Patch/Tree file list on the right; status bar.
- **Worktree layout**:
  - Menu bar, then "Current Branch:".
  - Left column: Unstaged over Staged. Right column: diff.
  - Bottom band: buttons (Rescan, Stage Changed, Sign Off, Push, Commit) plus the message area
    (New Commit/Amend radios, width guide).
- Splits are per-mille ratios stored in `state.splits`. Below 80x24, show a "too small" message.
- Draw directly into the buffer and push hit zones while rendering.
- `App` fields the UI uses: `hits`, `size`, `maximized`, `menu`, `dialogs`, `status`.

### 5. `src/main.rs` + `src/lib.rs`

- `lib.rs` → `pub mod app; pub mod config; pub mod domain; pub mod git; pub mod ui;`
- CLI flags: `--repo`, `--view`, `--read-only`, `--no-mouse`, `--icons`, `--theme`, `--colors`,
  `--version`, plus a `doctor` subcommand. `--help`, `--version` and `doctor` must work
  without a repo or TTY.
- Startup: check git ≥ 2.25, then `discover` the repo (never create one).
- Terminal: raw mode, alternate screen, mouse capture, bracketed paste, and a panic hook that
  restores the terminal.
- Loop: drain `workers.rx` → `on_msg`, `tick`, draw, OSC 52 copy.
- Hand-off: suspend the TUI, run the command, print "Press Enter to return", restore, then
  `after_handoff`.
- Editor: write the message to a tempfile, run `git var GIT_EDITOR`, read the file back.

### 6. Compile & quality (original plan)

- `cargo build`. Expect **many** compile errors at first, since config/app/ui were never
  compiled. Then run `cargo clippy --all-targets -- -D warnings`, `cargo fmt`, `cargo test`.
- To check at runtime: whether `git diff-tree` accepts `--no-relative`, and `range-diff` usage.

### 7. Tests

- Integration tests in `tests/` on temp repos (isolated `HOME`, `GIT_CONFIG_NOSYSTEM=1`, local
  identity). Cover partial staging, hostile file names (newlines, escape sequences, invalid
  UTF-8), refusal in `--read-only`, commit, history and graph.
- PTY smoke test (portable-pty + vt100): launch, display, quit.

### 8. Docs, CI, repo

- `README.md`, `LICENSE` (Apache-2.0), `docs/architecture.md`, `docs/build-install.md`
  (see design.md §9), `docs/testing.md`, `docs/limitations.md`.
- Update the statuses in `docs/parity.md`.
- `.github/workflows/ci.yml`: fmt, clippy, test on Linux/macOS.
- `git init`, then a local commit (no push), with the trailer
  `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`.

## Known gaps and decisions to document (`docs/limitations.md`)

- gitk's "Command to generate more commits" (arbitrary shell) is not implemented, on purpose
  for security.
- External diff and blame use hand-off (`git difftool`), or show their text in a dialog.
- No gitk/git gui screenshots (no Xvfb available).

## Useful references

- `design.md`: §4 UI (l. 86–172), §7 git/security, §8 CLI/config (~l. 302), §9 build/install
  (l. 335–493), §10 tests (l. 494+), §12 definition of done (l. 565).
- `docs/reference-notes.md`:
  - gitk keys: l. 43–80.
  - git gui menus and keys: l. 120–175.
  - Dialogs: l. 176–200.
