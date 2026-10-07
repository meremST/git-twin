# Architecture

Vue d'ensemble des couches du prototype, de la plus basse (sans dependance
interne) a la plus haute. `src/lib.rs` les expose toutes :
`pub mod app; pub mod config; pub mod domain; pub mod git; pub mod ui;`.

```
main.rs / bin/git-tui.rs   Points d'entree minces, partagent cli.rs
cli.rs        CLI (clap), terminal (raw mode, alt screen, souris),
              boucle d'evenements, transfert (handoff) vers un sous-processus
ui/           Rendu ratatui ; empile les zones cliquables (ui/hit.rs) en dessinant
app/          Etat + comportement, sans IO terminal ; App::dispatch(Action)
config/       Config / State (TOML), enums (theme, commit_handoff, stage_untracked...)
git/          runner (Command sans shell), decouverte de depot, log en flux, lectures, operations
domain/       oid, sanitize, path, commit, refs, status, diff, patch, graph, textmatch
```

Chaque couche ne depend que des couches situees en dessous d'elle dans ce
schema. `domain/` est pur (aucun IO, aucune dependance Git) ; `git/` encapsule
tous les appels au binaire `git` (jamais de shell, toujours `--` avant les
chemins) ; `app/` ne touche jamais au terminal directement ; `ui/` ne modifie
jamais l'etat, elle le lit et pousse des zones de hit-test.

## `domain/` — logique pure, testee unitairement

- `oid` : identifiant de commit (SHA), forme hexadecimale et forme courte.
- `path` : chemins en octets bruts plus une forme d'affichage (gere les noms
  non UTF-8 et les caracteres de controle).
- `sanitize` : retire les sequences de controle/echappement avant tout
  affichage (defense contre les noms de fichiers ou messages hostiles).
- `commit`, `refs` : modeles de commit/ref (`GitRef`, `Head`, `RefSet`, `RefKind`).
- `status` : analyseur du porcelain v2 `-z` (`git status`).
- `diff` : analyseur de diff unifie ; `patch` : construit des patchs partiels
  Forward/Reverse pour l'indexation, la desindexation et l'annulation de lignes.
- `graph` : disposition des voies du graphe de commits (style gitk).
- `textmatch` : recherche exacte / insensible a la casse / regex.

## `git/` — adaptateur du binaire `git`, teste unitairement

- `runner` : construction d'arguments uniquement (jamais de chaine shell),
  `--` systematique avant les chemins, durcissement des variables `GIT_*`,
  annulation (cancellation) et limites de sortie.
- `repo` : decouverte du depot (`discover`), ne cree jamais de depot ; version
  minimale `MIN_GIT`.
- `log` : flux par lots de `git log`, `ViewSpec` (equivalent de la "vue" gitk :
  refs, branches/tags/remotes, auteur/committer/message, dates, tri...).
- `read` : lectures seules — status, diffs (travail, index, commit), arbre,
  blob, refs, recherche (`find`), blame, resolution de commit, details de tag
  annote (`tag_details`), etc.
- `ops` : mutations — indexation, application de patch, commit, branche, tag,
  reset, merge, cherry-pick, revert, checkout, transfert vers un
  sous-processus (hand-off) pour les commandes necessitant un terminal
  (editeur, signature, merge/diff externes).

## `app/` — etat et comportement, sans IO terminal

- `mod.rs` : structure `App`, `dispatch(Action)`, `on_msg`, `on_op`, `confirm`,
  `run_op`, `After::*` (ce qu'il faut faire une fois une mutation confirmee cote
  Git : rafraichir, selectionner un commit, offrir une suppression forcee...).
- `action.rs` : enum `Action`, une entree par commande gitk / git gui, point de
  passage commun pour clavier, souris et menus.
- `worker.rs` : pool de threads d'arriere-plan, messages `Payload::*`, jetons
  d'annulation ; un seul writer serialise pour les mutations.
- `dialog.rs` : dialogues Confirm / Form / Info / Text / List, `on_key` ->
  `DialogResult`.
- `diffview.rs` : etat du panneau de diff (curseur, selection, defilement,
  recherche, selection de hunk).
- `input.rs` : `LineInput` et `TextArea` (graphemes UTF-8, caracteres larges).
- `history.rs` (~1700 lignes, comportement gitk) : `HistoryState`, chargement
  (flux de log + lignes pseudo pour les changements locaux), selection,
  navigation arriere/avant, saisie de SHA1, recherche (debattue, sur un
  thread), recherche dans le diff, liste de fichiers Patch/Tree, options de
  diff, comparaison entre deux commits, informations de tag annote
  (`TagInfoPanel`, `show_tag_info`, `on_tag_info`), toutes les actions du menu
  contextuel gitk, `about_text`, `KEYS_HELP`.
- `worktree.rs` (comportement git gui) : listes Unstaged/Staged, indexation
  partielle, commit/amend, synchronisation avec l'historique, formulaires et
  transferts (hand-offs).
- `forms.rs` : constructeurs de formulaires (preferences, vues, branches,
  tags, reset...), `on_dialog_result`.
- `menu.rs`, `keys.rs`, `mouse.rs` : arbres de menus, priorite des evenements
  clavier (dialogue > menu > vue), zones cliquables (`HitMap`), glisser-deposer
  des separateurs et de la selection de diff, double-clic.

## `ui/` — rendu ratatui

- `mod.rs` : `draw(frame, &mut App)` ; dispositions History et Worktree,
  bascule vers un mode compact par onglets en dessous de 120 x 35.
- `theme.rs` : `Theme::new(&cfg)`. Trois palettes : `Dark`, `Light` (couleurs
  RGB fixes, degradees en 256/16 couleurs selon le terminal detecte ou force
  par `--colors`), et `Terminal` (n'utilise que `Color::Reset` et les couleurs
  ANSI nommees — Red, Green, Blue... — pour que l'interface suive la palette
  que l'utilisateur a configuree dans son emulateur de terminal, sans imposer
  de RGB, sauf pour les variantes sombres du texte colore dans le commit
  selectionne : RGB ou 256 couleurs sur fond inverse clair, couleurs ANSI
  d'origine en mode 16 couleurs).
- `hit.rs` : `Hit`, `PaneId`, `SplitId`, `HitMap` (hit-testing souris).
- `overlays.rs` : menus, dialogues, formulaires.

Les separateurs sont des ratios en pour-mille stockes dans `state.splits`.
En dessous de 79 x 24, un message "trop petit" est affiche a la place de
l'interface.

## Points d'entree

- `src/main.rs` -> binaire `gitt` (vue par defaut : History).
- `src/bin/git-tui.rs` -> binaire `git-tui`, invocable aussi via `git tui`
  (vue par defaut : Worktree).
- `src/cli.rs` : logique partagee (clap, terminal, boucle d'evenements,
  hand-off, `doctor`). `--view` remplace la vue par defaut de chaque point
  d'entree ; `--all` (comme `gitk --all`) fait porter la vue par defaut sur
  toutes les refs plutot que sur HEAD seul ; des chemins positionnels (comme
  `gitk chemin...`) sont convertis par `repo::workdir_pathspec` en chemins
  relatifs a la racine et limitent la vue par defaut ("Command line").

## Flux d'un evenement

1. `cli.rs` lit un evenement crossterm (clavier/souris/resize) ou un message
   de worker (`workers.rx`).
2. `app/keys.rs` ou `app/mouse.rs` le traduit en `Action` (ou appelle
   directement une methode d'`App` pour les gestes complexes comme le clic
   sur un tag).
3. `App::dispatch` ou la methode appelee met a jour l'etat, lance eventuellement
   un job en arriere-plan (`workers.read` / `workers.write`) qui renvoie un
   `Payload` via le canal.
4. `App::on_msg` applique le `Payload` a l'etat (`on_details`, `on_tag_info`,
   `on_commit_diff`...).
5. `ui::draw` lit l'etat a chaque tour de boucle et redessine, en poussant les
   nouvelles zones cliquables.

Aucune etape ne bloque la boucle principale sur une commande Git : les lectures
passent par le pool de workers, les mutations par un writer serialise, et les
operations necessitant un terminal (editeur, merge/diff externe, push avec
prompt) suspendent la TUI via `Handoff`.
