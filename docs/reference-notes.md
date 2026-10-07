# Git Twin — Notes de reference pour implementation

Ces notes completent `docs/parity.md`. Les libelles sont anglais car releves dans les sources Tcl installees. Toutes les commandes et conditions ci-dessous sont issues des sources citees.

## gitk 2.43.0

### Sources et version

- Script: `/usr/bin/gitk` (12802 lignes). Pas de `appvers`; `set appname "gitk"`; version effective dans la matrice = version Git/paquet 2.43.0. About: "Gitk - a commit viewer for git" et copyright 2005-2016 (`gitk:3150-3158`, `gitk:12678-12685`).

### Menu tree gitk (Linux)

- **File** (`gitk:2183-2191`)
  - **Update** — accelerator `F5`, proc `updatecommits`. Relit les commits nouveaux.
  - **Reload** — accelerator `Shift-F5`, proc `reloadcommits`. Recharge la vue.
  - **Reread references** — proc `rereadrefs`; relit `git show-ref -d`, `git rev-parse HEAD`, `git symbolic-ref HEAD` (`gitk:1880-1930`, `gitk:11371`).
  - **List references** — accelerator `F2`, proc `showrefs`; dialogue `Tags and heads` avec filtre glob (`gitk:10135-10230`).
  - separator
  - **Start git gui** — command `safe_exec_redirect [list git gui] [list &]` (`gitk:2189`).
  - separator
  - **Quit** — accelerator `Ctrl-Q`, proc `doquit`.
- **Edit** (`gitk:2194-2198`)
  - **Preferences** — proc `doprefs`; pages General, Colors, Fonts.
- **View** (`gitk:2200-2206`)
  - **New view...** — `Shift-F4`, proc `newview 0`.
  - **Edit view...** — `F4`, proc `editview`, disabled until a non-zero view is selected.
  - **Delete view** — proc `delview`, disabled until a non-zero view is selected.
  - separator
  - **All files** — radio `selectedview=0`, command `showview 0`. User views are appended dynamically by `addviewmenu` (`gitk:4556-4564`).
- **Help** (`gitk:2207-2208`)
  - **About gitk** — proc `about`.
  - **Key bindings** — proc `keys`.
- macOS/Aqua conditional: Apple menu gets About, standard Preferences/Quit callbacks are registered, Help keeps Key bindings (`gitk:2212-2224`).

### Layout gitk

- Main window uses a vertical panedwindow `.ctop`. Top frame `.tf` holds a horizontal panedwindow with three synchronized canvases: graph `.canv`, subject/refs `.canv2`, metadata `.canv3`, plus one shared vertical scrollbar (`gitk:2236-2285`).
- Toolbar `.tf.bar`: disabled-looking `SHA1 ID:` label/button becoming `Goto:` when text differs, SHA1 entry width 40, back/forward arrow buttons, `Row` selected number / total count, status label, progressbar (`gitk:2298-2388`).
- Find bar `.tf.lbar`: label `Find`, down/up buttons, literal label `commit`, selector values `containing:`, `touching paths:`, `adding/removing string:`, `changing lines matching:`, entry, match selector `Exact/IgnCase/Regexp`, field selector `All fields/Headline/Comments/Author/Committer` (`gitk:2390-2444`).
- Bottom left `.bleft`: diff/details text with search bar (`Search` button + entry), radios `Diff`, `Old version`, `New version`, spinbox `Lines of context` 0..10000000, checkbox `Ignore space change`, worddiff selector `Line diff/Markup words/Color words` if Git >= 1.7.2 (`gitk:2450-2534`).
- Bottom right `.bright`: radios `Patch` / `Tree`, text file list/tree with scrollbar (`gitk:2536-2581`).

### gitk key bindings

| Keys | Action/proc | Source |
| --- | --- | --- |
| `Ctrl-Q` | Quitte gitk via doquit | gitk:2690, gitk:3188 |
| `Ctrl-W` | Ferme fenetre courante; sur . appelle doquit | gitk:2665, gitk:3189 |
| `Home` | Selectionne premier commit | gitk:2667, gitk:3190 |
| `End` | Selectionne dernier commit | gitk:2668, gitk:3191 |
| `Up / p / k` | Commit precedent | gitk:2669, gitk:2686, gitk:3192 |
| `Down / n / j` | Commit suivant | gitk:2670, gitk:2687, gitk:3193 |
| `Left / z / h` | Reculer dans historique de navigation | gitk:2673, gitk:2688, gitk:3194 |
| `Right / x / l` | Avancer dans historique de navigation | gitk:2674, gitk:2689, gitk:3195 |
| `Ctrl-1..Ctrl-9` | Aller au n-ieme parent du commit courant | gitk:2732-2734, gitk:3196 |
| `PageUp` | Page precedente dans liste commits | gitk:2675, gitk:3197 |
| `PageDown` | Page suivante dans liste commits | gitk:2676, gitk:3198 |
| `Ctrl-Home` | Scroll liste commits au debut | gitk:2677, gitk:3199 |
| `Ctrl-End` | Scroll liste commits a la fin | gitk:2678, gitk:3200 |
| `Ctrl-Up` | Scroll liste commits une ligne haut | gitk:2679, gitk:3201 |
| `Ctrl-Down` | Scroll liste commits une ligne bas | gitk:2680, gitk:3202 |
| `Ctrl-PageUp` | Scroll liste commits une page haut | gitk:2681, gitk:3203 |
| `Ctrl-PageDown` | Scroll liste commits une page bas | gitk:2682, gitk:3204 |
| `Shift-Up` | Trouver hit precedent (later commits) | gitk:2671, gitk:3205 |
| `Shift-Down` | Trouver hit suivant (earlier commits) | gitk:2672, gitk:3206 |
| `Delete / Backspace / b` | Scroll diff vers le haut (page ou fichier precedent pour b selon bind prevfile) | gitk:2683-2695, gitk:3207 |
| `Space` | Scroll diff une page bas | gitk:2685, gitk:3209 |
| `u` | Scroll diff 18 lignes haut | gitk:2696, gitk:3210 |
| `d` | Scroll diff 18 lignes bas | gitk:2695, gitk:3211 |
| `Ctrl-F` | Lance recherche commit | gitk:2691, gitk:3212 |
| `Ctrl-G / Return` | Hit recherche suivant | gitk:2692, gitk:2700, gitk:3213 |
| `g` | Focus champ SHA1/Goto | gitk:2697, gitk:3215 |
| `/ / KP_Divide` | Focus champ Find | gitk:2698-2699, gitk:3216 |
| `?` | Hit recherche precedent | gitk:2701, gitk:3217 |
| `f` | Scroll diff au fichier suivant | gitk:2702, gitk:3218 |
| `Ctrl-S` | Search hit suivant dans diff | gitk:2693, gitk:3219 |
| `Ctrl-R` | Search hit precedent dans diff | gitk:2694, gitk:3220 |
| `Ctrl-plus / Ctrl-equal / Ctrl-KP_Add` | Augmente polices | gitk:2707-2709, gitk:3221 |
| `Ctrl-minus / Ctrl-KP_Subtract` | Diminue polices | gitk:2710-2711, gitk:3223 |
| `F5` | Update | gitk:2703, gitk:3224 |
| `F2` | List references | gitk:2705 |
| `Shift-F4 / F4` | Nouvelle vue / editer vue courante | gitk:2706-2707 |

### gitk mouse/context behavior

- Left click on commit canvases selects a row (`selcanvline`, `selectline`) and fills details/diff. Right click on a commit row opens `.rowctxmenu`; fake local/index rows use `.fakerowmenu` (`gitk:2720`, `gitk:2741-2770`, `gitk:9057-9091`).
- Right click on branch/ref labels opens `.headctxmenu`; remote refs disable rename/remove, current branch disables checkout/remove (`gitk:2772-2780`, `gitk:9990-10004`).
- File list: left click in patch mode highlights and scrolls diff; release in tree mode opens/closes dirs or displays file. Right click opens highlight/external diff/blame/copy path menu; External diff disabled in Tree mode (`gitk:3426-3610`).
- Diff text right click ignores file/hunk separators, resolves file for clicked line, then offers `Show origin of this line` and `Run git gui blame on this line` (`gitk:3643-3659`).
- Mouse wheel: X11 ButtonRelease-4/5 scrolls commit canvases, buttons 6/7 scroll diff horizontally; middle button scans all canvases vertically and current canvas horizontally (`gitk:2639-2662`, `gitk:2809-2820`).

### gitk context menu command details

- Commit row commands (`gitk:2741-2760`): diff against selected, make patch, create tag, copy commit reference (`git show -s --pretty=format:%h ("%s", %ad) --date=short`), write commit to file, create branch, cherry-pick, reset branch, mark/return/find descendant/compare/diff marked/revert.
- Branch/ref menu (`gitk:2772-2779`): checkout (`git checkout`, or `git checkout -b local --track remotes/...`), rename (`git branch -m`), remove (`git branch -D`), copy branch name.
- File menu (`gitk:2781-2790`): file highlights mutate `findstring` and `gdttype`; external diff extracts files with `git show` / worktree / index then launches `extdifftool fileA fileB`; blame parent launches `git gui blame [--line=N] commit file`.
- Diff menu (`gitk:2792-2798`): origin uses `git blame -p -Lline,+1` or `git cat-file blob` piped into blame for index; gui blame uses `git gui blame --line=N commit file`.

### gitk dialogs and fields

- **View definition** (`gitk:4194-4455`): `View Name`; `Remember this view`; `References (space separated list)`; `Branches & tags`; checkboxes `All refs`, `All (local) branches`, `All tags`, `All remote-tracking branches`; commit info regex fields `Author`, `Committer`, `Commit Message`; checkboxes `Matches all Commit Info criteria`, `Matches no Commit Info criteria`; changes fields radio `Fixed String`/`Regular Expression`, `Search string`; date fields `Since`, `Until`; limits `Number to show`, `Number to skip`; misc `Strictly sort by date`, `Mark branch sides`, `Limit to first parent`, `Simple history`; `Additional arguments to git log`; path text area one per line; command field `Command to generate more commits to include`; buttons `OK`, `Apply (F5)`, `Cancel`. Encoding uses `--all`, `--branches`, `--tags`, `--remotes`, `--author=`, `--committer=`, `--grep=`, `--all-match`, `--invert-grep`, `-S`, `--pickaxe-regex`, `--since/--after`, `--until/--before`, `--max-count`, `--skip`, `--date-order`, `--left-right`, `--first-parent`, `--simplify-by-decoration` (`gitk:4214-4242`).
- **Preferences** (`gitk:11643-11852`): General options listed in parity; Colors choose via `tk_chooseColor`; Fonts choose via Tk fontchooser or custom list. `OK` may call `redisplay`, `reselectline`, `rereadrefs`, or show/hide local changes depending changed variables.
- **Generate patch** (`gitk:9331-9415`): From/To IDs/headlines readonly, `Reverse`, output file default `patchN.patch`, `Generate` runs `diffcmd [old new] -p > file &`.
- **Create tag** (`gitk:9419-9485`): ID/headline readonly, `Tag name`, optional `Tag message`; empty tag error, duplicate tag error; commands `git tag tag id` or `git tag -a -m msg tag id`.
- **Write commit to file** (`gitk:9550-9598`): `Command` default `git diff-tree --stdin -p --pretty=email`, output default `commit-<7>`, command `sh -c "echo id | <command>" >file &`.
- **Branch dialog** (`gitk:9608-9749`): create/rename share ID/headline/name layout. Create confirms `Branch '%s' already exists. Overwrite?` and runs `git branch [-f] name id`; rename runs `git branch -m old new`.
- **Reset** (`gitk:9920-9978`): title `Confirm reset`, message `Reset branch %s to %s?`, radio `Soft: Leave working tree and index untouched`, `Mixed: Leave working tree untouched, reset index` default, `Hard: Reset working tree and index
(discard ALL local changes)`, command `git reset --$resettype $rowmenuid`.
- **List references** (`gitk:10135-10230`): title `Tags and heads: <repo>`, filter default `*`; ref types H local head, R remote, T tag, o other. Clicking selects associated id.

### gitk confirmations/errors/edge cases
- No repository: `Cannot find a git repository here.` (`gitk:12598-12602`). Bad args: `Bad arguments to gitk:`; ambiguous file/revision text (`gitk:12637-12658`).
- Detached HEAD disables reset menu: label `Detached head: can't reset` (`gitk:9074-9079`).
- Cherry-pick already contained asks `Commit %s is already included in branch %s -- really re-apply it?`; conflict asks `Cherry-pick failed because of merge conflict.
Do you wish to run git citool to resolve it?` (`gitk:9791-9826`).
- Revert not contained asks `Commit %s is not included in branch %s -- really revert it?`; conflict asks analogous git citool question (`gitk:9854-9883`).
- Stale config temp file warns it prevents saving configuration (`gitk:2918-2932`).

## git-gui 2.43.0

### Sources et version

- Main script `/usr/lib/git-core/git-gui`; modules in `/usr/share/git-gui/lib`. Internal version `set appvers {0.21.0.116.g0730a}` and wrapper `git-gui version 0.21.0.116.g0730a` (`git-gui:3-12`).

### Menu tree git-gui (Linux)

- **Repository** (`git-gui:2730-2817`): `Explore Working Copy` (non-bare, `xdg-open`), `Browse Current Branch's Files`, `Browse Branch Files...`, `Visualize Current Branch's History`, `Visualize All Branch History`, `Database Statistics`, `Compress Database`, `Verify Database`, `Quit` (`Ctrl-Q`). Windows-only `Git Bash` and platform desktop icon entries are absent on Linux.
- **Edit** (`git-gui:2821-2844`, `git-gui:2994-3000`): `Undo` Ctrl-Z, `Redo` Ctrl-Y, Cut/Copy/Paste/Delete, Select All, `Options...`.
- **Branch** (`git-gui:2849-2877`): `Create...` Ctrl-N, `Checkout...` Ctrl-O, `Rename...`, `Delete...`, `Reset...`. Entries are registered in `disable_on_lock`.
- **Commit** (`git-gui:2890-2957`): `Amend Last Commit` Ctrl-E, Rescan F5, Stage To Commit Ctrl-T, Stage Changed Files To Commit Ctrl-I, Unstage Ctrl-U, Revert Changes Ctrl-J, Show Less/More Context Ctrl-/Ctrl=, Sign Off Ctrl-S, Commit Ctrl-Return.
- **Merge** (`git-gui:2962-2972`): `Local Merge...` Ctrl-M, `Abort Merge...`.
- **Remote** (`git-gui:2977-2989`, `lib/remote.tcl:173-276`): static `Add...`, `Push...`, `Delete Branch...`; dynamic cascades inserted at top: `Push to`, `Fetch from`, `Prune from`, `Remove Remote`, plus `All` for fetch/prune when multiple remotes and Git >= 1.6.6.
- **Tools** (`git-gui:3004-3012`, `lib/tools.tcl`): configured tools inserted before separator; tail entries `Add...`, `Remove...`.
- **Help** (`git-gui:3018-3050`): `About Git Gui`, `Online Documentation`, `Show SSH Key`. macOS has an Apple menu About/Preferences hook, absent on Linux.

### Layout git-gui
- Top branch bar shows `Current Branch:` and `current_branch` (`git-gui:3219-3236`).
- Main split: left vertical panedwindow contains `Unstaged Changes` and `Staged Changes (Will Commit)` lists; right/lower pane contains diff header/body and commit area (`git-gui:3239-3444`).
- Commit area buttons: Rescan, Stage Changed, optional Sign Off, Commit/Done, optional Push (`git-gui:3325-3370`). Message header text is derived from `commit_type` (`git-gui:3401-3414`).
- Diff header shows status, `File:`, path (blue underlined, click opens via explorer); right-click path has Copy/Open (`git-gui:3488-3546`).
- Diff body has colored tags for 2-way, 3-way, submodule and conflict marker lines (`git-gui:3569-3616`, `lib/diff.tcl:386-570`).

### git-gui key-binding table

| Keys | Action | Source |
| --- | --- | --- |
| `Ctrl-Q` | Quitter via do_quit | git-gui:3052-3054 |
| `Ctrl-W` | Fermer toplevel ou do_quit pour main window | git-gui:3056-3075 |
| `Ctrl-N` | Ouvrir Create Branch | git-gui:3947-3950 |
| `Ctrl-O` | Ouvrir Checkout Branch | git-gui:3951-3952 |
| `Ctrl-M` | Ouvrir Merge | git-gui:3953-3954 |
| `Ctrl-P` | Push anywhere | git-gui:3957-3959 |
| `F5 / Ctrl-R` | Rescan et force_first_diff | git-gui:3960-3962 |
| `Ctrl-S` | Ajouter Signed-off-by | git-gui:3963-3964 |
| `Ctrl-T` | Toggle stage/unstage selon liste focus | git-gui:3965-3968 |
| `Ctrl-U` | Toggle stage/unstage selon liste focus | git-gui:3969-3970 |
| `Ctrl-J` | Revert selection | git-gui:3971-3972 |
| `Ctrl-I` | Stage all changed files | git-gui:3973-3974 |
| `Ctrl-E` | Toggle Amend Last Commit | git-gui:3975-3976 |
| `Ctrl-minus` | Moins de contexte diff | git-gui:3977-3978 |
| `Ctrl-plus/Ctrl-equal` | Plus de contexte diff | git-gui:3978-3979 |
| `Ctrl-Return / Ctrl-KP_Enter` | Commit | git-gui:3980-3981 |
| `Up/Down dans listes` | Selection/diff precedent/suivant | git-gui:3983-3987 |
| `Alt-1` | Focus Unstaged Changes | git-gui:3991 |
| `Alt-2` | Focus Staged Changes | git-gui:3992 |
| `Alt-3` | Focus diff | git-gui:3993 |
| `Alt-4` | Focus message | git-gui:3994 |
| `Ctrl-T dans message` | Stage selection | git-gui:3902-3905 |
| `Ctrl-U dans message` | Unstage selection | git-gui:3906-3909 |
| `Ctrl-J dans message` | Revert selection | git-gui:3910-3911 |
| `Ctrl-I dans message` | Stage all | git-gui:3912-3914 |
| `Ctrl-X/C/V/A` | Cut/copy/paste/select all message | git-gui:3915-3926 |
| `Ctrl-C/Ctrl-X dans diff` | Copie selection; paste bloque | git-gui:3930-3937 |
| `Ctrl-A dans diff` | Selectionne tout diff | git-gui:3938-3940 |
| `Ctrl-J dans diff` | Revert selection | git-gui:3941-3942 |
| `Arrow keys / h j k l / Ctrl-b / Ctrl-f` | Scroll diff horizontal/vertical/page | git-gui:3943-3945 |

### git-gui file-list click semantics
- `toggle_or_diff click` computes line/column from text index (`git-gui:2440-2450`). If click is on column 0 and y>1, the icon acts as a stage/unstage button; otherwise the row is selected and `show_diff` is called (`git-gui:2492-2523`).
- In **Unstaged Changes**, icon click stages via `update_index --add --remove -z --stdin`; in **Staged Changes**, icon click unstages via `update-index -z --index-info` (`git-gui:2498-2516`, `lib/index.tcl:63-207`).
- Conflicts (`U`) always route through `merge_stage_workdir`; if the diff still contains conflict markers, it asks `File %s seems to have unresolved conflicts, still stage?` (`lib/mergetool.tcl:22-43`).
- Ctrl-click toggles membership in `selected_paths`; Shift-click selects a range from the last click in the same list (`git-gui:2526-2575`).
- Up/Down in lists reuses `toggle_or_diff up/down`; Alt-1/2 focus lists and restore last clicked path or first entry (`git-gui:3983-3994`, `git-gui:2670-2695`).

### git-gui partial staging rules
- Context menu labels are dynamically rewritten by `popup_diff_menu`: `Stage Hunk For Commit` / `Unstage Hunk From Commit`, `Stage Line(s) For Commit` / `Unstage Line(s) From Commit`, plus `Revert Hunk/Line(s)` only for workdir side (`git-gui:3770-3824`).
- Hunk staging/unstaging/reverting uses the original diff header and selected hunk, then `git apply --whitespace=nowarn` with `--cached`, `--reverse --cached`, or `--reverse` (`lib/diff.tcl:573-638`).
- Line/range staging constructs a synthetic hunk. For staging additions after removals, preceding `-` lines are converted to context after the selected `+` block to support staging half of a replacement. It then writes patch to `git apply --whitespace=nowarn` (`lib/diff.tcl:664-878`).
- Partial actions are disabled for 3-way diffs, no current file, untracked `_O`, type changes `?T/T?`, and textconv files (`git-gui:3798-3811`).
- Revert is only enabled for workdir side; undo stores a single `last_revert` patch, not a stack (`lib/diff.tcl:881-908`).

### git-gui dialogs and fields
- **Create Branch** (`lib/branch_create.tcl:17-224`): Branch Name group (`Name:`, `Match Tracking Branch Name`), Starting Revision chooser, Options: `Update Existing Branch:` No/Fast Forward Only/Reset (default ff), `Fetch Tracking Branch` default 1, `Checkout After Creation` default 1. Validates branch name and uses `checkout_op`.
- **Checkout Branch** (`lib/branch_checkout.tcl:10-91`): Revision chooser, `Fetch Tracking Branch` default 1, `Detach From Local Branch` default 0, checkout via `checkout_op`.
- **Rename Branch** (`lib/branch_rename.tcl:10-124`): `Branch:` combobox (default current), `New Name:` default `gui.newbranchtemplate`, validates non-empty, non-existing, `git check-ref-format`, command `git branch -m old new`.
- **Delete Local Branch** (`lib/branch_delete.tcl:10-143`): list local branches excluding current; Delete Only If Merged Into chooser (with `Always (Do not perform merge checks)`); merge check `git merge-base`; unmerged info dialog; Always warns `Recovering deleted branches is difficult.

Delete the selected branches?`; command `git branch -D`.
- **Merge** (`lib/merge.tcl:1-212`): preflight rejects amend, stale repo, uncommitted changes, existing conflicts. Dialog `Merge Into <branch>`, buttons Visualize/Merge/Cancel, Revision To Merge chooser. Writes `.git/FETCH_HEAD`, runs `git merge --strategy=recursive FETCH_HEAD` (Git >=2.5) or older `fmt-merge-msg` flow.
- **Push Branches** (`lib/transport.tcl:120-233`): Source Branches list defaults selecting current branch; Destination Remote/Arbitrary Location; Transfer Options Force/Thin/Include tags defaults 0. Command `git push -v [--thin] [--force] [--tags] dest refs/heads/b:refs/heads/b`; mirror remotes push without selected refs.
- **Add Remote** (`lib/remote_add.tcl:14-183`): Name, Location, Further Action Fetch Immediately / Initialize Remote Repository and Push / Do Nothing Else Now. Adds remote via `git remote add`; optional fetch or remote init/push chain.
- **Delete Branch Remotely** (`lib/remote_branch_delete.tcl:18-280`): repository remote/url, branches list with Rescan, validation Merged Into/Always. Uses `git ls-remote`, merge-base tests, then `git push -v uri :refs/heads/name`.
- **Options** (`lib/option.tcl:91-360`): repo/global columns for user.name, user.email, merge.summary, merge.verbosity -1..5, merge.diffstat, merge.tool, gui.trustmtime, gui.pruneduringfetch, gui.matchtrackingbranch, gui.textconv, gui.fastcopyblame, gui.maxrecentrepo 0..100, gui.copyblamethreshold 20..200, gui.blamehistoryctx 0..300, gui.diffcontext 1..99, gui.diffopts, gui.commitmsgwidth 0..99, gui.newbranchtemplate, gui.encoding, gui.warndetachedcommit, gui.stageuntracked yes/no/ask, gui.displayuntracked, gui.tabsize 1..99, spelling dictionary, global UI/diff fonts. Defaults in `git-gui:890-925`: e.g. diffcontext 5, commitmsgwidth 75, maxfilesdisplayed 5000, stageuntracked ask, displayuntracked true.
- **Tools Add/Remove/Run** (`lib/tools_dlg.tcl`, `lib/tools.tcl`): Add Tool fields Name, Command, Add globally, pre-run dialog with revision/args, no console, needs file. Runtime env: `GIT_GUITOOL`, `FILENAME`, `FILENAMES`, `CUR_BRANCH`, optional `ARGS`, `REVISION`; command runs `sh -c`.
- **Browser** (`lib/browser.tcl`): `git ls-tree -z`; keys Ctrl-Up/Left parent, Up/Down move, Ctrl-Right/Return enter, PageUp/Down; double-click enters; blobs open blame viewer.
- **Blame viewer** (`lib/blame.tcl`): file text plus two blame columns; context menu Copy Commit, Find Text F7, Goto Line Ctrl-G, Encoding, Do Full Copy Detection, Show History Context (gitk), Blame Parent Commit. Runs `git cat-file [--textconv]`, `git blame --incremental --encoding=utf-8`, and parent diff mapping.
- **SSH Key** (`lib/sshkey.tcl`): searches public keys in DSA/ECDSA/ED25519/RSA/identity order; Generate Key runs `echo | ssh-keygen -q -t rsa -f ~/.ssh/id_rsa 2>&1`; Close/Copy To Clipboard.

### git-gui confirmations/errors/edge cases
- Bare repository: normal gui/citool rejects with `Cannot use bare repository:`; browser/blame subcommands enable bare (`git-gui:1285-1306`).
- Detached HEAD: `load_current_branch` sets current_branch `HEAD`; committing warns if `gui.warndetachedcommit` and not rebase merge (`git-gui:703-727`, `lib/commit.tcl:262-279`).
- Initial commit: `repository_state` sets `ct initial` when `git rev-parse --verify HEAD` fails; PARENT falls back to empty tree via `git mktree` (`git-gui:1390-1429`).
- Merge in progress: MERGE_HEAD sets `ct merge`, rescan loads MERGE_MSG/SQUASH/template in priority order (`git-gui:1398-1410`, `git-gui:1458-1476`).
- No staged changes: info dialog `No changes to commit. You must stage at least 1 file before you can commit.` (`lib/commit.tcl:198-209`). Empty message: error with recommended commit message format (`lib/commit.tcl:212-233`).
- Commit empty tree compared to parent: info `No changes to commit. No files were modified by this commit and it was not a merge commit. A rescan will be automatically started now.` (`lib/commit.tcl:361-374`).
- Revert confirmations: tracked `Revert changes in file %s?` / `Revert changes in these %i files?` plus `Any unstaged changes will be permanently lost by the revert.`; untracked `Delete untracked file %s?` / `Delete these %i untracked files?` plus `Files will be permanently deleted.` (`lib/index.tcl:487-568`).
- Stage all untracked: `gui.stageuntracked` no/yes/ask controls `Stage %d untracked files?` (`lib/index.tcl:398-418`).
- Index update failure: Index Error dialog says update failed and rescan will start; `Unlock Index` deletes `.git/index.lock` (`lib/index.tcl:24-62`).
- Merge conflicts: Use Local/Remote/Base confirmation says diff shows only conflicting changes, file will be overwritten, undo only by restarting merge (`lib/mergetool.tcl:4-20`).
- Mergetool: unsupported/deletion/link conflicts have explicit errors; `merge.tool` default is `meld`, per-tool command lines in `lib/mergetool.tcl:118-294`.
- Commit message backup: `.git/GITGUI_BCK` wins over `GITGUI_MSG` if newer; autosaved every 2s (`git-gui:4024-4070`).
