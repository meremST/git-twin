# Installation et distribution Linux

Git Twin fournit deux executables : `gitt` (vue History) et `git-tui`
(vue Worktree, aussi accessible par `git tui`). Installer les deux.
La premiere distribution cible Linux x86_64 ; macOS, Windows et ARM ne sont
pas annonces comme supportes.

## 1. Installer une archive binaire (sans Rust)

Prerequis d'execution :

- Linux x86_64, glibc >= 2.35 et bibliotheque d'execution GCC (`libgcc_s.so.1`).
  Les archives sont construites sous Ubuntu 22.04 ; la CI prevoit leur
  installation sous Ubuntu 22.04 et 24.04. Debian 12 dispose aussi des
  bibliotheques requises, mais n'est pas dans cette matrice.
- Git dans `PATH`. Le programme refuse Git < 2.25 ; ce minimum n'a pas encore
  ete valide sur une installation reelle. Les essais locaux utilisent Git 2.43.
- Un terminal interactif UTF-8, au moins 79 x 24 cellules (120 x 35 conseille).
  Aucune Nerd Font n'est necessaire avec les icones Unicode par defaut.

Ces binaires ne sont **pas statiques** : Alpine Linux (musl), les systemes
avec une ancienne glibc et les machines ARM doivent utiliser une compilation
native, sans garantie de support avant validation sur ces plateformes.

Une fois la version publiee, ouvrir la page **Releases du depot officiel** et
telecharger les deux fichiers dans un meme repertoire :

```text
git-twin-0.1.0-x86_64-unknown-linux-gnu.tar.gz
git-twin-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Aucune URL publique n'est encore configuree dans ce depot local. Ne pas
utiliser une URL de telechargement devinee ou un paquet homonyme.

Depuis le repertoire de telechargement :

```sh
sha256sum --check git-twin-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf git-twin-0.1.0-x86_64-unknown-linux-gnu.tar.gz
cd git-twin-0.1.0-x86_64-unknown-linux-gnu
install -d "$HOME/bin"
install -m 0755 gitt git-tui "$HOME/bin/"
export PATH="$HOME/bin:$PATH"
gitt --version
git tui --version
git tui doctor
```

**Arreter si la verification SHA-256 echoue.** La somme detecte une corruption ;
elle ne constitue pas une signature d'authenticite. Telecharger les deux
fichiers depuis la meme release officielle via HTTPS.
L'archive inclut le script `scripts/uninstall.sh`, la documentation, la licence du projet et les licences
des dependances Cargo de compilation/execution sous `licenses/`.

Rendre l'ajout au `PATH` permanent dans le fichier de demarrage de votre shell
(`~/.profile` ou `~/.bashrc` pour Bash selon la session, `~/.zshrc` pour Zsh) :

```sh
export PATH="$HOME/bin:$PATH"
```

Ouvrir ensuite un nouveau terminal. Aucun `sudo` n'est necessaire pour cette
installation. Ne pas melanger cette methode avec une installation Cargo.

## 2. Compiler et installer depuis les sources

Necessaires : Git, un compilateur/linker C, Rust et Cargo geres par `rustup`.
Python n'est necessaire que pour fabriquer une archive de distribution,
pas pour compiler ou executer l'application.

Installer les outils systeme avec le gestionnaire de votre distribution :

```sh
# Debian / Ubuntu
sudo apt-get update
sudo apt-get install --yes build-essential git curl ca-certificates

# Fedora (alternative)
sudo dnf install gcc git curl ca-certificates

# Arch Linux (alternative)
sudo pacman -S --needed base-devel git curl ca-certificates
```

Installer Rust selon <https://rustup.rs/>. Pour examiner le script avant
execution :

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o rustup-init.sh
less rustup-init.sh
sh rustup-init.sh
. "$HOME/.cargo/env"
```

Recuperer les sources du tag `v0.1.0` depuis la release officielle (archive
source GitHub ou clone du depot), puis ouvrir le repertoire contenant
`Cargo.toml`. Ne pas copier uniquement `src/` : conserver `Cargo.lock` et
`rust-toolchain.toml`. Depuis ce repertoire :

```sh
rustc --version
cargo install --path . --locked --root "$HOME"
export PATH="$HOME/bin:$PATH"
gitt --version
git tui doctor
```

`rustup` selectionne et installe automatiquement Rust **1.93.1** avec rustfmt
et Clippy. Pas de nightly. Le premier telechargement de la toolchain et des
dependances necessite le reseau ; `--locked` conserve les versions de
`Cargo.lock`. Ne jamais lancer Cargo avec `sudo`.

L'option `--root "$HOME"` installe les deux commandes dans `~/bin/gitt` et
`~/bin/git-tui` : Cargo ajoute lui-meme le sous-repertoire `bin` a la racine.
Elle ne deplace ni Rust ni son cache de dependances. Cargo conserve ses
metadonnees d'installation dans `~/.crates.toml` et `~/.crates2.json` ;
utiliser la meme option `--root "$HOME"` pour mettre a jour ou desinstaller.
Conserver l'ajout de `~/bin` au `PATH` dans la configuration du shell, comme
indique en section 1.
Les builds Cargo install sont optimises en mode release par defaut.
`cargo install git-twin` depuis crates.io n'est pas propose :
`publish = false` est volontaire tant que cette distribution n'est pas decidee.

Pour compiler sans installer :

```sh
cargo build --locked --release --bins
./target/release/gitt --version
./target/release/git-tui doctor
./target/release/gitt --repo /chemin/vers/depot --read-only
```

## 3. Mise a jour et desinstallation

Pour une archive : telecharger la nouvelle version et sa somme, verifier,
puis remplacer **les deux** executables avec la procedure de la section 1.
Fermer l'application avant remplacement.

Pour une installation Cargo : recuperer les sources du nouveau tag, puis :

```sh
cargo install --path . --locked --root "$HOME" --force
gitt --version
git tui --version
```

Verifier la copie effectivement utilisee avec `command -v gitt` et
`command -v git-tui`. Un alias Git `tui` peut masquer la sous-commande :
inspecter `git config --get alias.tui` et lancer `git-tui` directement si besoin.

Fermer l'application, puis lancer le script depuis la racine des sources ou
de l'archive extraite, selon la methode d'installation :

```sh
# Installation Cargo uniquement
sh scripts/uninstall.sh --cargo

# Installation manuelle dans ~/bin uniquement
sh scripts/uninstall.sh
```

Sans option, le script supprime uniquement `~/bin/gitt` et `~/bin/git-tui`
(y compris les liens symboliques, sans supprimer leur cible). Une commande
deja absente est signalee sans erreur ; un repertoire a sa place est refuse.
Utiliser **`--cargo` pour une installation geree par Cargo** : cette option
execute `cargo uninstall --root "$HOME" git-twin`, afin de mettre aussi a jour
ses metadonnees. Si Cargo est absent ou echoue, le script signale l'erreur sans
basculer vers une suppression manuelle. Ne pas utiliser `sudo`.
`sh scripts/uninstall.sh --help` affiche l'aide.

La configuration et les brouillons sont conserves :

| Contenu | Chemin par defaut | Variable XDG |
| --- | --- | --- |
| Preferences | `~/.config/git-twin` | `XDG_CONFIG_HOME` |
| Etat et brouillons | `~/.local/state/git-twin` | `XDG_STATE_HOME` |
| Cache | `~/.cache/git-twin` | `XDG_CACHE_HOME` |

Avec une variable XDG absolue non vide, le sous-repertoire `git-twin` est
utilise sous cette variable. Ne supprimer ces donnees que volontairement,
apres avoir recupere les brouillons utiles. Aucun depot Git n'est a supprimer.

## 4. Preparer une release (maintenance)

Le workflow `.github/workflows/ci.yml` execute le formatage, Clippy strict,
les tests unitaires/integration/PTY, puis construit une archive et installe
les deux commandes sur Ubuntu 22.04 et 24.04. Les pull requests ont uniquement
des permissions de lecture sur le depot.

Avant de creer un tag :

1. Mettre a jour la version dans `Cargo.toml` et `Cargo.lock`, les exemples
   d'installation et `CHANGELOG.md` (date, changements et limites).
2. Executer les controles ci-dessous, revoir les licences et essayer
   l'interface installee sur un depot jetable.
3. Apres configuration du depot GitHub officiel et validation de la CI,
   pousser le commit puis un tag annote `v0.1.0` correspondant exactement a la
   version Cargo. Ne jamais deplacer un tag deja publie.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 scripts/package-release.py --tag v0.1.0
```

La fabrication de l'archive demande Python >= 3.8 (bibliotheque standard
uniquement), Linux x86_64 et la toolchain epinglee. Le script compile avec
`--locked --release --target x86_64-unknown-linux-gnu`, verifie les versions
des deux binaires, collecte les licences des dependances actives hors
dependances de test et produit l'archive et sa somme sous `target/dist/`
(ou sous le repertoire cible personnalise de Cargo).
Une licence absente ou un tag incoherent fait echouer la fabrication.
Une archive construite sur une distribution plus recente peut exiger une
glibc plus recente : **distribuer l'archive de la CI Ubuntu 22.04**, pas un
binaire local d'origine non controlee.

Un push de tag `v*` declenche `.github/workflows/release.yml`, qui rejoue
toute la CI. Apres succes, il cree une **release en brouillon**, jamais une
publication automatique, contenant les binaires, SHA-256 et notes de version.
Le mainteneur doit revoir le brouillon, les limites et les liens avant de
cliquer sur **Publish release**. Une relance avec un brouillon deja cree
echoue plutot que de remplacer silencieusement des fichiers existants.
Les archives de sources sont fournies par GitHub a partir du tag.

## 5. Depannage

| Probleme | Verification / resolution |
| --- | --- |
| `cargo` introuvable | Charger `. "$HOME/.cargo/env"` et verifier `PATH` |
| Linker `cc` absent | Installer le compilateur/linker systeme |
| `GLIBC_x.y not found` | Utiliser une distribution compatible ou compiler depuis les sources ; ne pas remplacer manuellement la glibc systeme |
| `Exec format error` | Verifier `uname -m` : l'archive est pour x86_64, pas ARM |
| Echec de telechargement | Verifier reseau, proxy et certificats ; ne pas desactiver TLS |
| Lockfile incompatible | Utiliser la toolchain fixee et les sources completes ; ne pas effacer `Cargo.lock` |
| Glyphes incorrects | Utiliser un terminal UTF-8 ou `--icons ascii` ; les Nerd Fonts sont optionnelles |
| Souris indisponible | Tester hors tmux, verifier sa configuration ou utiliser `--no-mouse` |
| Mauvais binaire lance | Comparer `command -v gitt`, `command -v git-tui` et leurs `--version` |
| `git: 'tui' is not a git command` | Ajouter le repertoire contenant `git-tui` au `PATH` |
| `git tui --help` cherche une page man | Git intercepte `--help` ; utiliser `git tui -h` ou `git-tui --help` (aucune page man n'est fournie) |
| Terminal mal restaure | Apres sortie du processus, executer `stty sane` ou `reset`, puis signaler le bug |
| Git < 2.25 | Mettre a jour Git ; le minimum est verifie au demarrage |

Pour signaler un probleme, joindre la version de Git Twin, la distribution,
l'architecture, le terminal et la sortie de `gitt doctor`, apres avoir retire
les chemins ou informations sensibles. `doctor` fonctionne hors depot,
sans terminal interactif et sans appel reseau.
