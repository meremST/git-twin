# Linux installation and distribution

Git Twin provides two executables: `gitt` (History view) and `git-tui`
(Worktree view, also reachable as `git tui`). Install both.
The first distribution targets Linux x86_64; macOS, Windows and ARM are
not announced as supported.

## 1. Install a binary archive (no Rust)

Runtime prerequisites:

- Linux x86_64, glibc >= 2.35 and the GCC runtime library (`libgcc_s.so.1`).
  The archives are built on Ubuntu 22.04; the CI covers their
  installation on Ubuntu 22.04 and 24.04. Debian 12 also has the required
  libraries, but is not in this matrix.
- Git in `PATH`. The program refuses Git < 2.25; this minimum has not yet
  been validated on a real installation. Local tests use Git 2.43.
- An interactive UTF-8 terminal, at least 79 x 24 cells (120 x 35 recommended).
  No Nerd Font is needed with the default Unicode icons.
  `--icons nerd` (Nerd Fonts v3) adds icons to views, panels,
  buttons, file states and file types, and displays references
  as colored badges; `unicode` and `ascii` remain text-only.

These binaries are **not static**: Alpine Linux (musl), systems
with an old glibc and ARM machines must use a native build,
with no support guarantee until validated on those platforms.

Once the version is published, open the
[official Releases page](https://github.com/meremST/git-twin/releases)
and download both files into the same directory:

```text
git-twin-1.0.1-x86_64-unknown-linux-gnu.tar.gz
git-twin-1.0.1-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Use only releases from `meremST/git-twin`, not a package with the same name.
Draft releases and private repositories are not accessible to the public.

From the download directory:

```sh
sha256sum --check git-twin-1.0.1-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf git-twin-1.0.1-x86_64-unknown-linux-gnu.tar.gz
cd git-twin-1.0.1-x86_64-unknown-linux-gnu
install -d "$HOME/bin"
install -m 0755 gitt git-tui "$HOME/bin/"
export PATH="$HOME/bin:$PATH"
gitt --version
git tui --version
git tui doctor
```

**Stop if the SHA-256 verification fails.** The checksum detects corruption;
it is not a signature of authenticity. Download both
files from the same official release over HTTPS.
The archive includes the `scripts/uninstall.sh` script, the documentation, the project license and the licenses
of the Cargo build/runtime dependencies under `licenses/`.

Make the `PATH` addition permanent in your shell's startup file
(`~/.profile` or `~/.bashrc` for Bash depending on the session, `~/.zshrc` for Zsh):

```sh
export PATH="$HOME/bin:$PATH"
```

Then open a new terminal. No `sudo` is needed for this
installation. Do not mix this method with a Cargo installation.

## 2. Build and install from source

Required: Git, a C compiler/linker, Rust and Cargo managed by `rustup`.
Python is only needed to build a distribution archive,
not to compile or run the application.

Install the system tools with your distribution's package manager:

```sh
# Debian / Ubuntu
sudo apt-get update
sudo apt-get install --yes build-essential git curl ca-certificates

# Fedora (alternative)
sudo dnf install gcc git curl ca-certificates

# Arch Linux (alternative)
sudo pacman -S --needed base-devel git curl ca-certificates
```

Install Rust following <https://rustup.rs/>. To inspect the script before
running it:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
less rustup-init.sh
sh rustup-init.sh
. "$HOME/.cargo/env"
```

Get the sources of the `v1.0.1` tag from the official release (GitHub
source archive or repository clone), then open the directory containing
`Cargo.toml`. Do not copy only `src/`: keep `Cargo.lock` and
`rust-toolchain.toml`. From that directory:

```sh
rustc --version
cargo install --path . --locked --root "$HOME"
export PATH="$HOME/bin:$PATH"
gitt --version
git tui doctor
```

`rustup` automatically selects and installs Rust **1.93.1** with rustfmt
and Clippy. No nightly. The first download of the toolchain and of the
dependencies requires network access; `--locked` keeps the versions in
`Cargo.lock`. Never run Cargo with `sudo`.

The `--root "$HOME"` option installs both commands to `~/bin/gitt` and
`~/bin/git-tui`: Cargo itself appends the `bin` subdirectory to the root.
It moves neither Rust nor its dependency cache. Cargo keeps its installation
metadata in `~/.crates.toml` and `~/.crates2.json`;
use the same `--root "$HOME"` option to update or uninstall.
Keep the `~/bin` addition to `PATH` in your shell configuration, as
described in section 1.
Cargo install builds are optimized in release mode by default.
`cargo install git-twin` from crates.io is not offered:
`publish = false` is intentional: v1 is distributed through GitHub only.

To build without installing:

```sh
cargo build --locked --release --bins
./target/release/gitt --version
./target/release/git-tui doctor
./target/release/gitt --repo /path/to/repo --read-only
```

## 3. Update and uninstall

For an archive: download the new version and its checksum, verify,
then replace **both** executables using the procedure in section 1.
Close the application before replacing.

For a Cargo installation: get the sources of the new tag, then:

```sh
cargo install --path . --locked --root "$HOME" --force
gitt --version
git tui --version
```

Check which copy is actually used with `command -v gitt` and
`command -v git-tui`. A Git alias `tui` can shadow the subcommand:
inspect `git config --get alias.tui` and run `git-tui` directly if needed.

Close the application, then run the script from the source root or
the extracted archive, depending on the installation method:

```sh
# Cargo installation only
sh scripts/uninstall.sh --cargo

# Manual installation in ~/bin only
sh scripts/uninstall.sh
```

Without options, the script removes only `~/bin/gitt` and `~/bin/git-tui`
(including symbolic links, without removing their target). A command
that is already absent is reported without error; a directory in its place is refused.
Use **`--cargo` for a Cargo-managed installation**: this option
runs `cargo uninstall --root "$HOME" git-twin`, so that its metadata
is also updated. If Cargo is missing or fails, the script reports the error without
falling back to manual removal. Do not use `sudo`.
`sh scripts/uninstall.sh --help` displays the help.

Configuration and drafts are kept:

| Content | Default path | XDG variable |
| --- | --- | --- |
| Preferences | `~/.config/git-twin` | `XDG_CONFIG_HOME` |
| State and drafts | `~/.local/state/git-twin` | `XDG_STATE_HOME` |
| Cache | `~/.cache/git-twin` | `XDG_CACHE_HOME` |

With a non-empty absolute XDG variable, the `git-twin` subdirectory is
used under that variable. Delete this data only deliberately,
after recovering any useful drafts. No Git repository needs to be deleted.

## 4. Prepare a release (maintenance)

The `.github/workflows/ci.yml` workflow runs formatting, strict Clippy,
unit/integration/PTY tests, then builds an archive and installs
both commands on Ubuntu 22.04 and 24.04. Pull requests only have
read permissions on the repository.

Before creating a tag:

1. Update the version in `Cargo.toml` and `Cargo.lock`, the installation
   examples and the matching version section in `CHANGELOG.md` (date, changes
   and limitations). Confirm the date before tagging.
2. Run the checks below, review the licenses and try
   the installed interface on a throwaway repository.
3. Push the release preparation commit to `meremST/git-twin` and require a
   successful hosted CI run on that exact commit. Then create and push an
   annotated tag `v1.0.1` matching the Cargo version exactly.
   Never move a tag that has already been published.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 -m unittest discover -s tests -p 'test_*.py'
python3 scripts/package-release.py --tag v1.0.1
```

Building the archive requires Python >= 3.8 (standard library
only), Linux x86_64 and the pinned toolchain. The script builds with
`--locked --release --target x86_64-unknown-linux-gnu`, checks the versions
of both binaries, collects the licenses of the active dependencies excluding
test dependencies and produces the archive and its checksum under `target/dist/`
(or under Cargo's custom target directory).
A missing license, an inconsistent tag or missing/empty release notes makes
the build fail. `target/dist/RELEASE_NOTES.md` contains only the matching
version's changelog section, not the unreleased or historical entries.
An archive built on a more recent distribution may require a more recent
glibc: **distribute the archive from the Ubuntu 22.04 CI**, not a local
binary of uncontrolled origin.

A `v*` tag push triggers `.github/workflows/release.yml`, which replays
the whole CI. On success, it creates a **draft release**, never an automatic
publication, containing the archive, SHA-256 and version-specific release notes.
The maintainer must review the draft, the limitations and the links before
clicking **Publish release**. A re-run with a draft already created
fails rather than silently replacing existing files.
Source archives are provided by GitHub from the tag.

### First public release checklist

- Review the complete repository history and bundled files for material that
  must not become public, and review the project/dependency licenses.
- The repository was private during v1 preparation. An owner must explicitly
  approve and perform the visibility change in GitHub settings; publishing a
  release in a private repository does not make it publicly accessible.
- After the final commit passes hosted CI, create the tag deliberately:
  `git tag -a v1.0.1 -m "Git Twin v1.0.1"` then
  `git push origin v1.0.1`. These are publication steps, not local checks.
- Review the generated draft and its two assets, confirm it is not marked as
  a prerelease, and publish it as the latest release. Leave the older 0.x
  drafts unpublished.
- From a signed-out browser, verify the repository, release, source archive,
  binary archive, checksum and documentation links are accessible.

## 5. Troubleshooting

| Problem | Check / resolution |
| --- | --- |
| `cargo` not found | Load `. "$HOME/.cargo/env"` and check `PATH` |
| Linker `cc` missing | Install the system compiler/linker |
| `GLIBC_x.y not found` | Use a compatible distribution or build from source; do not manually replace the system glibc |
| `Exec format error` | Check `uname -m`: the archive is for x86_64, not ARM |
| Download failure | Check network, proxy and certificates; do not disable TLS |
| Incompatible lockfile | Use the pinned toolchain and the complete sources; do not delete `Cargo.lock` |
| Incorrect glyphs | Use a UTF-8 terminal or `--icons ascii`; Nerd Fonts are optional |
| Mouse unavailable | Test outside tmux, check its configuration or use `--no-mouse` |
| Wrong binary launched | Compare `command -v gitt`, `command -v git-tui` and their `--version` |
| `git: 'tui' is not a git command` | Add the directory containing `git-tui` to `PATH` |
| `git tui --help` looks for a man page | Git intercepts `--help`; use `git tui -h` or `git-tui --help` (no man page is provided) |
| Terminal badly restored | After the process exits, run `stty sane` or `reset`, then report the bug |
| Git < 2.25 | Update Git; the minimum is checked at startup |

To report a problem, attach the Git Twin version, the distribution,
the architecture, the terminal and the output of `gitt doctor`, after removing
sensitive paths or information. `doctor` works outside a repository,
without an interactive terminal and without any network call.
