# Tests

Etat de la suite de tests, a completer au fur et a mesure
que les scenarios de la section **Next priorities** de `status.md` sont
couverts.

## Lancer les tests

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --locked
python3 tests/test_uninstall.py
```

Au 7 octobre 2026 : **71 tests unitaires** (bibliotheque, repartis
dans `src/`) et **15 tests d'integration** (`tests/prototype.rs`), tous
passants localement. Les tests d'integration prennent environ 8 s (ils creent de vrais
depots temporaires et lancent des pseudo-terminaux).

## Tests unitaires (`src/**/*.rs`, modules `#[cfg(test)]`)

Repartis par couche, pres du code qu'ils couvrent :

- `domain/oid.rs`, `domain/path.rs`, `domain/sanitize.rs` : formats, octets
  bruts/affichage, retrait des sequences de controle/echappement.
- `domain/status.rs` : analyseur du porcelain v2 `-z`.
- `domain/diff.rs`, `domain/patch.rs` : analyse de diff unifie, construction de
  patchs partiels Forward/Reverse (indexation/desindexation/annulation de
  lignes).
- `domain/graph.rs` : disposition des voies du graphe de commits.
- `domain/refs.rs`, `domain/textmatch.rs` : modele de refs, recherche
  exacte/insensible a la casse/regex.
- `git/runner.rs` : construction d'arguments (jamais de shell), durcissement
  `GIT_*`.
- `git/log.rs`, `git/ops.rs` : `ViewSpec`/`split_args`, operations.
- `app/mod.rs`, `app/worker.rs`, `app/mouse.rs`, `app/input.rs`,
  `app/diffview.rs`, `app/worktree.rs` : transitions d'etat, messages de
  worker, zones cliquables, saisie de texte, construction du panneau de diff.
- `config/mod.rs` : chargement/sauvegarde Config/State (TOML), valeurs par
  defaut, erreurs explicites.
- `ui/hit.rs` : hit-testing des zones cliquables.
- `ui/theme.rs` : construction des themes dans tous les modes de couleur
  (`color_downgrade`, `themes_build_in_all_modes`,
  `terminal_theme_uses_reset_and_named_colors`).

## Tests d'integration (`tests/prototype.rs`)

Les tests utilisent des depots Git temporaires, une identite locale et
`GIT_CONFIG_NOSYSTEM=1` / `GIT_CONFIG_GLOBAL=/dev/null` pour les commandes
de preparation. Les lancements PTY isolent aussi les repertoires XDG.
Les scenarios ne demandent aucun acces reseau :

- `real_repository_renders_both_views_and_resizes` : rendu des deux vues sur
  un vrai depot (branches, merge, changements indexes/non indexes), plusieurs
  tailles de terminal.
- `icon_click_stages_file_and_read_only_refuses_mutations` : clic sur l'icone
  de statut indexe/desindexe ; `--read-only` refuse toute mutation.
- `double_click_does_not_stage_the_next_file_after_refresh` : un double-clic
  sur une liste de fichiers git gui n'ajoute pas une seconde mutation apres
  rafraichissement.
- `commit_keeps_unstaged_changes_and_clears_message` : un commit simple efface
  le message et conserve les changements non indexes.
- `menus_and_dialogs_have_topmost_click_targets` : empilement correct des
  zones cliquables (dialogue > menu > vue).
- `cli_commands_do_not_need_a_repository` : `--help`, `--version`, `doctor`
  fonctionnent hors depot et hors terminal interactif.
- `terminal_launch_switch_view_and_clean_exit` /
  `terminal_launch_at_79_columns_switches_view_and_exits` /
  `check_terminal_launcher` /
  `both_launchers_and_about_texts_use_the_new_names` : lancement reel en
  pseudo-terminal (portable-pty + vt100) des deux binaires `gitt` et
  `git-tui`, bascule de vue (`F6`), sortie propre, vue par defaut de chaque
  point d'entree, tailles 120 x 35, 79 x 24 et 79 x 26, textes "A propos" a jour.
- `compact_panes_keep_controls_accessible_and_return_to_full_layout` : mode
  compact en 79 x 24, 79 x 26 et 80 x 24, onglets, retour au layout complet.
- `commit_guide_tracks_display_columns_without_overwriting_text` : guide de
  largeur de message, colonnes d'affichage Unicode.
- `long_dialog_manual_scroll_stays_put_and_buttons_remain_clickable` :
  defilement manuel d'un long formulaire, boutons toujours cliquables.
- `fine_selection_copies_graphemes_and_never_becomes_a_partial_patch` :
  selection fine de texte dans un diff (copie uniquement, jamais une mutation
  Git partielle).
- `persistence_errors_and_quit_confirmation_are_recoverable` : erreurs de
  sauvegarde explicites, confirmation de sortie avec un brouillon non vide,
  recuperation apres echec.
- `terminal_commit_selection_preserves_color_families` : la selection d'un
  commit preserve les familles de couleurs du theme terminal dans le rendu.

## Ce qui n'est pas encore couvert

Voir **Next priorities** dans `status.md` pour la liste a jour. En resume,
manquent des scenarios d'integration dedies pour :

- indexation/annulation partielles sur des hunks multiples, `amend`,
  conflits de fusion ;
- noms de fichiers inhabituels (espaces, retours a la ligne, octets non
  UTF-8, sequences d'echappement) au-dela de ce que couvrent les tests
  `domain::sanitize`/`domain::path` ;
- transfert (hand-off) vers l'editeur externe et la signature de commit ;
- recuperation apres echec au milieu d'une operation (verrou concurrent, hook
  qui refuse, commande interrompue) ;
- le nouveau flux de clic sur un tag (`show_tag_info` / `on_tag_info`,
  `src/app/history.rs`) n'a pour l'instant qu'une verification manuelle et la
  couverture par les tests unitaires existants sur le reste du module ; un
  test d'integration dedie (tag annote et tag leger, sur un depot temporaire)
  reste a ecrire.

## Ce qui ne peut pas etre teste ici

- Pas de captures gitk/git gui de reference : ni `Xvfb` ni `xvfb-run` ne sont
  disponibles dans cet environnement (voir `docs/limitations.md`).
- Aucun test macOS n'a ete execute (pas de machine macOS disponible).
- Les benchmarks sur un historique de grande taille (section 10.2 de
  `design.md`, cible : premier affichage utile en moins de 2 s sur 100 000
  commits) n'ont pas encore ete mesures ; a faire avant toute annonce de
  performance.

## CI

`.github/workflows/ci.yml` execute sur les branches et pull requests, avec
la toolchain de `rust-toolchain.toml`, les controles suivants sous Ubuntu 22.04 :

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 tests/test_uninstall.py
python3 scripts/package-release.py
```

Le formatage et Clippy strict sont des portes de qualite obligatoires et
passent localement, sans desactivation globale de lints.
Le script compile les deux executables en mode release pour
`x86_64-unknown-linux-gnu`, controle leurs versions et fabrique l'archive avec
documentation, licences et SHA-256.

Deux jobs d'installation consomment **la meme archive** sur Ubuntu 22.04
et 24.04 : verification SHA-256, extraction, installation dans `~/bin`,
`gitt --help/--version`, `git tui -h`, `git-tui --help`,
`git tui --version` et `git tui doctor`.
Cette verification n'utilise pas Cargo pour installer les binaires ; les tests
PTY restent executes depuis les sources dans le job de qualite.
Les jobs executent ensuite le script de desinstallation fourni dans l'archive
et verifient la disparition des deux binaires.

`tests/test_uninstall.py` utilise uniquement Python et des repertoires
temporaires : suppression manuelle, liens symboliques, conservation des autres
fichiers et des donnees, absence de binaires, refus des repertoires et arguments
invalides, transmission des arguments a Cargo et propagation de ses erreurs.

Le workflow `release.yml` reutilise la CI avant de creer un brouillon de release
sur un tag `v*`. Un tag different de la version Cargo est refuse. Voir la
[procedure de publication](build-install.md#4-preparer-une-release-maintenance).
Les workflows doivent encore etre executes sur le depot GitHub officiel :
une execution locale ne valide pas a elle seule les runners heberges.
