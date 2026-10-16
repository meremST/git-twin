# Git Twin - Design and implementation specification

## 1. Status and goal

This document is the requirements specification of the application. The commands and
behaviors described form the target contract; the actual state of the prototype
and its limits are recorded in `status.md` and `docs/limitations.md`.

Name: **Git Twin**, command `gitt` (History view), and **Git TUI**, command
`git tui` via the `git-tui` binary (Worktree view). The Cargo package remains `git-twin`.
Check the availability of the name before any public release.

Build a Git client in the terminal, with two views reproducing the
layouts, features and interactions of **gitk** and **git gui**. The aesthetic
is inspired by LazyVim and Nerd Fonts, without copying its shortcuts at the expense
of the interactions of the reference tools.

Priorities, in this order:

1. Repository integrity and correctness of Git operations.
2. Functional and gestural fidelity to the reference tools.
3. Fidelity of panel layout.
4. Fluidity, readability and graphical polish.

This project is not a Lazygit variant. Do not replace the reference layouts
with a dashboard-style interface, a single command palette or a succession of
full-screen views.

## 2. Platforms and explicit limits

- First supported platform: Linux, interactive UTF-8 terminal.
- Validate xterm, a modern terminal such as Kitty or WezTerm, the VS Code
  integrated terminal, then their use under tmux and over SSH.
- macOS and Windows are later extensions, not guaranteed by v1.
- The terminal must have a keyboard and may have a mouse; all
  operations must remain accessible without a mouse.
- Truecolor and Nerd Font are recommended, never mandatory. Provide reduced
  color modes and ASCII icons.
- The font is configured in the local terminal, not loaded by the application.
- A TUI cannot guarantee every gesture of a graphical application: intercepted
  shortcuts, native text selection, clipboard and pointer precision
  depend on the terminal. Document each adaptation.

Layout goal: reference panels visible from 120 x 35
cells. Between 79 x 24 and this threshold, allow explicit hiding of a panel
or its temporary enlargement. Below 79 x 24, display a too-small-size
notice without executing any hidden action. These thresholds must be
confirmed on the prototype and any change recorded.

## 3. Reference and parity matrix

Before coding the Git operations, observe the installed reference versions of
gitk and git gui. Record Git, the versions or revisions of both tools,
the distribution, the locale, the configuration and the associated screenshots. If a
version is not exposed directly, note the package or the source revision.
Do not invent the menus from a model's recollections.

Produce `docs/parity.md`, with one row per command, option, menu, dialog,
gesture and observable behavior. Use the following columns:

| ID | Tool/version | Location | Gesture | Preconditions | Effect and result | TUI equivalent | Test | Status/gap |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GUI-STAGE-FILE | To record | Unstaged list | Gesture to record | Modified file | Index updated | To implement | Temporary repository | To inventory |

Allowed statuses: to inventory, specified, implemented, verified, documented
adaptation, blocked. A missing feature is not a valid adaptation.

Record in particular:

- History: graph, columns, references, navigation, searches and filters,
  view definition, context menus, comparisons, details and preferences.
- Work: staged/unstaged lists, untracked files, diffs, selection,
  staging by file/hunk/line according to the reference's capabilities,
  commit message, amend, rescan and discarding changes.
- Exposed operations: branches, tags, checkout, cherry-pick, reset, remotes,
  fetch/push, conflicts, blame, external tools and configuration dialogs.
  The inventory determines which tool they belong to and their behavior.
- Edge cases: empty repository, detached HEAD, incomplete history, no changes,
  Git operation in progress, error, closing and restoring preferences.

An additional command may be proposed after parity. It must neither replace
a reference feature nor delay the main milestones.
Do not claim full parity until every row has been verified or
a platform adaptation has been explicitly accepted by the requester.

## 4. Interface and interactions

### 4.1 History view

Indicative diagram to be adjusted from the reference screenshots:

```text
+-----------------------------------------------------------------------+
| Menus gitk equivalents                                                 |
+-----------------------------------------------------------------------+
| Graph | Commit subject                    | Author       | Date       |
|        |                                   |              |            |
+-----------------------------------------------------------------------+
| Search / navigation according to reference layout                      |
+----------------------------------------------+------------------------+
| Commit details and diff                      | File list/tree         |
|                                              |                        |
+----------------------------------------------+------------------------+
| Status / progress / non-modal error                                    |
+-----------------------------------------------------------------------+
```

The graph correctly represents parents, merges, branches and references.
Navigation keeps the identity of the selected commit across a refresh.
Searches must not silently restrict themselves to already-loaded rows.
Comparisons clearly display the start and end revisions.
Merge diffs must make explicit the parent or comparison mode chosen.

### 4.2 Work view

```text
+-----------------------------------------------------------------------+
| Menus git gui equivalents                                              |
+---------------------------+-------------------------------------------+
| Unstaged changes          | Diff of the selected file                  |
|                           |                                           |
+---------------------------+                                           |
| Staged changes            |                                           |
|                           |                                           |
+---------------------------+-------------------------------------------+
| Commit actions            | Commit message                             |
|                           |                                           |
+---------------------------+-------------------------------------------+
| Status / branch / operation in progress                                |
+-----------------------------------------------------------------------+
```

Keep the selected context between the lists and the diff. Indicate
distinctly the staged and unstaged changes of the same file.
The message editor supports multiple lines, local undo,
paste and launching an external editor. Do not lose a draft on error.
Do not add a message spell-checker or AI generation in v1.

### 4.3 Mouse and keyboard contract

- Click: focus and selection according to the component and the reference behavior.
- Double-click: reference action when it exists; testable detection of the
  delay and position, without double execution of a mutation.
- Right-click: context menu anchored to the target, content depending on its state.
- Wheel: scrolling of the hovered panel, without implicit change of selection.
- Dragging a separator: resizing bounded by the minimum sizes.
- Text selection in the diff and the message, with explicit copy.
- Menus accessible by keyboard; visible focus; escape closes the current dialog
  or menu; focus restored after closing.
- Keep the reference shortcuts when the terminal distinguishes them;
  document conflicts and provide a configurable alternative.
- The same application action is used by keyboard, mouse and menu.

Clickable areas come from the rectangles computed for rendering. Do not
duplicate coordinates in the event handlers. Handle Unicode,
combining characters, wide glyphs and horizontal scrolling in the computations.
Diff lines are unwrapped by default to make selection reliable.

Provide accessible help, a mouse capture option and profiles without
Nerd Font. Remote clipboard support via OSC 52 is optional and
disabled by default; never transmit content automatically.

### 4.4 Visual direction

Dark theme inspired by development interfaces, with a light theme provided.
Consistent semantic colors for additions, deletions, references, focus and
errors. Always add a textual indicator to important states.
No decorative animation, blinking or marketing landing page.
Icons are chosen for their meaning; the ASCII version remains functional.
Size and focus preferences must not make the view unusable
after a change of terminal or dimensions.

## 5. Technical choices

- Rust, edition 2024, Ratatui for rendering and Crossterm for events.
- Git CLI as the engine; no libgit2 and no reimplementation of Git objects.
- `clap` for the CLI, `serde` and TOML for configuration.
- Worker threads and bounded channels for Git tasks; no additional async
  runtime unless a measured need justifies it.
- `tempfile` for test repositories; Ratatui buffer tests and PTY scenarios
  for rendering and terminal interactions.
- A single Cargo package at the start. Split into several crates only on real need.

At the initial milestone, choose compatible versions, pin an exact version
of stable Rust in `rust-toolchain.toml` and fill in `rust-version` in
`Cargo.toml`. Commit `Cargo.lock` and use `--locked` in CI and in the build
procedures. The minimum Git is determined by the commands used,
documented and tested; do not announce an unverified minimum.

## 6. Architecture and execution

Target structure, to be created in a new dedicated repository:

```text
git-twin/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  README.md
  LICENSE
  src/
    main.rs
    app/          # States, actions, orchestration, scheduling
    domain/       # Git identities, selections, revisions, results
    git/          # Processes, parsers, reads and mutations
    ui/           # Layouts, widgets, mouse, themes
    config/       # Loading and validation of preferences
  tests/          # Git integration, fixtures and PTY scenarios
  docs/
    parity.md
    architecture.md
    build-install.md
    testing.md
    limitations.md
  .github/workflows/ci.yml
```

Choose the license with the requester before distribution. Do not copy code
from gitk/git gui, themes or assets without checking their licenses.

The domain does not depend on the terminal. The Git adapter does not depend on widgets.
State transitions are explicit and testable. Avoid monolithic functions
mixing drawing, Git parsing and repository modification.

The main loop handles events and rendering, with no blocking Git command.
Asynchronous responses carry a request identifier and a repository
generation: ignore those made obsolete by a new selection.
Limit the concurrency of reads and serialize the application's writes.

An external Git process may modify the repository concurrently. File events
and periodic refresh are hints, not a guarantee of
consistency. Revalidate the state before a mutation and re-read the state after it.

Closing stops the reads in progress and restores the terminal. A mutation
in progress is not killed automatically: present its state and ask for an
appropriate decision. Restore the terminal mode even after an error or panic.

## 7. Git integration and security

### 7.1 Reads and rendering

- Use `std::process::Command` with separate arguments, without a shell built
  from file names, references or messages.
- Favor machine formats, notably `status --porcelain=v2 -z` and
  NUL-terminated path lists. Disable Git's pager and colors for
  parsed output. Do not parse localized error messages.
- Preserve non-UTF-8 paths on Linux; their escaped display must
  never become the identity used for a Git command.
- Distinguish arguments, revisions and paths; use literal pathspecs
  when necessary. `--` alone does not neutralize Git pathspec syntax.
- Neutralize control sequences present in names and messages
  before terminal rendering. No repository content may inject escapes.
- Handle repository roots, worktrees and bare repositories by querying Git,
  without assuming `.git` is a directory. In bare, disable the Work view.
- Disable external diff drivers and textconv by default in preview
  reads; enabling them must be explicit and documented.

The graph is built from parent relationships and a defined order,
not from a colored `git log --graph` output. Test octopus merges and
parents outside the filter. Progressive loading must preserve the graph
lanes and the junctions between batches, without arbitrary visual renumbering.

### 7.2 Mutations

- Use the standard Git commands to respect hooks, identity, signatures,
  configuration, attributes and authentication mechanisms.
- Never automatically disable a hook, a signature or a lock.
- A hook or signature error leaves the commit message available.
- Before discarding changes, destructive reset, deletion or force push,
  show exactly the target and the consequences, then ask for confirmation.
- Do not promise universal undo: reflog and inverse operations do not
  recover all uncommitted data.
- Never automatically delete an `index.lock` file nor modify the
  `safe.directory` checks. Explain the diagnosis and the manual choices.
- Respect operations in progress (merge, rebase, cherry-pick, revert) and
  present the continue/abort actions only when they apply.

To partially stage/unstage, produce a structured patch matching
the correct base (index, working tree or HEAD), including the sign and
context of the lines. Verify its application before mutation and handle the actual
rejection without retrying on other content. A prior check does not remove
races with external processes.

Test in particular new/deleted files, lines without end of line,
CRLF, executable modes, symbolic links and partially staged files.
Binaries, ambiguous renames and unmerged entries must offer
only the actions actually supported, with a visible reason.

### 7.3 Interactive subprocesses and sensitive data

Editors, mergetools, interactive hooks, SSH and signatures may request
the terminal. Provide a handoff mode: suspend rendering and capture,
restore the terminal, run the subprocess with the appropriate I/O, then
re-enable the TUI and refresh. Test the return after failure as after success.

Do not collect a password in a homemade TUI form. Respect Git
agents and credential helpers. The network is used only on an explicit
user action. No telemetry or built-in AI generation by default.
Mask credentials present in URLs; do not log messages,
patches, tokens or sensitive environment variables by default.

## 8. CLI and configuration to implement

```sh
gitt --help
gitt --version
gitt --repo /path/to/repo
gitt --repo /path/to/repo --view history
git tui --repo /path/to/repo
gitt --repo /path/to/repo --no-mouse --icons ascii
git tui --repo /path/to/repo --read-only
gitt doctor
```

Without `--repo`, discover the repository from the current directory. Without `--view`,
open History with `gitt`, Worktree with `git tui` (History for a bare repository).
An explicit `--view` option overrides this choice. Outside a repository,
display a clear error without creating one. `--help`, `--version` and `doctor`
work without a repository or an interactive terminal.

`--read-only` blocks all commands likely to modify the repository,
including fetch and uncontrolled external tools. Reads in this mode
also avoid Git's optional index updates.

`doctor` reports versions, presence of Git, detectable terminal capabilities
and configuration paths, without secrets and without network. It does not claim to
detect the installed font: offer a visual glyph check.

Linux configuration: `$XDG_CONFIG_HOME/git-twin/config.toml`, falling back to
`~/.config/git-twin/config.toml`. State in `$XDG_STATE_HOME/git-twin`, cache in
`$XDG_CACHE_HOME/git-twin`, with the usual XDG fallbacks. No preferences file
created in the user's repository. No automatic loading of an executable configuration
coming from the repository. Atomic writes and readable configuration errors;
priority: CLI, user configuration, default values.

## 9. Build, installation and usage

This section is normative for the future implementation. Reproduce it in
`docs/build-install.md` and provide a quick path in `README.md`.
Replace still-open decisions with versions actually tested.

### 9.1 Linux prerequisites

Required: Git, C compiler/linker, Rust and Cargo managed by rustup.
No need for Node.js, Python, a remote service or root access to run the TUI.
Introduce an additional native dependency only if justified, and
then add the corresponding development packages to this procedure.

Debian/Ubuntu example, to be run by the user:

```sh
sudo apt-get update
sudo apt-get install --yes build-essential git curl ca-certificates
```

Install rustup following <https://rustup.rs/>. To avoid a script executed
directly from a pipe, it is possible to download then inspect:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
less rustup-init.sh
sh rustup-init.sh
. "$HOME/.cargo/env"
rustup --version
git --version
```

Choose a standard user installation. On entering the future repository,
rustup selects the version pinned by `rust-toolchain.toml`. The file must
include the `rustfmt` and `clippy` components. Do not require a nightly toolchain.
Also document the equivalent packages for the other tested distributions.

### 9.2 Fetch the sources and check the environment

The URL of the future repository does not exist yet in this specification. When it is
created, add to the README a real `git clone` command and a tag checkout
for releases. The following commands start from the project root:

```sh
cd /path/to/git-twin
rustc --version
cargo --version
cargo metadata --locked --no-deps --format-version 1
cargo fetch --locked
```

The maintainer initially generates and commits `Cargo.lock`. A user must not
have to regenerate it. Network access is required to obtain the
toolchain and the dependencies, unless a cache or vendored distribution is prepared.

### 9.3 Build and test

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
./target/release/gitt --version
./target/release/git-tui doctor
```

The expected binaries are `target/release/gitt` and `target/release/git-tui`. The release build must
succeed with the default features, without secret variables or private files.
Do not hide warnings to make Clippy pass.

To develop or run from the sources:

```sh
cargo run --locked -- --repo /path/to/repo --view history
cargo run --locked -- --repo /path/to/repo --view worktree
```

These two commands require an interactive terminal. Run them on a
throwaway repository to test mutations. Automated tests must never
use the requester's real working repositories as modifiable fixtures.

Once the toolchain and dependencies are present, also verify:

```sh
cargo build --locked --offline --release
```

### 9.4 Install for the user

Recommended method, from the sources root:

```sh
cargo install --path . --locked --root "$HOME"
export PATH="$HOME/bin:$PATH"
gitt --version
git tui doctor
```

Both binaries are placed in `~/bin` thanks to `--root "$HOME"`.
Keep adding `~/bin` to the `PATH` in the shell configuration.
No `sudo cargo install`.

Alternative without Cargo install management, after a release build:

```sh
install -d "$HOME/bin"
install -m 0755 target/release/gitt "$HOME/bin/gitt"
install -m 0755 target/release/git-tui "$HOME/bin/git-tui"
"$HOME/bin/gitt" --version
```

Add `~/bin` to the shell `PATH` if necessary. Choose a single method
to avoid launching an old copy. Check resolution with
`command -v gitt` / `command -v git-tui` and the version with `gitt --version`.

### 9.5 Update and uninstall

To update from the sources, fetch a published revision, re-run
the checks of section 9.3, then:

```sh
cargo install --path . --locked --root "$HOME" --force
gitt --version
```

For a manual installation, rebuild and repeat the `install` command.
To uninstall an installation managed by Cargo, from the sources or
the extracted archive:

```sh
sh scripts/uninstall.sh --cargo
```

For a manual installation in the path indicated above:

```sh
sh scripts/uninstall.sh
```

The `--cargo` mode also updates the Cargo metadata; the mode without options
removes only the two executables from `~/bin`. Do not use `sudo`.
Keep configuration, state and cache by default; document their paths
for voluntary cleanup. Never delete a user's Git repository.

### 9.6 Releases and troubleshooting

The first release targets Linux x86_64. Document build distribution,
architecture, libc requirements, tested Rust/Git versions and dynamic dependencies.
Provide a versioned archive containing binary, license and instructions, with
a SHA-256 checksum. Do not promise a static or portable binary without testing.
Add aarch64 only after compilation and execution are validated on that target.

| Problem | Check / resolution |
| --- | --- |
| `cargo` not found | Load the rustup environment and check `PATH` |
| `cc` linker missing | Install the system compiler/linker, including `build-essential` on Debian |
| Download failure | Check network, proxy and certificates; do not disable TLS |
| Incompatible lockfile | Use the pinned toolchain and the full revision; do not delete the lockfile |
| Incorrect glyphs | Configure a local Nerd Font or use `--icons ascii` |
| Mouse unavailable | Test outside tmux, check its configuration or use `--no-mouse` |
| Wrong binary launched | Compare `command -v gitt`, `command -v git-tui` and `gitt --version` |
| Terminal badly restored | After the process exits, run `stty sane` or `reset`, then report the bug |
| Git error | Show the context and the redacted diagnostic; do not force the command |

## 10. Test strategy and performance

### 10.1 Mandatory tests

- Unit: parsers, transitions, geometry, clickable areas, graph, Unicode
  width, selection and construction of partial patches.
- Integration: controlled temporary repositories, without hooks or global
  user configuration; local test identity and no network by default.
- Operations: compare HEAD, references, content/mode of index entries and
  working files, not only the message text or the return code.
- Coverage: merges, renames, binaries, spaces, tabs, newlines
  and non-UTF-8 bytes in paths, CRLF, conflicts, worktrees and submodules.
- Errors: concurrent lock, hook rejecting the commit, failing signature,
  interrupted command, repository changed during a selection and lack of permissions.
- Security: malicious terminal sequences, option-like arguments,
  no mutation in read-only mode and no secrets in diagnostics.
- Rendering: buffer captures at fixed sizes, dark/light themes, ASCII/Nerd Font,
  menus near the edges, long dialog, resize and absence of overlap.
- PTY: keyboard/mouse sequences, double-click, drag, focus, handoff to a
  subprocess and terminal restoration, including on error.

CI runs fmt, Clippy, tests, release build and CLI smoke tests on a clean
Linux machine. Deterministic PTY tests must be included in `cargo test`
on Linux or have a separate command that is explicitly documented and run
in CI. Manual graphical checks remain tracked in `docs/testing.md`.

### 10.2 Performance targets

Initial targets, to be measured on a machine and fixtures described in the
test report, without announcing them as achieved:

- First useful display in under 2 s on a local repository of 100,000 commits.
- Visual feedback of a local interaction in under 50 ms at the 95th percentile,
  excluding Git command duration; visible progress for long reads.
- No full loading of diffs or history before the first render.
- Memory bounded by explicit caches; test one million commits and
  very large diffs, without maintaining one widget per repository line.

Measure cold and warm, stating storage, CPU, terminal and Git version.
Large diffs are truncated or loaded progressively with explicit indication,
never silently used as the source of an incomplete partial patch.

## 11. Execution plan for the AI

| Milestone | Work | Exit condition |
| --- | --- | --- |
| J0 - Reference | Inventory, screenshots, matrix, versions, Cargo skeleton and CI | Buildable project, minimal CLI tested, decisions recorded |
| J1 - Prototype | Two layouts on fixtures, mouse, menus, keyboard, resize, themes | Gesture scenarios demonstrated, no Git mutation |
| J2 - History | Read adapter, graph, selection, diffs, search | Correct navigation on a large repository and graph tests passing |
| J3 - Local work | Status, staging, partial patches, commit and amend | Expected Git states verified on fixtures, hooks respected |
| J4 - Parity | All other rows, remote operations, conflicts and external tools | Each inventoried feature has its validated scenario |
| J5 - Delivery | Profiling, terminal matrix, documentation and archive | Clean install and uninstall reproduced, gaps accepted |

For each task, the AI must:

1. Identify the parity row and its success conditions.
2. Read the local implementation and existing tests before modifying.
3. Implement a coherent small slice, without opportunistic refactoring.
4. Run the most targeted test, then the milestone's mandatory checks.
5. Update status, documentation, limitations and validation evidence.
6. Report commands run, actual results and checks not performed.

Do not create the project in an existing firmware repository. If no dedicated
location is provided, ask for it before generating the sources. Do not commit,
push, publish or install on the system without explicit authorization.
Do not use the workspace repositories to experiment with a mutation.

Do not mark a feature done because a button exists. Do not leave
false successes, demo data in the real views or fake destructive
commands. Prototype stubs are removed or isolated in tests.

## 12. Definition of done

v1 is deliverable only when:

- Both views are usable with mouse and keyboard with the reference
  layout, and the matrix contains no unaccepted omission.
- Git operations are tested, their errors recoverable and the destructive
  confirmations precise; no secret is exposed in default traces.
- The terminal is restored after exit and after handoff to external tools.
- The Cargo checks and CI pass; platform limits are published.
- A person or an AI can build, install, launch, update and
  uninstall from a clean Linux machine following only the README
  and `docs/build-install.md`.
- The final report clearly distinguishes what was tested automatically,
  manually, on which environment, and what remains unverified.

The first risk to remove is the fidelity of gestures in a terminal. Validate
J1 before accumulating Git commands: if a fundamental interaction is
impossible, present the limit and have its adaptation explicitly accepted.