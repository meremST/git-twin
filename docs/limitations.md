# Known limitations of version 1.1.1

- Both views, menus, dialogs and basic interactions are wired up.
  This does not yet amount to a validation of every row of
  `parity.md`. Its 37 previously specified entries are marked
  **not implemented for v1**; existing implemented/verified/adaptation statuses
  are retained. Full reference parity is not promised for v1.
  Deterministic integration scenarios now cover partial staging/discard,
  commit/amend, hook/signing failures, conflicts and continue/abort for merge,
  rebase, cherry-pick, revert and three-way `am`, plus selected recovery cases.
  They do not validate every advanced operation or external environment.
- The multi-pane layout targets 120 x 35. Below that, down to 79 x 24,
  compact mode shows one pane at a time. Click the tabs or use
  Tab to reach the other panes; Worktree actions remain visible.
  Long content (paths, subjects) may be truncated, but not the main
  commands. The minimum separator sizes preserve the controls.
- The editor supports undo, selection, paste and external editing.
  The width guide uses Unicode display columns, follows horizontal
  scrolling and does not replace the characters of the message.
- Dialogs use Tab, Shift-Tab, Enter and Esc. Very long
  forms scroll with the wheel or Alt-PageUp/PageDown/Home/End without
  moving focus. Labels wrap and buttons remain reachable.
- The F2 reference browser is non-modal but fixed in place; only uncovered
  History elements can be clicked. Other dialogs remain modal. Switching to
  Worktree closes the reference browser. Reference category visibility and
  Worktree path filters are session-only; the existing Compact refs preference
  is still saved.
- Worktree filters only change file-list visibility, selection and file
  actions. They do not narrow commits: committing always uses the entire index.
- Ctrl-I/Tab and Ctrl-M/Enter cannot be told apart on all terminals.
  The enhanced keyboard protocol is enabled if the terminal advertises it.
  Buttons and menus are the alternatives; Alt-Enter commits.
- A single click on a file icon stages/unstages. A double-click
  does not add a second mutation. The git gui lists have no specific
  double-click action; it is not a shortcut for staging.
- Fine diff selection (drag or Shift-Left/Right) is only for
  copying. Shift-Up/Down selects Git lines to stage/revert.
  Attempting a mutation with a fine selection is explicitly refused.
  Native terminal selection
  may require Shift when mouse capture is active.
  `--no-mouse` leaves events to the terminal.
- Interactive actions suspend the interface and hand the terminal back to Git
  or the editor. Their status is re-read on return. PTY scenarios exercise
  editor success/failure, a rejecting signer and Ctrl-C during an interactive
  hook, including terminal restoration and draft preservation.
  Successful signing with real keys/pinentry, arbitrary user hooks and remote
  authentication still require validation in their real environments.
- The gitk command that allows generating commits with an arbitrary shell
  is not implemented. Explicitly configured tools and the Git editor
  run only on user action.
- The application asks for confirmation before quitting with a non-empty message
  when `confirm_quit` is enabled. A save failure prevents normal
  closing; the message stays in memory and the save can be retried.
  Errors reading and deleting drafts are also reported.
  The explicit quit-without-saving option discards the in-memory
  message changes.
- The Git 2.25 floor is checked at launch, but only version 2.43 has
  been used for the current trials. macOS, large repositories and performance
  budgets remain to be validated.
- The binary distribution targets Linux x86_64 GNU with glibc >= 2.35;
  the executables are not static. Alpine/musl, ARM and Windows are not
  part of this first distribution. See the
  [installation guide](build-install.md).
- No screenshots of the gitk/git gui graphical applications have been produced.
  The `preview` example shows the real Git Twin rendering via ratatui TestBackend,
  not a static mock-up.

The commands are `gitt` (History) and `git tui` (Worktree, executable `git-tui`).
The package and the XDG paths remain named `git-twin` to keep existing
preferences and drafts.
