# Git Twin - Specification de conception et de realisation

## 1. Statut et objectif

Ce document est le cahier des charges de l'application. Les commandes et
comportements decrits constituent le contrat cible ; l'etat reel du prototype
et ses limites sont consignes dans `status.md` et `docs/limitations.md`.

Nom : **Git Twin**, commande `gitt` (vue History), et **Git TUI**, commande
`git tui` via le binaire `git-tui` (vue Worktree). Le package Cargo reste `git-twin`.
Verifier la disponibilite du nom avant toute publication publique.

Construire un client Git dans le terminal, avec deux vues reproduisant les
dispositions, fonctions et interactions de **gitk** et **git gui**. L'esthetique
s'inspire de LazyVim et des Nerd Fonts, sans copier ses raccourcis au detriment
des interactions des outils de reference.

Priorites, dans cet ordre :

1. Integrite du depot et exactitude des operations Git.
2. Fidelite fonctionnelle et gestuelle aux outils de reference.
3. Fidelite de disposition des panneaux.
4. Fluidite, lisibilite et finition graphique.

Ce projet n'est pas une variante de Lazygit. Ne pas remplacer les dispositions
de reference par une interface de type tableau de bord, une palette de commandes
unique ou une succession d'ecrans plein format.

## 2. Plateformes et limites explicites

- Premiere plateforme prise en charge : Linux, terminal interactif UTF-8.
- Valider xterm, un terminal moderne comme Kitty ou WezTerm, le terminal integre
  de VS Code, puis leur utilisation sous tmux et via SSH.
- macOS et Windows sont des extensions ulterieures, non garanties par la v1.
- Le terminal doit disposer du clavier et peut disposer de la souris ; toutes
  les operations doivent rester accessibles sans souris.
- Truecolor et Nerd Font sont recommandes, jamais obligatoires. Fournir des
  modes couleurs reduites et icones ASCII.
- La police est configuree dans le terminal local, pas chargee par l'application.
- Une TUI ne peut pas garantir tous les gestes d'une application graphique :
  raccourcis interceptes, selection native de texte, presse-papiers et precision
  du pointeur dependent du terminal. Documenter chaque adaptation.

Objectif de disposition : panneaux de reference visibles a partir de 120 x 35
cellules. Entre 80 x 24 et ce seuil, autoriser le masquage explicite d'un panneau
ou son agrandissement temporaire. En dessous de 80 x 24, afficher une indication
de taille insuffisante sans executer d'action cachee. Ces seuils devront etre
confirmes sur le prototype et tout changement consigne.

## 3. Reference et matrice de parite

Avant de coder les operations Git, observer les versions de reference installees
de gitk et git gui. Consigner Git, les versions ou revisions des deux outils,
la distribution, la locale, la configuration et les captures associees. Si une
version n'est pas exposee directement, relever le paquet ou la revision source.
Ne pas inventer les menus depuis les souvenirs d'un modele.

Produire `docs/parity.md`, avec une ligne par commande, option, menu, dialogue,
geste et comportement observable. Utiliser les colonnes suivantes :

| ID | Outil/version | Emplacement | Geste | Preconditions | Effet et resultat | Equivalent TUI | Test | Statut/ecart |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GUI-STAGE-FILE | A relever | Liste non indexee | Geste a relever | Fichier modifie | Index mis a jour | A implementer | Depot temporaire | A inventorier |

Statuts autorises : a inventorier, specifie, implemente, verifie, adaptation
documentee, bloque. Une fonction absente n'est pas une adaptation valide.

Relever notamment :

- Historique : graphe, colonnes, references, navigation, recherches et filtres,
  definition des vues, menus contextuels, comparaisons, details et preferences.
- Travail : listes indexees/non indexees, fichiers non suivis, diffs, selection,
  indexation par fichier/bloc/ligne selon les possibilites de la reference,
  message de commit, amendement, rescannage et annulation des modifications.
- Operations exposees : branches, tags, checkout, cherry-pick, reset, remotes,
  fetch/push, conflits, blame, outils externes et dialogues de configuration.
  L'inventaire determine a quel outil elles appartiennent et leur comportement.
- Cas limites : depot vide, HEAD detache, historique incomplet, aucun changement,
  operation Git en cours, erreur, fermeture et restoration des preferences.

Une commande supplementaire peut etre proposee apres la parite. Elle ne doit
ni remplacer une fonction de reference ni retarder les jalons principaux.
Ne revendiquer la parite complete que lorsque chaque ligne a ete verifiee ou
qu'une adaptation de plateforme a ete explicitement acceptee par le demandeur.

## 4. Interface et interactions

### 4.1 Vue Historique

Schema indicatif a ajuster d'apres les captures de reference :

```text
+-----------------------------------------------------------------------+
| Menus gitk equivalents                                                 |
+-----------------------------------------------------------------------+
| Graphe | Sujet du commit                   | Auteur       | Date       |
|        |                                   |              |            |
+-----------------------------------------------------------------------+
| Recherche / navigation selon disposition de reference                  |
+----------------------------------------------+------------------------+
| Details du commit et diff                    | Liste/arbre des fichiers|
|                                              |                        |
+----------------------------------------------+------------------------+
| Etat / progression / erreur non modale                                 |
+-----------------------------------------------------------------------+
```

Le graphe represente correctement les parents, merges, branches et references.
La navigation conserve l'identite du commit selectionne lors d'un rafraichissement.
Les recherches ne doivent pas se limiter silencieusement aux lignes deja chargees.
Les comparaisons affichent clairement les revisions de depart et d'arrivee.
Les diffs de merge doivent expliciter le parent ou le mode de comparaison choisi.

### 4.2 Vue Travail

```text
+-----------------------------------------------------------------------+
| Menus git gui equivalents                                              |
+---------------------------+-------------------------------------------+
| Changements non indexes   | Diff du fichier selectionne                |
|                           |                                           |
+---------------------------+                                           |
| Changements indexes       |                                           |
|                           |                                           |
+---------------------------+-------------------------------------------+
| Actions de commit         | Message de commit                          |
|                           |                                           |
+---------------------------+-------------------------------------------+
| Etat / branche / operation en cours                                    |
+-----------------------------------------------------------------------+
```

Conserver le contexte selectionne entre les listes et le diff. Indiquer
distinctement les changements indexees et non indexes d'un meme fichier.
L'editeur de message prend en charge plusieurs lignes, annulation locale,
collage et lancement d'un editeur externe. Ne pas perdre un brouillon sur erreur.
Ne pas ajouter de correcteur de message ou de generation IA a la v1.

### 4.3 Contrat souris et clavier

- Clic : focus et selection selon le composant et le comportement de reference.
- Double-clic : action de reference lorsqu'elle existe ; detection testable du
  delai et de la position, sans double execution d'une mutation.
- Clic droit : menu contextuel ancre a la cible, contenu dependant de son etat.
- Molette : defilement du panneau survole, sans changement implicite de selection.
- Glissement d'un separateur : redimensionnement borne par les tailles minimales.
- Selection de texte dans le diff et le message, avec copie explicite.
- Menus accessibles au clavier ; focus visible ; echappement ferme le dialogue
  ou menu courant ; restoration du focus apres fermeture.
- Conserver les raccourcis de reference quand le terminal les distingue ;
  documenter les conflits et fournir une alternative configurable.
- Une meme action applicative est utilisee par clavier, souris et menu.

Les zones cliquables proviennent des rectangles calcules pour le rendu. Ne pas
dupliquer les coordonnees dans les gestionnaires d'evenements. Traiter Unicode,
caracteres combines, glyphes larges et defilement horizontal dans les calculs.
Les lignes de diff sont non repliees par defaut pour fiabiliser la selection.

Prevoir une aide accessible, une option de capture souris et des profils sans
Nerd Font. Le support du presse-papiers distant via OSC 52 est optionnel et
desactive par defaut ; ne jamais transmettre automatiquement de contenu.

### 4.4 Direction visuelle

Theme sombre inspire des interfaces de developpement, avec theme clair fourni.
Couleurs semantiques coherentes pour ajouts, suppressions, references, focus et
erreurs. Toujours ajouter un indicateur textuel aux etats importants.
Pas d'animation decorative, de clignotement ou de page d'accueil marketing.
Les icones sont choisies pour leur sens ; la version ASCII reste fonctionnelle.
Les preferences de tailles et de focus ne doivent pas rendre la vue inutilisable
apres un changement de terminal ou de dimensions.

## 5. Choix techniques

- Rust, edition 2024, Ratatui pour le rendu et Crossterm pour les evenements.
- Git CLI comme moteur ; pas de libgit2 ni de reimplementation des objets Git.
- `clap` pour la CLI, `serde` et TOML pour la configuration.
- Threads de travail et canaux bornes pour les taches Git ; pas de runtime async
  supplementaire tant qu'un besoin mesure ne le justifie pas.
- `tempfile` pour les depots de test ; tests de buffers Ratatui et scenarios PTY
  pour le rendu et les interactions terminal.
- Un package Cargo au depart. Ne separer en plusieurs crates que sur besoin reel.

Au jalon initial, choisir des versions compatibles, fixer une version exacte
de Rust stable dans `rust-toolchain.toml` et renseigner `rust-version` dans
`Cargo.toml`. Committer `Cargo.lock` et utiliser `--locked` en CI et dans les
procedures de build. Le minimum Git est determine par les commandes utilisees,
documente et teste ; ne pas annoncer un minimum non verifie.

## 6. Architecture et execution

Structure cible, a creer dans un nouveau depot dedie :

```text
git-twin/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  README.md
  LICENSE
  src/
    main.rs
    app/          # Etats, actions, orchestration, ordonnancement
    domain/       # Identites Git, selections, revisions, resultats
    git/          # Processus, parsers, lectures et mutations
    ui/           # Layouts, widgets, souris, themes
    config/       # Chargement et validation des preferences
  tests/          # Integration Git, fixtures et scenarios PTY
  docs/
    parity.md
    architecture.md
    build-install.md
    testing.md
    limitations.md
  .github/workflows/ci.yml
```

Choisir la licence avec le demandeur avant diffusion. Ne pas recopier de code
gitk/git gui, de themes ou d'assets sans verification de leurs licences.

Le domaine ne depend pas du terminal. L'adaptateur Git ne depend pas des widgets.
Les transitions d'etat sont explicites et testables. Eviter les fonctions
monolithiques melangeant dessin, parsing Git et modification du depot.

La boucle principale gere les evenements et le rendu, sans commande Git
bloquante. Les reponses asynchrones portent un identifiant de requete et une
generation du depot : ignorer celles rendues obsoletes par une nouvelle selection.
Limiter la concurrence des lectures et serialiser les ecritures de l'application.

Un processus Git externe peut modifier le depot simultanement. Les evenements
fichiers et le rafraichissement periodique sont des indices, pas une garantie de
coherence. Revalider l'etat avant mutation et relire l'etat apres celle-ci.

La fermeture arrete les lectures en cours et restaure le terminal. Une mutation
en cours n'est pas tuee automatiquement : presenter son etat et demander une
decision adaptee. Restaurer le mode terminal meme apres erreur ou panic.

## 7. Integration Git et securite

### 7.1 Lectures et rendu

- Utiliser `std::process::Command` avec des arguments separes, sans shell construit
  a partir de noms de fichiers, de references ou de messages.
- Privilegier les formats machine, notamment `status --porcelain=v2 -z` et les
  listes de chemins terminees par NUL. Desactiver pager et couleurs Git pour
  les sorties parsees. Ne pas parser les messages d'erreur localises.
- Preserver les chemins non UTF-8 sous Linux ; leur affichage echappe ne doit
  jamais devenir l'identite utilisee pour une commande Git.
- Distinguer arguments, revisions et chemins ; utiliser des pathspecs litteraux
  quand necessaire. `--` seul ne neutralise pas la syntaxe des pathspecs Git.
- Neutraliser les sequences de controle presentes dans les noms et messages
  avant rendu terminal. Aucun contenu du depot ne doit injecter d'echappements.
- Gerer les racines de depot, worktrees et depots bare par interrogation de Git,
  sans supposer que `.git` est un repertoire. En bare, desactiver la vue Travail.
- Desactiver par defaut les diff drivers externes et textconv dans les lectures
  de preview ; leur activation doit etre explicite et documentee.

Le graphe se construit a partir des relations de parents et d'un ordre defini,
pas depuis une sortie coloree `git log --graph`. Tester les merges octopus et
les parents hors filtre. Un chargement progressif doit conserver les voies du
graphe et les raccordements entre lots, sans renumerotation visuelle arbitraire.

### 7.2 Mutations

- Utiliser les commandes Git standard pour respecter hooks, identite, signatures,
  configuration, attributs et mecanismes d'authentification.
- Ne jamais desactiver automatiquement un hook, une signature ou un verrou.
- Une erreur de hook ou de signature laisse le message de commit disponible.
- Avant abandon de changements, reset destructif, suppression ou push force,
  montrer exactement la cible et les consequences puis demander confirmation.
- Ne pas promettre d'annulation universelle : reflog et operations inverses ne
  recuperent pas toutes les donnees non committees.
- Ne jamais supprimer automatiquement un fichier `index.lock` ni modifier les
  verifications `safe.directory`. Expliquer le diagnostic et les choix manuels.
- Respecter les operations en cours (merge, rebase, cherry-pick, revert) et
  presenter les actions continuer/abandonner seulement lorsqu'elles s'appliquent.

Pour indexer/desindexer partiellement, produire un patch structure correspondant
a la base correcte (index, arbre de travail ou HEAD), y compris le signe et le
contexte des lignes. Verifier son application avant mutation et gerer le refus
reel sans retenter sur un autre contenu. Une verification prealable ne supprime
pas les courses avec les processus externes.

Tester en particulier les fichiers nouveaux/supprimes, lignes sans fin de ligne,
CRLF, modes executables, liens symboliques et fichiers partiellement indexes.
Les binaires, renommages ambigus et entrees non fusionnees doivent proposer
uniquement les actions effectivement prises en charge, avec raison visible.

### 7.3 Sous-processus interactifs et donnees sensibles

Les editeurs, mergetools, hooks interactifs, SSH et signatures peuvent demander
le terminal. Prevoir un mode de transfert : suspendre le rendu et la capture,
restaurer le terminal, executer le sous-processus avec les E/S appropriees, puis
reactiver la TUI et rafraichir. Tester le retour apres echec comme apres succes.

Ne pas collecter de mot de passe dans un formulaire TUI maison. Respecter les
agents et credential helpers Git. Le reseau n'est sollicite que par une action
utilisateur explicite. Aucune telemetrie ni generation IA integree par defaut.
Masquer les identifiants presents dans les URLs ; ne pas journaliser messages,
patchs, tokens ou variables d'environnement sensibles par defaut.

## 8. CLI et configuration a implementer

```sh
gitt --help
gitt --version
gitt --repo /chemin/vers/depot
gitt --repo /chemin/vers/depot --view history
git tui --repo /chemin/vers/depot
gitt --repo /chemin/vers/depot --no-mouse --icons ascii
git tui --repo /chemin/vers/depot --read-only
gitt doctor
```

Sans `--repo`, decouvrir le depot depuis le repertoire courant. Sans `--view`,
ouvrir History avec `gitt`, Worktree avec `git tui` (History pour un depot bare).
Une option `--view` explicite remplace ce choix. En dehors d'un depot,
afficher une erreur claire sans en creer un. `--help`, `--version` et `doctor`
fonctionnent sans depot ni terminal interactif.

`--read-only` bloque toutes les commandes susceptibles de modifier le depot,
y compris fetch et outils externes non controles. Les lectures de ce mode
evitent aussi les mises a jour optionnelles de l'index par Git.

`doctor` rapporte versions, presence de Git, capacites detectables du terminal
et chemins de configuration, sans secrets et sans reseau. Il ne pretend pas
detecter la police installee : proposer un controle visuel des glyphes.

Configuration Linux : `$XDG_CONFIG_HOME/git-twin/config.toml`, avec repli sur
`~/.config/git-twin/config.toml`. Etat dans `$XDG_STATE_HOME/git-twin`, cache dans
`$XDG_CACHE_HOME/git-twin`, avec replis XDG usuels. Aucun fichier de preferences
cree dans le depot utilisateur. Pas de chargement automatique d'une configuration
executable provenant du depot. Ecritures atomiques et erreurs de configuration
lisibles ; priorite CLI, configuration utilisateur, valeurs par defaut.

## 9. Compilation, installation et utilisation

Cette section est normative pour la future implementation. La reproduire dans
`docs/build-install.md` et fournir un parcours rapide dans `README.md`.
Remplacer les decisions encore ouvertes par des versions effectivement testees.

### 9.1 Prerequis Linux

Necessaires : Git, compilateur/linker C, Rust et Cargo geres par rustup.
Pas besoin de Node.js, Python, service distant ou acces root pour lancer la TUI.
N'introduire de dependance native supplementaire que si elle est justifiee et
ajouter alors les paquets de developpement correspondants a cette procedure.

Exemple Debian/Ubuntu, a executer par l'utilisateur :

```sh
sudo apt-get update
sudo apt-get install --yes build-essential git curl ca-certificates
```

Installer rustup selon <https://rustup.rs/>. Pour eviter un script execute
directement depuis un pipe, il est possible de telecharger puis examiner :

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
less rustup-init.sh
sh rustup-init.sh
. "$HOME/.cargo/env"
rustup --version
git --version
```

Choisir une installation utilisateur standard. A l'entree dans le futur depot,
rustup selectionne la version fixee par `rust-toolchain.toml`. Le fichier doit
inclure les composants `rustfmt` et `clippy`. Ne pas exiger une toolchain nightly.
Documenter aussi les paquets equivalents pour les autres distributions testees.

### 9.2 Recuperer les sources et verifier l'environnement

L'URL du futur depot n'existe pas encore dans ce cahier des charges. Lors de sa
creation, ajouter au README une commande `git clone` reelle et un checkout de
tag pour les releases. Les commandes suivantes partent de la racine du projet :

```sh
cd /chemin/vers/git-twin
rustc --version
cargo --version
cargo metadata --locked --no-deps --format-version 1
cargo fetch --locked
```

Le mainteneur genere et committe initialement `Cargo.lock`. Un utilisateur ne
doit pas devoir le regenerer. L'acces reseau est necessaire pour obtenir la
toolchain et les dependances, sauf cache ou distribution vendoree prepares.

### 9.3 Compiler et tester

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
./target/release/gitt --version
./target/release/git-tui doctor
```

Les binaires attendus sont `target/release/gitt` et `target/release/git-tui`. Le build release doit
reussir avec les fonctions par defaut, sans variable secrete ni fichier prive.
Ne pas masquer les avertissements pour faire passer Clippy.

Pour developper ou lancer depuis les sources :

```sh
cargo run --locked -- --repo /chemin/vers/depot --view history
cargo run --locked -- --repo /chemin/vers/depot --view worktree
```

Ces deux commandes necessitent un terminal interactif. Les lancer sur un depot
jetable pour tester les mutations. Les tests automatises ne doivent jamais
utiliser les depots de travail reels du demandeur comme fixtures modifiables.

Une fois la toolchain et les dependances presentes, verifier aussi :

```sh
cargo build --locked --offline --release
```

### 9.4 Installer pour l'utilisateur

Methode recommandee, depuis la racine des sources :

```sh
cargo install --path . --locked
gitt --version
git tui doctor
```

Les deux binaires sont places dans `$CARGO_HOME/bin`, ou `~/.cargo/bin` par defaut.
Verifier que ce repertoire est dans `PATH`, via l'environnement configure par
rustup. Pas de `sudo cargo install`.

Alternative sans gestion d'installation Cargo, apres compilation release :

```sh
install -d "$HOME/.local/bin"
install -m 0755 target/release/gitt "$HOME/.local/bin/gitt"
install -m 0755 target/release/git-tui "$HOME/.local/bin/git-tui"
"$HOME/.local/bin/gitt" --version
```

Ajouter `~/.local/bin` au `PATH` du shell si necessaire. Choisir une seule methode
pour eviter de lancer une ancienne copie. Verifier la resolution avec
`command -v gitt` / `command -v git-tui` et la version avec `gitt --version`.

### 9.5 Mise a jour et desinstallation

Pour mettre a jour depuis les sources, recuperer une revision publiee, relancer
les controles de la section 9.3, puis :

```sh
cargo install --path . --locked --force
gitt --version
```

Pour une installation manuelle, recompiler et repeter la commande `install`.
Pour desinstaller une installation geree par Cargo :

```sh
cargo uninstall git-twin
```

Pour une installation manuelle dans le chemin indique ci-dessus :

```sh
rm -- "$HOME/.local/bin/gitt" "$HOME/.local/bin/git-tui"
```

Conserver configuration, etat et cache par defaut ; documenter leurs chemins
pour un nettoyage volontaire. Ne jamais supprimer un depot Git utilisateur.

### 9.6 Releases et depannage

La premiere release cible Linux x86_64. Documenter distribution de build,
architecture, exigences libc, versions Rust/Git testees et dependances dynamiques.
Fournir une archive versionnee contenant binaire, licence et instructions, avec
empreinte SHA-256. Ne pas promettre un binaire statique ou portable sans test.
Ajouter aarch64 uniquement apres compilation et execution validees sur cette cible.

| Probleme | Verification / resolution |
| --- | --- |
| `cargo` introuvable | Charger l'environnement rustup et verifier `PATH` |
| Linker `cc` absent | Installer le compilateur/linker systeme, dont `build-essential` sous Debian |
| Echec de telechargement | Verifier reseau, proxy et certificats ; ne pas desactiver TLS |
| Lockfile incompatible | Utiliser la toolchain fixee et la revision complete ; ne pas effacer le lockfile |
| Glyphes incorrects | Configurer une Nerd Font locale ou utiliser `--icons ascii` |
| Souris indisponible | Tester hors tmux, verifier sa configuration ou utiliser `--no-mouse` |
| Mauvais binaire lance | Comparer `command -v gitt`, `command -v git-tui` et `gitt --version` |
| Terminal mal restaure | Apres sortie du processus, executer `stty sane` ou `reset`, puis signaler le bug |
| Erreur Git | Afficher le contexte et le diagnostic expurge ; ne pas forcer la commande |

## 10. Strategie de tests et performances

### 10.1 Tests obligatoires

- Unitaires : parsers, transitions, geometrie, zones cliquables, graphe, largeur
  Unicode, selection et construction de patches partiels.
- Integration : depots temporaires controles, sans hooks ou configuration globale
  de l'utilisateur ; identite de test locale et absence de reseau par defaut.
- Operations : comparer HEAD, references, contenu/mode des entrees d'index et
  fichiers de travail, pas uniquement le texte des messages ou le code retour.
- Couverture : merges, renommages, binaires, espaces, tabulations, nouvelles lignes
  et octets non UTF-8 dans les chemins, CRLF, conflits, worktrees et sous-modules.
- Erreurs : verrou concurrent, hook refusant le commit, signature en echec,
  commande interrompue, depot change pendant une selection et manque de droits.
- Securite : sequences terminal malveillantes, arguments ressemblant a des options,
  aucune mutation en lecture seule et absence de secrets dans les diagnostics.
- Rendu : captures de buffers a tailles fixes, themes sombre/clair, ASCII/Nerd Font,
  menus pres des bords, dialogue long, resize et absence de chevauchement.
- PTY : sequences clavier/souris, double-clic, glissement, focus, transfert a un
  sous-processus et restauration du terminal, y compris sur erreur.

La CI execute fmt, Clippy, tests, build release et smoke tests CLI sur une machine
Linux propre. Les tests PTY deterministes doivent etre inclus dans `cargo test`
sur Linux ou disposer d'une commande separee explicitement documentee et executee
en CI. Les controles graphiques manuels restent traces dans `docs/testing.md`.

### 10.2 Cibles de performance

Cibles initiales, a mesurer sur une machine et des fixtures decrites dans le
rapport de test, sans les annoncer comme acquises :

- Premier affichage utile en moins de 2 s sur un depot local de 100 000 commits.
- Retour visuel d'une interaction locale en moins de 50 ms au 95e percentile,
  hors duree des commandes Git ; progression visible pour les lectures longues.
- Pas de chargement integral des diffs ou de l'historique avant le premier rendu.
- Memoire bornee par des caches explicites ; tester un million de commits et
  des diffs tres volumineux, sans maintenir un widget par ligne du depot.

Mesurer a froid et a chaud, en indiquant stockage, CPU, terminal et version Git.
Les gros diffs sont tronques ou charges progressivement avec indication explicite,
jamais silencieusement utilises comme source d'un patch partiel incomplet.

## 11. Plan d'execution pour l'IA

| Jalon | Travail | Condition de sortie |
| --- | --- | --- |
| J0 - Reference | Inventaire, captures, matrice, versions, squelette Cargo et CI | Projet compilable, CLI minimale testee, decisions consignees |
| J1 - Prototype | Deux layouts sur fixtures, souris, menus, clavier, resize, themes | Scenarios gestuels demontres, aucune mutation Git |
| J2 - Historique | Adaptateur de lecture, graphe, selection, diffs, recherche | Navigation correcte sur gros depot et tests de graphe passes |
| J3 - Travail local | Status, indexation, patches partiels, commit et amendement | Etats Git attendus verifies sur fixtures, hooks respectes |
| J4 - Parite | Toutes les autres lignes, operations distantes, conflits et outils externes | Chaque fonction inventoriee dispose de son scenario valide |
| J5 - Livraison | Profilage, matrice terminal, documentation et archive | Installation propre et desinstallation reproduites, ecarts acceptes |

Pour chaque tache, l'IA doit :

1. Identifier la ligne de parite et ses conditions de succes.
2. Lire l'implementation locale et les tests existants avant de modifier.
3. Implementer une tranche coherente de faible taille, sans refonte opportuniste.
4. Executer le test le plus cible, puis les controles obligatoires du jalon.
5. Mettre a jour statut, documentation, limitations et preuves de validation.
6. Rapporter commandes executees, resultats reels et verifications non effectuees.

Ne pas creer le projet dans un depot firmware existant. Si aucun emplacement
dedie n'est fourni, le demander avant de generer les sources. Ne pas faire de
commit, push, publication ou installation systeme sans autorisation explicite.
Ne pas utiliser les depots de l'espace de travail pour experimenter une mutation.

Ne pas marquer une fonction terminee parce qu'un bouton existe. Ne pas laisser
de faux succes, de donnees de demonstration dans les vues reelles ni de commande
destructive factice. Les stubs du prototype sont retires ou isoles dans les tests.

## 12. Definition de termine

La v1 est livrable seulement lorsque :

- Les deux vues sont utilisables a la souris et au clavier avec la disposition
  de reference, et la matrice ne contient aucune omission non acceptee.
- Les operations Git sont testees, leurs erreurs recuperables et les confirmations
  destructrices precises ; aucun secret n'est expose dans les traces par defaut.
- Le terminal est restaure apres sortie et apres transfert aux outils externes.
- Les controles Cargo et la CI passent ; les limites de plateforme sont publiees.
- Une personne ou une IA peut compiler, installer, lancer, mettre a jour et
  desinstaller depuis une machine Linux propre en suivant seulement le README
  et `docs/build-install.md`.
- Le rapport final distingue clairement ce qui a ete teste automatiquement,
  manuellement, sur quel environnement et ce qui reste non verifie.

Le premier risque a lever est la fidelite des gestes dans un terminal. Valider
J1 avant d'accumuler les commandes Git : si une interaction fondamentale est
impossible, presenter la limite et faire accepter son adaptation explicitement.