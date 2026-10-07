# Git Twin

Prototype de client Git pour terminal, avec une vue **History** inspiree de
gitk et une vue **Worktree** inspiree de git gui. Le prototype est executable ;
la parite complete avec les applications de reference n'est pas encore acquise.
**`gitt`** lance Git Twin en vue History ; **`git tui`** lance Git TUI en vue
Worktree, via l'executable **`git-tui`**. Les deux commandes donnent acces aux
deux vues ; `--view` remplace leur vue initiale.

## Essayer rapidement

Prerequis : Linux, Git et Rust 1.93.1 (version fixee par `rust-toolchain.toml`).
Le programme refuse Git < 2.25 ; ce minimum reste a valider sur cette version.
Les essais actuels utilisent Git 2.43.

```sh
cargo build

# Apercu texte des deux vues, sur un depot temporaire de demonstration.
# Aucune modification de vos depots ou de votre configuration Git.
cargo run --example preview -- --demo

# Interface interactive sur votre depot.
./target/debug/gitt --repo /chemin/vers/depot --read-only
export PATH="$PWD/target/debug:$PATH"
git tui --repo /chemin/vers/depot
```

Pour installer les deux commandes dans `~/.cargo/bin` : `cargo install --path . --locked`.

L'application ne cree pas de depot si le chemin fourni n'en contient pas.
Le repertoire des sources n'est pas encore initialise en depot Git :
utiliser `--repo` pour le lancement, ou la demonstration ci-dessus.

Taille conseillee : **120 x 35** cellules. Taille minimale : **80 x 24**.
Entre ces tailles, le mode compact affiche un panneau a la fois, avec des onglets
cliquables et navigation par Tab. `F11` agrandit/restaure le panneau actif.
En dessous de la taille minimale, les actions sont bloquees : agrandir le
terminal pour traiter une confirmation ou une erreur avant de quitter.

## Utilisation

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
gitt doctor
gitt --repo /chemin/vers/depot --no-mouse --icons ascii
git tui --repo /chemin/vers/depot --theme light --colors 256
```

`--help`, `--version` et `doctor` fonctionnent sans depot ni terminal interactif.
`doctor` n'effectue aucun appel reseau.

Les preferences sont stockees sous `$XDG_CONFIG_HOME/git-twin`, l'etat et les
brouillons sous `$XDG_STATE_HOME/git-twin`, avec les replis XDG habituels.
Le presse-papiers OSC 52 est optionnel et desactive par defaut.

## Developpement

```sh
cargo test
cargo run --example preview -- /chemin/vers/depot
```

Les tests du prototype utilisent des depots temporaires et un pseudo-terminal.
Voir [status.md](status.md) pour les travaux restants,
[design.md](design.md) pour la specification, et
[docs/limitations.md](docs/limitations.md) pour les adaptations et limites.
