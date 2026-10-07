# Git Twin

Client Git pour terminal Linux, avec une vue **History** inspiree de
gitk et une vue **Worktree** inspiree de git gui.
**`gitt`** lance Git Twin en vue History ; **`git tui`** lance Git TUI en vue
Worktree, via l'executable **`git-tui`**. Les deux commandes donnent acces aux
deux vues ; `--view` remplace leur vue initiale.

La premiere version **0.1.0** propose l'historique avec graphe, branches et tags,
les diffs, l'indexation par fichier/bloc/lignes et l'edition des commits dans
une interface clavier et souris. La parite complete avec gitk/git gui n'est pas
revendiquee : consulter les [limites connues](docs/limitations.md), notamment
pour les operations avancees.

## Installation Linux

**Sans Rust** : une fois la release publiee, telecharger l'archive
`git-twin-0.1.0-x86_64-unknown-linux-gnu.tar.gz` et son fichier `.sha256`
depuis la page Releases du depot. Le binaire cible Linux **x86_64 avec glibc
2.35 ou plus recente** (Ubuntu 22.04/24.04, Debian 12, par exemple).
Il ne convient pas a Alpine/musl ni a ARM.
Voir le [guide d'installation](docs/build-install.md) pour verifier l'archive,
installer les deux commandes dans `~/bin` sans droits administrateur et
configurer `PATH`.

**Depuis les sources** : installer Git, un compilateur C et
[Rust via rustup](https://rustup.rs/), puis executer depuis les sources de la
version choisie :

```sh
cargo install --path . --locked --root "$HOME"
export PATH="$HOME/bin:$PATH"
gitt --version
git tui doctor
```

Les binaires sont installes dans `~/bin/gitt` et `~/bin/git-tui`.
Conserver l'ajout de `~/bin` au `PATH` dans la configuration de votre shell.

La toolchain de compilation est fixee a **Rust 1.93.1** par
`rust-toolchain.toml`. Rust n'est pas necessaire pour utiliser les binaires.
Git doit etre dans `PATH` ; le programme refuse Git < 2.25, mais ce plancher
reste a valider sur cette version. Les essais locaux utilisent Git 2.43.
La distribution initiale se fait par archives et sources, pas par crates.io.

### Desinstallation

Depuis les sources ou l'archive extraite, sans `sudo` :

```sh
sh scripts/uninstall.sh --cargo  # installation avec Cargo dans ~/bin
sh scripts/uninstall.sh          # alternative : installation manuelle dans ~/bin
```

Choisir la commande correspondant a l'installation. La configuration, les
brouillons et les depots sont conserves.

## Premier lancement

```sh
# Commencer sans autoriser de modification du depot.
gitt --repo /chemin/vers/depot --read-only

# Ouvrir le worktree pour indexer et committer.
git tui --repo /chemin/vers/depot
```

Sans `--repo`, le repertoire courant est utilise. L'application ne cree pas
de depot si le chemin fourni n'en contient pas. Git Twin utilise votre Git,
votre configuration, vos hooks et vos outils externes : n'ouvrez que des
depots de confiance. Le mode lecture seule n'est pas un bac a sable.

Taille conseillee : **120 x 35** cellules. Taille minimale : **79 x 24**.
Entre ces tailles, le mode compact affiche un panneau a la fois, avec des onglets
cliquables et navigation par Tab. `F11` agrandit/restaure le panneau actif.
En dessous de la taille minimale, les actions sont bloquees : agrandir le
terminal pour traiter une confirmation ou une erreur avant de quitter.

## Utilisation

Dans le panneau **Details**, la ligne **Tags** affiche uniquement les tags
(legers ou annotes) qui pointent sur le commit selectionne. Elle est masquee
si ce commit ne porte aucun tag. **Follows** et **Precedes** indiquent
separement les tags voisins dans l'historique.

| Commande | Effet |
| --- | --- |
| `F6` | Basculer entre History et Worktree |
| `Tab` / `Shift-Tab` | Changer de panneau ou de champ |
| `F10` / `Alt` + lettre du menu | Ouvrir les menus |
| `Shift-F10` / clic droit | Menu contextuel |
| `F1` | Aide clavier |
| `F11` | Agrandir / restaurer le panneau actif |
| `Ctrl-Q` | Confirmer si le message est non vide, sauvegarder le brouillon et quitter |
| Glisser dans un diff / `Shift-Gauche/Droite` | Selectionner du texte precis pour la copie |
| `Shift-Haut/Bas` dans un diff | Selectionner des lignes entieres pour l'indexation |
| Molette / `Alt-PageUp/PageDown` dans un formulaire | Faire defiler sans changer le champ actif |
| Clic sur un nom de fichier | Selectionner et afficher le diff |
| Clic sur la lettre d'etat du fichier | Indexer / desindexer |
| `s` dans le diff Worktree | Indexer / desindexer le bloc ou les lignes selectionnees |
| `r` dans le diff Worktree | Annuler les changements, apres confirmation |
| `Alt-4` dans Worktree | Saisir le message de commit |
| `Alt-Enter` dans Worktree | Creer le commit |

Glisser les separateurs pour redimensionner ; utiliser la molette pour faire
defiler le panneau survole. Le mode `--read-only` bloque aussi fetch, push et
les outils externes susceptibles de modifier le depot. La selection fine est
reservee a la copie : pour indexer ou annuler, utiliser la selection de lignes.
L'editeur affiche un guide a `commit_width` colonnes sans remplacer le texte.
Une erreur de sauvegarde est affichee ; un echec de sauvegarde a la sortie
garde l'application ouverte et le brouillon en memoire.

```sh
gitt --help
git tui -h
gitt doctor
gitt --repo /chemin/vers/depot --no-mouse --icons ascii
gitt --repo /chemin/vers/depot --all
gitt chemin/fichier.c
gitt --all -- src/ docs/
git tui --repo /chemin/vers/depot --theme light --colors 256
git tui --repo /chemin/vers/depot --theme terminal
```

`--all` affiche l'historique de toutes les refs (branches, tags, remotes),
comme `gitk --all`, au lieu de se limiter a HEAD. Comme `gitk chemin...`,
des chemins en argument (relatifs au repertoire courant, eventuellement apres
`--`) limitent l'historique aux commits qui les modifient ; les diffs et les
lignes de modifications locales sont filtres de la meme facon. Le theme `terminal`
utilise les couleurs par defaut et ANSI du terminal :
l'interface suit la palette configuree par l'utilisateur dans son
emulateur, plutot que d'imposer `dark`/`light`.
Il conserve le fond du terminal, avec un accent jaune ANSI pour le focus et
des boutons sans aplats. Le commit selectionne inverse le fond et la couleur
de base du texte sur toute la ligne, avec du gras lorsque le panneau a le focus.
Sur cette ligne, les textes colores utilisent des variantes sombres de la meme
famille (vert fonce, bleu fonce, etc.), adaptees a un fond inverse clair.
Cette palette dediee utilise du RGB ou 256 couleurs selon `--colors` ; en mode
16 couleurs, les couleurs ANSI d'origine sont conservees. Les autres selections
conservent leurs couleurs sur un fond bleu (actif) ou gris (inactif).

`--help`, `--version` et `doctor` fonctionnent sans depot ni terminal interactif.
`doctor` n'effectue aucun appel reseau.
Pour l'aide via Git, utiliser `git tui -h` ou `git-tui --help` :
`git tui --help` est intercepte par Git et cherche une page man non fournie.

Les preferences sont stockees sous `$XDG_CONFIG_HOME/git-twin`, l'etat et les
brouillons sous `$XDG_STATE_HOME/git-twin`, avec les replis XDG habituels.
Le presse-papiers OSC 52 est optionnel et desactive par defaut.

## Developpement

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run --locked --example preview -- --demo
```

L'apercu `--demo` utilise un depot temporaire sans modifier vos depots.
Les tests utilisent des depots temporaires et un pseudo-terminal.
Voir [status.md](status.md) pour les travaux restants,
[design.md](design.md) pour la specification,
[docs/architecture.md](docs/architecture.md) pour l'organisation des couches,
[docs/build-install.md](docs/build-install.md) pour la compilation,
l'installation et la desinstallation detaillees,
[docs/testing.md](docs/testing.md) pour la suite et la CI,
[CHANGELOG.md](CHANGELOG.md) pour les notes de version, et
[docs/limitations.md](docs/limitations.md) pour les adaptations et limites.

## Licence

Apache-2.0, voir [LICENSE](LICENSE).
