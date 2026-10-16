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

As of October 7, 2026: **71 unit tests** (library, spread
across `src/`) and **15 integration tests** (`tests/prototype.rs`), all
passing locally. The integration tests take about 8 s (they create real temporary
repositories and launch pseudo-terminals).

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

## What is not covered yet

See **Next priorities** in `status.md` for the up-to-date list. In short,
dedicated integration scenarios are missing for:

- partial staging/reverting across multiple hunks, `amend`,
  merge conflicts;
- unusual file names (spaces, newlines, non-UTF-8
  bytes, escape sequences) beyond what the
  `domain::sanitize`/`domain::path` tests cover;
- hand-off to the external editor and commit signing;
- recovery after failure in the middle of an operation (concurrent lock, hook
  that rejects, interrupted command);
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
The workflows still need to be run on the official GitHub repository:
a local run alone does not validate the hosted runners.
