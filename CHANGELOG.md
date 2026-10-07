# Notes de version

## 0.1.0 — premiere release en preparation

Premiere distribution Linux x86_64 de Git Twin, client Git pour terminal
inspire de gitk et git gui.

### Fonctionnalites

- Deux points d'entree : `gitt` pour History, `git tui` / `git-tui` pour
  Worktree ; bascule entre les deux vues avec F6.
- Historique avec graphe, branches, tags, recherche, diffs, option `--all`
  et filtrage par chemins (`gitt chemin/fichier.c`, comme gitk).
- Indexation/desindexation par fichier, bloc ou lignes, edition du message de
  commit et sauvegarde des brouillons.
- Clavier, souris, menus, panneaux redimensionnables, mode compact,
  themes dark/light/terminal et modes ASCII/Unicode.
- Mode `--read-only`, aide et diagnostic local avec `doctor`.

### Distribution

- Archive Linux x86_64 GNU avec les deux executables, documentation, licences
  des dependances et somme SHA-256 ; installation utilisateur sans Rust.
- Compilation depuis les sources avec Rust 1.93.1 et dependances verrouillees.
- Installation dans `~/bin` et script de desinstallation pour les installations
  manuelles ou Cargo, sans supprimer configuration, brouillons ou depots.
- CI stricte (formatage, Clippy, tests), fabrication d'archive et verification
  d'installation sur Ubuntu 22.04/24.04 avant creation d'une release en brouillon.

### Limites connues

Les binaires GNU demandent glibc >= 2.35 ; pas de binaire ARM, musl, Windows
ou macOS dans cette premiere distribution. Git >= 2.25 est exige au lancement,
mais la version minimale n'a pas encore ete testee directement.
La parite complete avec gitk/git gui, les gros historiques et les scenarios
avances (conflits, rebase, signature, hooks et transport distant) ne sont pas
tous valides de bout en bout. Voir [docs/limitations.md](docs/limitations.md).
