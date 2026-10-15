# Notes de version

## 0.3.1 — 2026-10-15

### Nouveautes
- Le mode « Compact refs » est memorise dans l'etat (`state.toml`) et
  restaure au lancement.
- Comme gitk, l'historique garde le sommet de depart (HEAD au lancement) apres
  un checkout, cherry-pick, etc., en plus de la branche courante. `F5`
  (Update) conserve cet etat ; `Shift-F5` (Reload) le remet a zero.
- Mode « Compact refs » (menu View ou case en bas a droite de l'historique) :
  un marqueur par type de ref (`[T]`, `[B]`, `[R]`, ou icones en mode Nerd) ;
  un clic deplie les noms du commit, un clic sur la fleche les replie.
- « Messages log » dans le menu Help (donc dans la palette de commandes).

### Changements
- Les checkouts de branche ou de commit relisent l'historique pour y inclure
  la nouvelle branche courante.
- `Suppr` dans le champ SHA1 efface tout le champ.

## 0.3.0 — 2026-10-12

Mise a jour depuis le tag `v0.2.0` : palette de commandes, fichiers ouverts
dans l'editeur et messages d'etat plus clairs.

### Nouveautes

- Palette de commandes sur `F12` : recherche floue de toutes les commandes
  activees de la vue courante et du menu contextuel de la selection. Les
  commandes recemment lancees apparaissent en premier ; les commandes
  indisponibles sont grisees et ne se lancent pas.
- Visualiseur de fichier avec coloration syntaxique (Rust, Python, JS/TS, Go,
  C/C++, Shell, TOML, YAML, JSON, Markdown, reStructuredText), defilement
  horizontal et vertical, numeros de ligne.
- « Open in editor » sur les fichiers des panneaux patch (History et
  Worktree) ; les editeurs terminaux (vim, nvim, nano, hx...) prennent le
  terminal pendant l'edition.
- Barre d'etat : « Ready » ou « Working » selon l'activite reelle, suivi du
  dernier message. Clic sur le message : historique des messages.

### Changements

- Messages de fin d'operation explicites (nom de la commande et resultat).
- Entree « Online Documentation » retiree du menu Help de Worktree.

### Distribution

- Versions `Cargo.toml`, `Cargo.lock` et documentation portees a 0.3.0.
- Archive attendue : `git-twin-0.3.0-x86_64-unknown-linux-gnu.tar.gz`.

## 0.2.0 — 2026-10-08

Mise a jour depuis le tag `v0.1.0` : themes, panneau de preferences, gestion
des stash et interface Worktree affinee.

### Nouveautes

- Clic droit sur un fichier (panneau patch de History et Worktree) : « Open
  in editor ». L'editeur se choisit dans les preferences (`editor` dans la
  configuration, `code` par defaut). Les editeurs terminaux (vim, nvim, nano,
  hx...) prennent le terminal pendant l'edition.
- Themes Gruvbox ajoutes, avec un menu deroulant dans les preferences ;
  contraste de la selection et de l'onglet actif ameliore.
- Liste de references accessible avec F2 ; aide des raccourcis clavier
  revue.
- Historique limite aux chemins passes en ligne de commande (`gitt chemin`).
- Graphe : noeud de commit unifie et lignes de connexion optionnelles ; sur
  les gros historiques, lisibilite amelioree.
- Worktree : controles de commit revus, icones et libelles de dialogues
  clarifies.
- Stash ajoute ; processus de checkout simplifie.

### Distribution

- Versions `Cargo.toml`, `Cargo.lock` et documentation portees a 0.2.0.
- Archive attendue : `git-twin-0.2.0-x86_64-unknown-linux-gnu.tar.gz`.

### Limites connues

Identiques a la 0.1.0 : voir [docs/limitations.md](docs/limitations.md).

## 0.1.0 — premiere release en preparation

Premiere distribution Linux x86_64 de Git Twin, client Git pour terminal
inspire de gitk et git gui.

### Fonctionnalites

- Deux points d'entree : `gitt` pour History, `git tui` / `git-tui` pour
  Worktree ; bascule entre les deux vues avec F6.
- Historique avec graphe, branches, tags, recherche, diffs, option `--all`
  et filtrage par chemins (`gitt chemin/fichier.c`, comme gitk).
- Graphe lisible sur les gros historiques : largeur plafonnee (colonnes en
  trop resumees par `…`) et, comme gitk, coupure des longues lignes inactives
  par des pointilles gras `┇` cliquables qui menent l'un a l'autre (reglables
  dans Preferences).
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
