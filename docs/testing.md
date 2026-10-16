# Tests

State of the test suite, to be completed as the scenarios of the
**Next priorities** section of `status.md` get
covered.

## Running the tests

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --locked
python3 tests/test_uninstall.py
```

As of October 16, 2026: **89 unit tests** (library, spread across `src/`)
and **77 integration tests**: 54 in `tests/prototype.rs`, 1 in
`tests/file_browser.rs`, and 22 in `tests/operations.rs`.
The integration tests create real temporary repositories and launch
pseudo-terminals.

## Unit tests (`src/**/*.rs`, `#[cfg(test)]` modules)

Spread across layers, next to the code they cover:

- `domain/oid.rs`, `domain/path.rs`, `domain/sanitize.rs`: formats, raw/display
  bytes, removal of control/escape sequences.
- `domain/status.rs`: `-z` porcelain v2 parser.
- `domain/diff.rs`, `domain/patch.rs`: unified diff parsing, construction of
  Forward/Reverse partial patches (staging/unstaging/reverting of
  lines).
- `domain/graph.rs`: lane layout of the commit graph.
- `domain/refs.rs`, `domain/textmatch.rs`: refs model, exact/case-insensitive/regex
  search.
- `git/runner.rs`: argument construction (never a shell), `GIT_*`
  hardening.
- `git/log.rs`, `git/ops.rs`: `ViewSpec`/`split_args`, operations.
- `app/mod.rs`, `app/worker.rs`, `app/mouse.rs`, `app/input.rs`,
  `app/diffview.rs`, `app/worktree.rs`: state transitions, worker
  messages, clickable areas, text input, diff pane construction.
- `config/mod.rs`: loading/saving Config/State (TOML), default
  values, explicit errors.
- `ui/hit.rs`: hit-testing of clickable areas.
- `ui/theme.rs`: theme construction in all color modes
  (`color_downgrade`, `themes_build_in_all_modes`,
  `terminal_theme_uses_reset_and_named_colors`).

## Integration tests (`tests/prototype.rs`)

The tests use temporary Git repositories, a local identity and
`GIT_CONFIG_NOSYSTEM=1` / `GIT_CONFIG_GLOBAL=/dev/null` for the setup
commands. PTY launches also isolate the XDG directories.
The scenarios need no network access:

- `real_repository_renders_both_views_and_resizes`: rendering of both views on
  a real repository (branches, merge, staged/unstaged changes), several
  terminal sizes.
- `icon_click_stages_file_and_read_only_refuses_mutations`: click on the status
  icon stages/unstages; `--read-only` refuses any mutation.
- `double_click_does_not_stage_the_next_file_after_refresh`: a double-click
  on a git gui file list does not add a second mutation after
  refresh.
- `commit_keeps_unstaged_changes_and_clears_message`: a simple commit clears
  the message and keeps unstaged changes.
- `menus_and_dialogs_have_topmost_click_targets`: correct stacking of
  clickable areas (dialog > menu > view).
- `cli_commands_do_not_need_a_repository`: `--help`, `--version`, `doctor`
  work outside a repository and outside an interactive terminal.
- `terminal_launch_switch_view_and_clean_exit` /
  `terminal_launch_at_79_columns_switches_view_and_exits` /
  `check_terminal_launcher` /
  `both_launchers_and_about_texts_use_the_new_names`: real launch in a
  pseudo-terminal (portable-pty + vt100) of both `gitt` and
  `git-tui` binaries, view switching (`F6`), clean exit, default view of each
  entry point, sizes 120 x 35, 79 x 24 and 79 x 26, up-to-date "About" texts.
- `compact_panes_keep_controls_accessible_and_return_to_full_layout`: compact
  mode at 79 x 24, 79 x 26 and 80 x 24, tabs, return to the full layout.
- `commit_guide_tracks_display_columns_without_overwriting_text`: message
  width guide, Unicode display columns.
- `long_dialog_manual_scroll_stays_put_and_buttons_remain_clickable`:
  manual scrolling of a long form, buttons still clickable.
- `fine_selection_copies_graphemes_and_never_becomes_a_partial_patch`:
  fine text selection in a diff (copy only, never a partial Git
  mutation).
- `persistence_errors_and_quit_confirmation_are_recoverable`: explicit
  save errors, quit confirmation with a non-empty draft,
  recovery after failure.
- `terminal_commit_selection_preserves_color_families`: selecting a
  commit preserves the terminal theme's color families in the rendering.
- `history_patch_selection_follows_scrolling_and_remains_visible`,
  `history_patch_selection_follows_cursor_mouse_and_search_hits`,
  `history_patch_sync_handles_rebuilds_without_changing_tree_selection`:
  Patch highlighting and list visibility follow scrolling, line navigation,
  clicks, dragging and search; diff rebuilds remain synchronized and Tree
  selection is preserved.

## V1 operation regression scenarios (`tests/operations.rs`)

Run this suite independently with:

```sh
cargo test --locked --test operations
```

Every scenario runs in a child test process with temporary HOME/XDG directories,
isolated Git configuration, a local identity and disabled default hooks/signing.
Tests install their own hooks, editor or rejecting signer when needed.
They require neither network access nor signing keys. Assertions inspect the
actual index, HEAD, file bytes, operation markers, saved drafts and application
errors, not just whether a command returned successfully.

| Scenario family | Covered behavior |
| --- | --- |
| Partial staging | Stage/unstage a hunk, one added line, and selections spanning multiple hunks; preserve all unselected work |
| Discard and undo | Cancel/confirm hunk and line discard; preserve staged work; undo restores exact content |
| Failed undo | External edit prevents applying the undo patch; retain undo information and allow retry |
| Concurrent changes | A stale patch cannot overwrite a changed index or incompatible worktree; refresh after failure |
| Index lock | Report an existing `index.lock`, preserve it and the work, then retry after its owner removes it |
| Commit/amend | Rejecting pre-commit and commit-msg hooks preserve HEAD, index, message, draft and amend mode; successful retry clears the draft |
| Amend history | Root and non-root commits, unchanged parent, unstaging relative to HEAD's parent, preservation of unstaged work and message restoration when leaving amend mode |
| Signing failure | A rejecting signer exercises automatic terminal handoff, failure preservation and retry for commit/amend; a separate PTY scenario checks terminal restoration |
| Conflicts | Merge, rebase, cherry-pick, revert and three-way `am`: refuse unresolved commit/continue, confirm staging conflict markers, stage a resolution and continue, or cancel/confirm abort |
| Merge versions | Use ours/theirs, refuse amend during a merge, and create a two-parent merge commit |
| Paths and bytes | Spaces, tabs, newlines, escape sequences, invalid UTF-8, leading dash and literal pathspec syntax; binary files, missing final newline and renames represented as deletion/addition |
| Truncation | A real diff exceeding the production 16 MiB capture limit refuses partial staging/discard without modifying files or index |
| Destructive actions | Refuse reset when HEAD moved after opening confirmation; confirm whole-file discard and untracked deletion |
| Read-only | Refuse staging, discard, undo, commit, reset, version selection and fetch without consuming undo information or the draft |
| Initial commit | Refuse amend on unborn HEAD and an empty commit message; stage/unstage new files, commit, then stage/unstage/restore a deletion |
| Terminal handoffs | Successful/rejected external editor, rejected signing and Ctrl-C during a waiting commit hook; canonical/raw mode, echo and alternate screen restored, drafts preserved, interrupted commit retry succeeds |

The PTY scenarios launch the real `git-tui` executable and inspect both its
screen and terminal attributes. The signing fixture deliberately rejects:
this does **not** validate successful GPG/SSH signing, a real pinentry or
remote authentication.

## What is not covered yet

See **Next priorities** in `status.md` for the up-to-date list. In short,
dedicated integration scenarios are missing for:

- successful signing with real GPG/SSH keys and pinentry;
- remote authentication and transport failures, complex user hooks and
  external merge tools;
- every possible concurrent modification or crash (the scenarios above cover
  specific stale patches, a moved HEAD, an existing lock and interruption);
- the new tag click flow (`show_tag_info` / `on_tag_info`,
  `src/app/history.rs`) currently has only manual verification and
  coverage from the existing unit tests on the rest of the module; a
  dedicated integration test (annotated and lightweight tag, on a temporary
  repository) remains to be written.

## What cannot be tested here

- No reference gitk/git gui screenshots: neither `Xvfb` nor `xvfb-run` is
  available in this environment (see `docs/limitations.md`).
- No macOS test has been run (no macOS machine available).
- Benchmarks on a large history (section 10.2 of
  `design.md`, target: first useful display in under 2 s on 100,000
  commits) have not been measured yet; to be done before any performance
  announcement.

## CI

`.github/workflows/ci.yml` runs on branches and pull requests, with
the toolchain from `rust-toolchain.toml`, the following checks on Ubuntu 22.04:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 tests/test_uninstall.py
python3 scripts/package-release.py
```

Formatting and strict Clippy are mandatory quality gates and
pass locally, without any global lint disabling.
The script builds both executables in release mode for
`x86_64-unknown-linux-gnu`, checks their versions and builds the archive with
documentation, licenses and SHA-256.

Two install jobs consume **the same archive** on Ubuntu 22.04
and 24.04: SHA-256 verification, extraction, installation into `~/bin`,
`gitt --help/--version`, `git tui -h`, `git-tui --help`,
`git tui --version` and `git tui doctor`.
This check does not use Cargo to install the binaries; the PTY tests
still run from source in the quality job.
The jobs then run the uninstall script shipped in the archive
and verify that both binaries are gone.

`tests/test_uninstall.py` uses only Python and temporary
directories: manual removal, symbolic links, preservation of other
files and of data, missing binaries, rejection of directories and invalid
arguments, forwarding of arguments to Cargo and propagation of its errors.

The `release.yml` workflow reuses the CI before creating a draft release
on a `v*` tag. A tag that differs from the Cargo version is rejected. See the
[release procedure](build-install.md#4-prepare-a-release-maintenance).
The existing workflows have run on the official repository. These new scenarios
are included automatically by `cargo test --locked`; their hosted result must
still be checked after pushing this change.
